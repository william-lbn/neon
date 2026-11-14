//! Compute运行时的专用管理连接。保留实例与进程内执行栅栏，不进行隐式重连换owner。

use std::{
    ops::Deref,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use compute_api::dsg_admission::DsgAdmissionRequest;
use compute_api::dsg_client::DsgSqlClient;
use compute_api::dsg_control::DsgUint64;
use compute_api::dsg_initialization::DsgInitializationResponse;
use compute_api::dsg_prepared::DsgPreparedResponse;
use compute_api::dsg_state::DsgPhase;
use compute_api::dynamic_shared_buffers::DynamicSharedBuffersSpec;
use compute_api::responses::ComputeStatus;
use tokio::sync::RwLockReadGuard;
use tokio::time::timeout;
use tokio_postgres::{Client, Config, NoTls};
use tracing::warn;

use crate::compute::{ComputeNode, ParsedSpec};

pub(crate) struct DsgManagementClient<'a> {
    client: &'a DsgSqlClient<'static>,
    _lifecycle: RwLockReadGuard<'a, bool>,
}

impl Deref for DsgManagementClient<'_> {
    type Target = DsgSqlClient<'static>;

    fn deref(&self) -> &Self::Target {
        self.client
    }
}

/// 本Compute首次接受的创建来源，冻结时使用ParsedSpec兼容官方旧GUC格式。
/// operation_uuid只作为原创建来源；当前配置UUID仍用于官方配置追踪，不冻结
/// roles、数据库目录或存储位置，否则合法configure后会错误复用旧认证证明。
pub(crate) struct CreationIdentity {
    operation_uuid: Option<String>,
    tenant_id: String,
    timeline_id: String,
    project_id: Option<String>,
    branch_id: Option<String>,
    endpoint_id: Option<String>,
    preserves_primary_catalog: bool,
}

impl CreationIdentity {
    pub(crate) fn from_parsed(parsed: &ParsedSpec) -> Option<Self> {
        parsed.spec.dynamic_shared_buffers.as_ref().map(|_| Self {
            operation_uuid: parsed.spec.operation_uuid.clone(),
            tenant_id: parsed.tenant_id.to_string(),
            timeline_id: parsed.timeline_id.to_string(),
            project_id: parsed.spec.project_id.clone(),
            branch_id: parsed.spec.branch_id.clone(),
            endpoint_id: parsed.spec.endpoint_id.clone(),
            preserves_primary_catalog: crate::dsg_roles::preserves_primary_catalog(&parsed.spec),
        })
    }
}

// 仅选择本fork已实现的大版本；返回值不授予初始化、准入或normal资格。
// 实际SQL身份仍须精确等于冻结配置，不能以"属于支持范围"接受另一个major。
struct DsgInitializationMajor(i32);

impl DsgInitializationMajor {
    fn select(configured: &str) -> Result<Self> {
        let major = match configured {
            "v17" | "17" => 17,
            "v18" | "18" => 18,
            _ => anyhow::bail!("DSGA initialization requires an implemented PG major"),
        };
        Ok(Self(major))
    }

    fn matches(&self, actual: i32) -> bool {
        self.0 == actual
    }
}

// 初始化失败只记录请求内固定阶段，避免泛化503掩盖首个未确认门。
// 不持有来源/凭据或原始错误，也不改变认证、PG事实、限额与公开响应。
// 该栈守卫最后析构；局部状态/认证锁先释放。取消同样仅记未确认，不重试。
#[derive(Clone, Copy)]
#[repr(u8)]
enum DsgInitializationStage {
    Running = 1,
    Major = 2,
    Creation = 3,
    AuthenticationReceipt = 4,
    NativeBefore = 5,
    PrivateObserver = 6,
    Storage = 7,
    Roles = 8,
    AuthenticationLoaded = 9,
    NativeAfter = 10,
    ConfigurationUnchanged = 11,
    AuthenticationUnchanged = 12,
    Response = 13,
}

struct DsgInitializationDiagnostic {
    stage: DsgInitializationStage,
    confirmed: bool,
}

impl Drop for DsgInitializationDiagnostic {
    fn drop(&mut self) {
        if !self.confirmed {
            warn!(stage = self.stage as u8, "DSGA_INITIALIZATION_UNCONFIRMED");
        }
    }
}

impl ComputeNode {
    /// 只读比较已冻结存储身份，不持锁/不访问网络。原创建缺UUID的旧配置仍可
    /// 沿原启动路径处理，但初始化证明必须拒绝补造来源；后续通知不能填补它。
    pub(crate) fn verify_dsg_storage(&self, parsed: &ParsedSpec) -> Result<()> {
        if parsed.spec.dynamic_shared_buffers.is_some() {
            let original = self
                .dsg_creation
                .get()
                .and_then(Option::as_ref)
                .context("DSGA original creation identity unavailable")?;
            ensure!(
                original.tenant_id == parsed.tenant_id.to_string()
                    && original.timeline_id == parsed.timeline_id.to_string(),
                "DSGA storage identity cannot change within a Compute"
            );
            ensure!(
                original.preserves_primary_catalog
                    == crate::dsg_roles::preserves_primary_catalog(&parsed.spec),
                "DSGA primary catalog mode cannot change within a Compute"
            );
            ensure!(
                original.project_id == parsed.spec.project_id
                    && original.branch_id == parsed.spec.branch_id
                    && original.endpoint_id == parsed.spec.endpoint_id,
                "DSGA project, branch and endpoint cannot change within a Compute"
            );
        }
        Ok(())
    }
    /// 只在Running/原启动契约下核验真实PG、当前角色和已发布认证策略。状态/认证
    /// 锁只复制快照，不跨网络await；末尾再次比对spec、receipt代次和PG boot，
    /// 并发重配、文件漂移、连接断开或内部恢复都不能拼出成功。此GET不打开准入。
    pub(crate) async fn dsg_initialization(&self) -> Result<DsgInitializationResponse> {
        let mut diagnostic = DsgInitializationDiagnostic {
            stage: DsgInitializationStage::Running,
            confirmed: false,
        };
        self.verify_dsg_running()?;
        diagnostic.stage = DsgInitializationStage::Major;
        let pg_major = DsgInitializationMajor::select(&self.params.pgversion)?;
        diagnostic.stage = DsgInitializationStage::Creation;
        let (spec, tenant, timeline) = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("Compute state poisoned"))?;
            let parsed = state.pspec.as_ref().context("Compute spec unavailable")?;
            self.verify_dsg_storage(parsed)?;
            (
                parsed.spec.clone(),
                parsed.tenant_id.to_string(),
                parsed.timeline_id.to_string(),
            )
        };
        let startup = self.dsg_spec()?;
        diagnostic.stage = DsgInitializationStage::AuthenticationReceipt;
        let authentication = self
            .dsg_authentication_receipt
            .read()
            .map_err(|_| anyhow::anyhow!("DSGA authentication state poisoned"))?
            .clone()
            .context("DSGA published authentication proof unavailable")?;
        diagnostic.stage = DsgInitializationStage::NativeBefore;
        let client = self.dsg_management_client(false).await?;
        let before = client.memory_status().await?;
        diagnostic.stage = DsgInitializationStage::PrivateObserver;
        let config = self.get_tokio_conn_conf(Some("compute_ctl:dsga_initialization"));
        let observer = management_connection(&config).await?;
        // PG实际启动的存储身份/大版本也须匹配ParsedSpec，不能只回显配置。
        // ParsedSpec允许官方GUC旧格式，直接读spec可选字段会误拒绝合法原配置。
        diagnostic.stage = DsgInitializationStage::Storage;
        let storage = timeout(Duration::from_secs(5), observer.query_one(
            "SELECT current_setting('server_version_num')::int/10000,current_setting('neon.tenant_id'),current_setting('neon.timeline_id')", &[])).await??;
        ensure!(
            pg_major.matches(storage.get::<_, i32>(0))
                && storage.get::<_, String>(1) == tenant
                && storage.get::<_, String>(2) == timeline,
            "DSGA PG storage identity or major version differs"
        );
        diagnostic.stage = DsgInitializationStage::Roles;
        crate::dsg_roles::attest(&observer, &spec, &self.params.privileged_role_name).await?;
        diagnostic.stage = DsgInitializationStage::AuthenticationLoaded;
        tokio::time::timeout(
            Duration::from_secs(10),
            crate::dsg_hba::verify_loaded(&config, &authentication),
        )
        .await??;
        diagnostic.stage = DsgInitializationStage::NativeAfter;
        let observed = client.memory_status().await?.observed;
        ensure!(
            before.observed.instance == observed.instance,
            "DSGA boot changed during initialization proof"
        );
        diagnostic.stage = DsgInitializationStage::ConfigurationUnchanged;
        self.verify_dsg_running()?;
        {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("Compute state poisoned"))?;
            // 逐字节比较同一类型序列化，覆盖roles/数据库/模式/存储和本次配置operation；
            // 不把公钥/verifier/目录快照写入回执或日志。
            ensure!(
                state.status == ComputeStatus::Running
                    && serde_json::to_vec(
                        &state
                            .pspec
                            .as_ref()
                            .context("Compute spec unavailable")?
                            .spec
                    )? == serde_json::to_vec(&spec)?,
                "DSGA configuration changed during initialization proof"
            );
        }
        diagnostic.stage = DsgInitializationStage::AuthenticationUnchanged;
        {
            let current = self
                .dsg_authentication_receipt
                .read()
                .map_err(|_| anyhow::anyhow!("DSGA authentication state poisoned"))?;
            ensure!(
                current
                    .as_ref()
                    .is_some_and(|now| Arc::ptr_eq(now, &authentication)),
                "DSGA authentication policy changed during initialization proof"
            );
        }
        diagnostic.stage = DsgInitializationStage::Response;
        let response = DsgInitializationResponse {
            protocol_version: 1,
            vm_shrink_permit: false,
            operation_uuid: self
                .dsg_creation
                .get()
                .and_then(Option::as_ref)
                .context("Original creation identity unavailable")?
                .operation_uuid
                .clone()
                .context("Original creation operation unavailable")?,
            tenant_id: tenant,
            timeline_id: timeline,
            startup,
            authentication_policy_verified: true,
            ordinary_roles_verified: true,
            observed,
        };
        response.validate_for(
            &response.startup,
            &response.operation_uuid,
            &response.tenant_id,
            &response.timeline_id,
        )?;
        diagnostic.confirmed = true;
        Ok(response)
    }

    /// state只用于同步拷贝已验证启动契约；绝不能持std Mutex跨数据库await。
    /// Root资源通道不依赖PG配置完成；Init仅允许原冻结身份的签名透传。
    /// 此getter不创建PG管理连接、不打开准入、不产生normal预算或resize许可。
    /// 原dsg_spec和verify_dsg_running继续限制全部PG操作生命周期。
    pub(crate) fn dsg_resource_spec(&self) -> Result<DynamicSharedBuffersSpec> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Compute state poisoned"))?;
        ensure!(
            !matches!(
                state.status,
                ComputeStatus::Empty
                    | ComputeStatus::ConfigurationPending
                    | ComputeStatus::Terminated
            ),
            "Compute has no available Root resource lifecycle"
        );
        let spec = self
            .frozen_dsg_startup()
            .context("DSGA original Root resource identity unavailable")?;
        spec.validate()?;
        Ok(spec)
    }

    pub(crate) fn dsg_spec(&self) -> Result<DynamicSharedBuffersSpec> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Compute state poisoned"))?;
        ensure!(
            !matches!(
                state.status,
                ComputeStatus::Empty
                    | ComputeStatus::Init
                    | ComputeStatus::ConfigurationPending
                    | ComputeStatus::Terminated
            ),
            "Compute has no available management lifecycle"
        );
        // 已发操作的观察/取消在配置、失败和终止准备时仍须可达；PG若已退出则连接
        // 自然报未知/不可用。新resize另在HTTP派发前要求Running，不能把状态切换当取消。
        // 运行中错误configure不能替换已发操作的原身份或隐藏旧结果。观察/取消
        // 始终绑定本Postmaster冻结的启动事实；新resize另核验当前spec未漂移。
        let spec = self
            .frozen_dsg_startup()
            .context("DSGA is not enabled for this Compute")?;
        spec.validate()?;
        Ok(spec)
    }

    pub(crate) async fn dsg_management_client(
        &self,
        observe_during_suspension: bool,
    ) -> Result<DsgManagementClient<'_>> {
        let lifecycle = self.dsg_suspension.read().await;
        ensure!(
            !*lifecycle || observe_during_suspension,
            "DSGA Compute is draining for suspension"
        );
        Ok(DsgManagementClient {
            client: self.dsg_sql_client().await?,
            _lifecycle: lifecycle,
        })
    }

    pub(crate) async fn dsg_suspension_drain(
        &self,
        close: &DsgAdmissionRequest,
        epoch: DsgUint64,
    ) -> Result<DsgPreparedResponse> {
        let mut suspended = self.dsg_suspension.try_write()?;
        let client = self.dsg_client.get().context("DSGA management is not bound")?;
        let fact = timeout(Duration::from_secs(5), client.prepared_state()).await??;
        verify_suspension_gate(&fact, close, epoch)?;
        if !*suspended {
            client.retire_execution_connection()?;
            *suspended = true;
            anyhow::bail!("DSGA suspension started; a fresh native observation is required");
        }
        ensure!(
            self.dsg_activity_monitor_stopped.load(Ordering::Acquire),
            "DSGA activity monitor is still exiting"
        );
        // The Owner polls native counts until PostgreSQL has processed EOF.
        Ok(fact)
    }

    pub(crate) async fn dsg_sql_client(&self) -> Result<&DsgSqlClient<'static>> {
        let spec = self.dsg_spec()?;
        self.dsg_client.get_or_try_init(|| async {
            let mut config = self.get_tokio_conn_conf(Some("compute_ctl:dsga"));
            // 只连接官方系统维护库，覆盖业务数据库/role/options，不能继承租户设置。
            // 这里不走旧zenith_admin的建角色兼容副作用；最新受管Compute应已配置cloud_admin。
            config.dbname("postgres").user("cloud_admin").options(
                "-c role=cloud_admin -c search_path=pg_catalog,pg_temp -c statement_timeout=0 -c pgaudit.log=none"
            );
            let execution = management_connection(&config).await?;
            let observer = management_connection(&config).await?;
            // PG协调器最大相对等待60s；传输等待稍长。到期仍不取消SQL或撤销预算。
            DsgSqlClient::bind_owned(execution, observer, &spec.vm_uid, &spec.compute_instance_id,
                Duration::from_secs(70)).await
        }).await
    }
}

fn verify_suspension_gate(
    fact: &DsgPreparedResponse,
    close: &DsgAdmissionRequest,
    epoch: DsgUint64,
) -> Result<()> {
    close.validate()?;
    fact.validate_for(&close.instance)?;
    let pg = &fact.connections.work.observed;
    ensure!(
        !close.open
            && close.issuer_epoch <= epoch
            && !pg.admission_open
            && pg.admission_revision == close.revision
            && pg.capacity_generation == close.expected_capacity_generation
            && pg.issuer_floor_epoch == epoch
            && fact.connections.private_management_ready
            && fact.connections.public_connections_blocked
            && fact.connections.work.observer_excluded
            && !pg.operation.as_ref().is_some_and(|op| op.phase == DsgPhase::Preparing),
        "DSGA suspension requires the original closed gate under the current issuer"
    );
    Ok(())
}

async fn management_connection(config: &Config) -> Result<Client> {
    let (client, connection) = timeout(Duration::from_secs(5), config.connect(NoTls))
        .await
        .context("DSGA management connection timed out")??;
    // Client销毁会通知driver结束；初始化失败/取消时局部Client也会释放。
    // 已初始化的断连Client保留在OnceCell中，失败关闭，不新建空栅栏绕过未知意图。
    tokio::spawn(async move {
        if connection.await.is_err() {
            warn!("DSGA management connection ended; explicit reconciliation is required");
        }
    });
    Ok(client)
}

// 测试置于生产项之后，保持all-targets静态门禁；移动不改变运行逻辑。
#[cfg(test)]
mod creation_tests {
    use super::*;
    use compute_api::responses::{ComputeConfig, ComputeCtlConfig};

    #[test]
    fn initialization_requires_exact_supported_pg_major() -> Result<()> {
        for (configured, expected) in [("17", 17), ("v17", 17), ("18", 18), ("v18", 18)] {
            let selected = DsgInitializationMajor::select(configured)?;
            assert!(selected.matches(expected));
            for actual in [-1, 0, 14, 16, 17, 18, 19, 170000, 180002] {
                assert_eq!(
                    selected.matches(actual),
                    actual == expected,
                    "supported choice must match actual SQL major exactly"
                );
            }
        }
        for unsupported in [
            "", "14", "16", "19", "v14", "v16", "v19", "v170", "v180", "018", "17\n", "18.2",
            "v17.1", "v18.2", "v17x", "V17", " v17", "v18 ", "+18", "unknown",
        ] {
            assert!(DsgInitializationMajor::select(unsupported).is_err());
        }
        Ok(())
    }

    // 直接消费 compute_ctl 参数生产者的同一枚举编码，避免裸数字夹具掩盖前缀差异。
    #[test]
    fn initialization_accepts_original_pg_major_v_str_producer() -> Result<()> {
        use postgres_versioninfo::PgMajorVersion;

        for (version, expected) in [
            (PgMajorVersion::PG17, 17),
            (PgMajorVersion::PG18, 18),
        ] {
            let configured = version.v_str();
            let selected = DsgInitializationMajor::select(&configured)?;
            assert!(selected.matches(expected));
            assert!(!selected.matches(if expected == 17 { 18 } else { 17 }));
        }
        for version in [PgMajorVersion::PG14, PgMajorVersion::PG16] {
            assert!(DsgInitializationMajor::select(&version.v_str()).is_err());
        }
        Ok(())
    }

    #[test]
    fn frozen_creation_is_distinct_from_current_configuration() -> Result<()> {
        let mut spec = crate::dsg_roles::tests::fixture();
        spec.operation_uuid = Some("original-create".into());
        let node = ComputeNode::new(
            crate::dsg_roles::tests::parameters(),
            ComputeConfig {
                spec: Some(spec.clone()),
                compute_ctl_config: ComputeCtlConfig {
                    jwks: jsonwebtoken::jwk::JwkSet { keys: vec![] },
                    tls: None,
                },
            },
        )?;
        spec.operation_uuid = Some("new-storage-notification".into());
        let mut changed = ParsedSpec::try_from(spec)?;
        node.verify_dsg_storage(&changed)?;
        assert_eq!(
            node.dsg_creation
                .get()
                .unwrap()
                .as_ref()
                .unwrap()
                .operation_uuid
                .as_deref(),
            Some("original-create")
        );
        changed.spec.skip_pg_catalog_updates = true;
        assert!(node.verify_dsg_storage(&changed).is_err());
        changed.spec.skip_pg_catalog_updates = false;
        changed.timeline_id = "ffffffffffffffffffffffffffffffff".parse()?;
        assert!(node.verify_dsg_storage(&changed).is_err());
        Ok(())
    }
    fn resource_node() -> Result<ComputeNode> {
        let spec = crate::dsg_roles::tests::fixture();
        let node = ComputeNode::new(
            crate::dsg_roles::tests::parameters(),
            ComputeConfig {
                spec: Some(spec.clone()),
                compute_ctl_config: ComputeCtlConfig {
                    jwks: jsonwebtoken::jwk::JwkSet { keys: vec![] },
                    tls: None,
                },
            },
        )?;
        assert!(node.frozen_dsg_startup().is_some());
        Ok(node)
    }

    #[test]
    fn root_resource_init_uses_original_frozen_identity_only() -> Result<()> {
        let node = resource_node()?;
        node.state.lock().unwrap().status = ComputeStatus::Init;
        let original = node.frozen_dsg_startup().unwrap();
        assert_eq!(node.dsg_resource_spec()?, original);
        assert!(node.dsg_spec().is_err());
        assert!(node.verify_dsg_running().is_err());
        {
            let mut state = node.state.lock().unwrap();
            let changed = state.pspec.as_mut().unwrap().spec.dynamic_shared_buffers.as_mut().unwrap();
            changed.vm_uid = "changed-vm".into();
            changed.compute_instance_id = "changed-compute".into();
        }
        assert_eq!(node.dsg_resource_spec()?, original);
        assert!(node.dsg_spec().is_err());
        Ok(())
    }

    #[test]
    fn root_resource_rejects_empty_unfrozen_and_disallowed_lifecycles() -> Result<()> {
        let node = resource_node()?;
        for status in [ComputeStatus::Empty, ComputeStatus::ConfigurationPending, ComputeStatus::Terminated] {
            node.state.lock().unwrap().status = status;
            assert!(node.dsg_resource_spec().is_err());
            assert!(node.dsg_spec().is_err());
        }
        let empty = ComputeNode::new(
            crate::dsg_roles::tests::parameters(),
            ComputeConfig {
                spec: None,
                compute_ctl_config: ComputeCtlConfig {
                    jwks: jsonwebtoken::jwk::JwkSet { keys: vec![] },
                    tls: None,
                },
            },
        )?;
        empty.state.lock().unwrap().status = ComputeStatus::Init;
        assert!(empty.dsg_resource_spec().is_err());
        assert!(empty.dsg_spec().is_err());
        node.state.lock().unwrap().status = ComputeStatus::Running;
        assert_eq!(node.dsg_resource_spec()?, node.dsg_spec()?);
        Ok(())
    }

}
