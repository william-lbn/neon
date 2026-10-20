//! 管理SQL的真实双连接客户端。
//!
//! 执行连接可能长时间阻塞在排水/Provider中，状态与业务取消必须走独立连接。
//! SDK不发放VM许可、不自动重试改ID、不把超时/ERROR/丢弃future当成取消成功。
//! 进程内栅栏不能替代外层持久意图、issuer租约和VM最后执行fencing。

use std::fmt;
use std::ops::Deref;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::de::DeserializeOwned;
use tokio::time::timeout;
use tokio_postgres::Client;

use crate::dsg_control::{
    DsgInstanceIdentity, DsgOperationKey, DsgOperationQuery, DsgResizeRequest, validate_id,
};
use crate::dsg_state::{
    DsgCapabilities, DsgControlResponse, DsgDisposition, DsgMemoryResponse, DsgPhase,
};

const MAX_RESPONSE_BYTES: usize = 32 * 1024;

/// 本地未发送和已发送后结果不确定必须可机器区分；后者需要按原意图对账。
#[derive(Debug)]
pub enum DsgCallError {
    NotSent(anyhow::Error),
    Unconfirmed {
        key: DsgOperationKey,
        source: anyhow::Error,
    },
}

impl fmt::Display for DsgCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSent(error) => write!(f, "DSGA request was not sent: {error}"),
            Self::Unconfirmed { source, .. } => write!(
                f,
                "DSGA result is unconfirmed; reconcile the original immutable intent: {source}"
            ),
        }
    }
}

impl std::error::Error for DsgCallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::NotSent(error) => error.as_ref(),
            Self::Unconfirmed { source, .. } => source.as_ref(),
        })
    }
}

#[derive(Default)]
struct ExecutionState {
    pending: Option<DsgResizeRequest>,
    in_flight: bool,
}

/// 不在await期间持Mutex。future取消/超时/异常默认留下原意图，下一新意图不能穿越。
struct InFlight<'a> {
    state: &'a Mutex<ExecutionState>,
    resolved: bool,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        // 锁若因panic中毒，后续调用全部拒绝，不在Drop中二次panic或擅自恢复状态。
        if let Ok(mut state) = self.state.lock() {
            state.in_flight = false;
            if self.resolved {
                state.pending = None;
            }
        }
    }
}

/// 两个专用、已认证连接可借用或交由SDK拥有；调用方驱动driver，不记录连接串/密码。
/// bind会固定这两个管理连接的search_path=pg_catalog，再核验扩展函数成员身份。
pub struct DsgSqlClient<'a> {
    execution: ManagementConnection<'a>,
    observer: ManagementConnection<'a>,
    execution_schema: String,
    observer_schema: String,
    instance: DsgInstanceIdentity,
    capabilities: DsgCapabilities,
    response_timeout: Duration,
    execution_state: Mutex<ExecutionState>,
}

// HTTP运行时须跨请求持有同一个客户端/执行栅栏。Owned避免自引用结构，也不把
// 每个请求的新连接误当作无在途操作；Borrowed保留嵌入式调用和原生测试接口。
enum ManagementConnection<'a> {
    Borrowed(&'a Client),
    Owned(Client),
}

impl Deref for ManagementConnection<'_> {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(client) => client,
            Self::Owned(client) => client,
        }
    }
}

impl DsgSqlClient<'static> {
    /// 所有权交给长期运行的服务；连接driver仍由创建连接的调用方驱动。
    pub async fn bind_owned(
        execution: Client,
        observer: Client,
        vm_uid: &str,
        compute_instance_id: &str,
        response_timeout: Duration,
    ) -> Result<Self> {
        Self::bind_connections(
            ManagementConnection::Owned(execution),
            ManagementConnection::Owned(observer),
            vm_uid,
            compute_instance_id,
            response_timeout,
        )
        .await
    }
}

impl<'a> DsgSqlClient<'a> {
    pub async fn bind(
        execution: &'a Client,
        observer: &'a Client,
        vm_uid: &str,
        compute_instance_id: &str,
        response_timeout: Duration,
    ) -> Result<Self> {
        Self::bind_connections(
            ManagementConnection::Borrowed(execution),
            ManagementConnection::Borrowed(observer),
            vm_uid,
            compute_instance_id,
            response_timeout,
        )
        .await
    }

    async fn bind_connections(
        execution: ManagementConnection<'a>,
        observer: ManagementConnection<'a>,
        vm_uid: &str,
        compute_instance_id: &str,
        response_timeout: Duration,
    ) -> Result<Self> {
        ensure!(
            !response_timeout.is_zero(),
            "DSGA response timeout must be positive"
        );
        validate_id(vm_uid, "VM")?;
        validate_id(compute_instance_id, "Compute")?;
        // PID来自实际两个会话，不能只比较Arc/Client包装对象的地址。
        let (execution_schema, execution_pid) = discover(&execution, response_timeout).await?;
        let (observer_schema, observer_pid) = discover(&observer, response_timeout).await?;
        ensure!(
            execution_pid != observer_pid,
            "DSGA execution and observation require independent sessions"
        );
        let execution_cap: DsgCapabilities = read_json(
            &execution,
            &sql(&execution_schema, "neon_dsg_capabilities", false),
            None,
            response_timeout,
        )
        .await?;
        let observer_cap: DsgCapabilities = read_json(
            &observer,
            &sql(&observer_schema, "neon_dsg_capabilities", false),
            None,
            response_timeout,
        )
        .await?;
        let instance = execution_cap.bind_instance(vm_uid, compute_instance_id)?;
        ensure!(
            instance == observer_cap.bind_instance(vm_uid, compute_instance_id)?,
            "DSGA management sessions target different instances or boots"
        );
        ensure!(
            execution_cap == observer_cap,
            "DSGA management sessions expose inconsistent capabilities"
        );
        Ok(Self {
            execution,
            observer,
            execution_schema,
            observer_schema,
            instance,
            capabilities: execution_cap,
            response_timeout,
            execution_state: Mutex::new(ExecutionState::default()),
        })
    }

    pub fn instance(&self) -> &DsgInstanceIdentity {
        &self.instance
    }

    /// 原样保留协商信息；绑定成功仅支持逻辑操作，不意味着production_ready。
    pub fn capabilities(&self) -> &DsgCapabilities {
        &self.capabilities
    }

    /// 网络/SQL错误、验证失败或future被丢弃后，只允许原意图的完整幂等重放。
    /// 查询终态不清掉本地栅栏：原执行连接可能仍停在COMMITTED后的Provider中。
    /// 完整重放经过执行连接的协议队列，才能证明本客户端前次SQL已结束及意图未变。
    /// 新客户端不能据此证明其他连接的Provider/VM动作已结束，仍须查独立回执。
    pub async fn request_resize(
        &self,
        request: &DsgResizeRequest,
    ) -> Result<DsgControlResponse, DsgCallError> {
        request.validate().map_err(DsgCallError::NotSent)?;
        let payload =
            serde_json::to_string(request).map_err(|error| DsgCallError::NotSent(error.into()))?;
        let mut flight = self.reserve(request).map_err(DsgCallError::NotSent)?;
        let result = self
            .call(
                &self.execution,
                &self.execution_schema,
                "neon_dsg_request_resize",
                &payload,
            )
            .await;
        let response: DsgControlResponse =
            result.map_err(|source| unconfirmed(&request.key, source))?;
        response
            .validate_resize(request, &self.instance)
            .map_err(|source| unconfirmed(&request.key, source))?;
        ensure_action(&response, Action::Resize)
            .map_err(|source| unconfirmed(&request.key, source))?;
        // BUSY/REJECTED已确证未建立本请求；STALE/CONFLICT可能涉及淘汰历史，
        // 不能清栅栏后换ID规避。PREPARING重放也仍须等待原操作终态。
        flight.resolved = matches!(
            response.receipt.disposition,
            DsgDisposition::Busy | DsgDisposition::Rejected
        ) || response
            .receipt
            .result
            .as_ref()
            .and_then(|s| s.operation.as_ref())
            .is_some_and(|operation| operation.phase != DsgPhase::Preparing);
        Ok(response)
    }

    pub async fn resize_status(
        &self,
        query: &DsgOperationQuery,
    ) -> Result<DsgControlResponse, DsgCallError> {
        self.query(query, Action::Status).await
    }

    pub async fn cancel_resize(
        &self,
        query: &DsgOperationQuery,
    ) -> Result<DsgControlResponse, DsgCallError> {
        self.query(query, Action::Cancel).await
    }

    pub async fn memory_status(&self) -> Result<DsgMemoryResponse> {
        let response: DsgMemoryResponse = read_json(
            &self.observer,
            &sql(&self.observer_schema, "neon_dsg_memory_status", false),
            None,
            self.response_timeout,
        )
        .await?;
        response.validate_for(&self.instance)?;
        Ok(response)
    }

    fn reserve(&self, request: &DsgResizeRequest) -> Result<InFlight<'_>> {
        let mut state = self
            .execution_state
            .lock()
            .map_err(|_| anyhow::anyhow!("DSGA execution fence is poisoned"))?;
        ensure!(
            !state.in_flight,
            "another DSGA execution call is in flight; use the observation channel"
        );
        if let Some(pending) = &state.pending {
            ensure!(
                pending == request,
                "unconfirmed DSGA intent must be reconciled unchanged before a new intent"
            );
        }
        state.pending = Some(request.clone());
        state.in_flight = true;
        Ok(InFlight {
            state: &self.execution_state,
            resolved: false,
        })
    }

    async fn query(
        &self,
        query: &DsgOperationQuery,
        action: Action,
    ) -> Result<DsgControlResponse, DsgCallError> {
        query.validate().map_err(DsgCallError::NotSent)?;
        let payload =
            serde_json::to_string(query).map_err(|error| DsgCallError::NotSent(error.into()))?;
        let function = match action {
            Action::Status => "neon_dsg_resize_status",
            Action::Cancel => "neon_dsg_cancel_resize",
            Action::Resize => unreachable!("resize has its own execution entry point"),
        };
        let response: DsgControlResponse = self
            .call(&self.observer, &self.observer_schema, function, &payload)
            .await
            .map_err(|source| unconfirmed(&query.key, source))?;
        response
            .validate_for(&query.key, &self.instance)
            .map_err(|source| unconfirmed(&query.key, source))?;
        ensure_action(&response, action).map_err(|source| unconfirmed(&query.key, source))?;
        Ok(response)
    }

    async fn call<T: DeserializeOwned>(
        &self,
        client: &Client,
        schema: &str,
        function: &str,
        payload: &str,
    ) -> Result<T> {
        read_json(
            client,
            &sql(schema, function, true),
            Some(payload),
            self.response_timeout,
        )
        .await
    }
}

#[derive(Clone, Copy)]
enum Action {
    Resize,
    Status,
    Cancel,
}

fn unconfirmed(key: &DsgOperationKey, source: anyhow::Error) -> DsgCallError {
    DsgCallError::Unconfirmed {
        key: key.clone(),
        source,
    }
}

fn ensure_action(response: &DsgControlResponse, action: Action) -> Result<()> {
    use DsgDisposition::*;
    let allowed = match action {
        Action::Resize => matches!(
            response.receipt.disposition,
            Accepted | Replayed | Busy | Stale | Conflict | Rejected
        ),
        Action::Status => matches!(
            response.receipt.disposition,
            Replayed | Stale | Conflict | ResultExpired | NotFound
        ),
        Action::Cancel => matches!(
            response.receipt.disposition,
            CancelRequested | TooLate | Stale | Conflict | ResultExpired | NotFound
        ),
    };
    ensure!(
        allowed,
        "DSGA disposition is invalid for the requested action"
    );
    Ok(())
}

fn sql(schema: &str, function: &str, argument: bool) -> String {
    // schema已经按实际命名空间逐个双引号转义；函数名仅为代码中的固定常量。
    let argument = if argument { "$1::pg_catalog.text" } else { "" };
    format!("SELECT {schema}.{function}({argument})::pg_catalog.text")
}

async fn read_json<T: DeserializeOwned>(
    client: &Client,
    sql: &str,
    payload: Option<&str>,
    wait: Duration,
) -> Result<T> {
    let row = timeout(wait, async {
        if let Some(payload) = payload {
            client.query_one(sql, &[&payload]).await
        } else {
            client.query_one(sql, &[]).await
        }
    })
    .await
    .context("DSGA SQL response deadline elapsed")??;
    let body: &str = row.try_get(0)?;
    ensure!(
        body.len() <= MAX_RESPONSE_BYTES,
        "DSGA response exceeds 32 KiB"
    );
    serde_json::from_str(body).context("invalid DSGA response schema")
}

async fn discover(client: &Client, wait: Duration) -> Result<(String, i32)> {
    timeout(wait, async {
        // 防御显式把pg_catalog放末尾的恶意search_path，目录比较操作符也只能用系统版本。
        // 这是调用方交给SDK的专用管理连接，不改变普通业务会话的配置。
        client.batch_execute("SET search_path = pg_catalog, pg_temp").await?;
        // trusted扩展可由普通用户安装。若扩展由租户所有，或任一非超级角色能
        // CREATE于其schema，首次查验后就可能发生drop/同名替换。专用管理库必须
        // 由管理员维护；继承CREATE与SET ROLE到超级角色均须检查。PG17成员边的
        // INHERIT/SET可分别控制，NOINHERIT不能证明不能切换管理员身份替换对象。
        let row = client.query_one("SELECT n.nspname::pg_catalog.text, pg_catalog.pg_backend_pid(), \
            owner.rolsuper AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles r \
                WHERE NOT r.rolsuper AND (EXISTS (SELECT 1 FROM pg_catalog.pg_roles elevated \
                    WHERE elevated.rolsuper AND pg_catalog.pg_has_role(r.oid,elevated.oid,'SET')) \
                OR (NOT (r.rolname='pg_database_owner' AND \
                    (SELECT dbowner.rolsuper FROM pg_catalog.pg_database db \
                     JOIN pg_catalog.pg_roles dbowner ON dbowner.oid=db.datdba \
                     WHERE db.datname=pg_catalog.current_database())) \
                AND pg_catalog.has_schema_privilege(r.oid,n.oid,'CREATE')))) AS trusted \
            FROM pg_catalog.pg_extension e JOIN pg_catalog.pg_namespace n ON n.oid=e.extnamespace \
            JOIN pg_catalog.pg_roles owner ON owner.oid=e.extowner WHERE e.extname='neon'", &[]).await?;
        let schema: String = row.try_get(0)?;
        let pid: i32 = row.try_get(1)?;
        ensure!(row.try_get::<_, bool>(2)?, "DSGA requires a trusted management extension and schema");
        let rows = client.query("SELECT p.proname::pg_catalog.text FROM pg_catalog.pg_extension e \
            JOIN pg_catalog.pg_depend d ON d.refobjid=e.oid AND d.refclassid='pg_catalog.pg_extension'::pg_catalog.regclass \
                AND d.classid='pg_catalog.pg_proc'::pg_catalog.regclass AND d.deptype='e' \
            JOIN pg_catalog.pg_proc p ON p.oid=d.objid AND p.pronamespace=e.extnamespace \
            JOIN pg_catalog.pg_language l ON l.oid=p.prolang \
            WHERE e.extname='neon' AND l.lanname='c' AND p.probin='$libdir/neon' \
                AND p.prosrc=p.proname AND p.prorettype='pg_catalog.jsonb'::pg_catalog.regtype \
                AND ((p.proname IN ('neon_dsg_capabilities','neon_dsg_memory_status') AND p.pronargs=0) \
                  OR (p.proname IN ('neon_dsg_request_resize','neon_dsg_resize_status','neon_dsg_cancel_resize') \
                      AND p.pronargs=1 AND p.proargtypes[0]='pg_catalog.text'::pg_catalog.regtype))", &[]).await?;
        // 五个精确签名均须属于真正Neon扩展，不能只看到同名租户SQL函数就启用。
        ensure!(rows.len() == 5, "Neon DSGA management extension functions are missing or incompatible");
        Ok((format!("\"{}\"", schema.replace('"', "\"\"")), pid))
    }).await.context("DSGA capability discovery deadline elapsed")?
}
