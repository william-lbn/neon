//! Compute运行时的专用管理连接。保留实例与进程内执行栅栏，不进行隐式重连换owner。

use std::time::Duration;

use anyhow::{Context, Result, ensure};
use compute_api::dsg_client::DsgSqlClient;
use compute_api::dynamic_shared_buffers::DynamicSharedBuffersSpec;
use compute_api::responses::ComputeStatus;
use tokio::time::timeout;
use tokio_postgres::{Client, Config, NoTls};
use tracing::warn;

use crate::compute::ComputeNode;

impl ComputeNode {
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
