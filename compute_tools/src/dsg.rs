//! Compute运行时的专用管理连接。保留实例与进程内执行栅栏，不进行隐式重连换owner。

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, ensure};
use compute_api::dsg_client::DsgSqlClient;
use compute_api::dsg_initialization::DsgInitializationResponse;
use compute_api::dynamic_shared_buffers::DynamicSharedBuffersSpec;
use compute_api::responses::ComputeStatus;
use tokio::time::timeout;
use tokio_postgres::{Client, Config, NoTls};
use tracing::warn;

use crate::compute::{ComputeNode, ParsedSpec};

/// 本Compute首次接受的创建来源，冻结时使用ParsedSpec兼容官方旧GUC格式。
/// operation_uuid只作为原创建来源；当前配置UUID仍用于官方配置追踪，不冻结
/// roles、数据库目录或存储位置，否则合法configure后会错误复用旧认证证明。
pub(crate) struct CreationIdentity {
    operation_uuid: Option<String>,
    tenant_id: String,
    timeline_id: String,
}

impl CreationIdentity {
    pub(crate) fn from_parsed(parsed: &ParsedSpec) -> Option<Self> {
        parsed.spec.dynamic_shared_buffers.as_ref().map(|_| Self {
            operation_uuid: parsed.spec.operation_uuid.clone(),
            tenant_id: parsed.tenant_id.to_string(),
            timeline_id: parsed.timeline_id.to_string(),
        })
    }
}

#[cfg(test)]
mod creation_tests {
    use super::*;
    use compute_api::responses::{ComputeConfig, ComputeCtlConfig};

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
        changed.timeline_id = "ffffffffffffffffffffffffffffffff".parse()?;
        assert!(node.verify_dsg_storage(&changed).is_err());
        Ok(())
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
        }
        Ok(())
    }
    /// 只在Running/原启动契约下核验真实PG、当前角色和已发布认证策略。状态/认证
    /// 锁只复制快照，不跨网络await；末尾再次比对spec、receipt代次和PG boot，
    /// 并发重配、文件漂移、连接断开或内部恢复都不能拼出成功。此GET不打开准入。
    pub(crate) async fn dsg_initialization(&self) -> Result<DsgInitializationResponse> {
        self.verify_dsg_running()?;
        ensure!(
            self.params.pgversion == "17",
            "DSGA initialization currently requires PG17"
        );
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
        let authentication = self
            .dsg_authentication_receipt
            .read()
            .map_err(|_| anyhow::anyhow!("DSGA authentication state poisoned"))?
            .clone()
            .context("DSGA published authentication proof unavailable")?;
        let client = self.dsg_management_client().await?;
        let before = client.memory_status().await?;
        let config = self.get_tokio_conn_conf(Some("compute_ctl:dsga_initialization"));
        let observer = management_connection(&config).await?;
        // PG实际启动的存储身份/大版本也须匹配ParsedSpec，不能只回显配置。
        // ParsedSpec允许官方GUC旧格式，直接读spec可选字段会误拒绝合法原配置。
        let storage = timeout(Duration::from_secs(5), observer.query_one(
            "SELECT current_setting('server_version_num')::int/10000,current_setting('neon.tenant_id'),current_setting('neon.timeline_id')", &[])).await??;
        ensure!(
            storage.get::<_, i32>(0) == 17
                && storage.get::<_, String>(1) == tenant
                && storage.get::<_, String>(2) == timeline,
            "DSGA PG storage identity or major version differs"
        );
        crate::dsg_roles::attest(&observer, &spec, &self.params.privileged_role_name).await?;
        tokio::time::timeout(
            Duration::from_secs(10),
            crate::dsg_hba::verify_loaded(&config, &authentication),
        )
        .await??;
        let observed = client.memory_status().await?.observed;
        ensure!(
            before.observed.instance == observed.instance,
            "DSGA boot changed during initialization proof"
        );
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
        Ok(response)
    }

    /// state只用于同步拷贝已验证启动契约；绝不能持std Mutex跨数据库await。
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

    pub(crate) async fn dsg_management_client(&self) -> Result<&DsgSqlClient<'static>> {
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
