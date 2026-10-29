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

use crate::dsg_admission::{
    DsgAdmissionClosure, DsgAdmissionContinuationRequest, DsgAdmissionContinuationResponse,
    DsgAdmissionRequest, DsgAdmissionResponse,
};

use crate::dsg_control::{
    DsgInstanceIdentity, DsgIssuerFenceRequest, DsgOperationKey, DsgOperationQuery,
    DsgResizeRequest, validate_id,
};
use crate::dsg_state::{
    DsgCapabilities, DsgControlResponse, DsgDisposition, DsgIssuerFenceResponse, DsgMemoryResponse,
    DsgPhase, DsgSharedCostResponse,
};

use crate::dsg_connection::DsgConnectionResponse;
use crate::dsg_dsm::DsgDsmResponse;
use crate::dsg_prepared::DsgPreparedResponse;
use crate::dsg_work::DsgWorkResponse;

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

/// 接管没有resize Key，不能伪造操作ID来表达不确定性。失败保留完整接管意图，
/// 请求方须按同实例/代次重试对账；不得把超时当作floor提升未发生。
#[derive(Debug)]
pub enum DsgFenceCallError {
    NotSent(anyhow::Error),
    Unconfirmed {
        request: DsgIssuerFenceRequest,
        source: anyhow::Error,
    },
}

impl fmt::Display for DsgFenceCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSent(source) => write!(f, "DSGA issuer fence was not sent: {source}"),
            Self::Unconfirmed { source, .. } => {
                write!(f, "DSGA issuer fence is unconfirmed: {source}")
            }
        }
    }
}
impl std::error::Error for DsgFenceCallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::NotSent(source) | Self::Unconfirmed { source, .. } => source.as_ref(),
        })
    }
}

#[derive(Default)]
struct ExecutionState {
    pending: Option<DsgResizeRequest>,
    in_flight: bool,
    admission_pending: Option<DsgAdmissionRequest>,
    admission_close: Option<DsgAdmissionRequest>,
    admission_continuation: Option<DsgAdmissionContinuationRequest>,
    admission_in_flight: bool,
}

/// 与resize分开保留原准入/关闭意图；错误不回显SQL、凭据或原始响应。
#[derive(Debug)]
pub enum DsgAdmissionCallError {
    NotSent(anyhow::Error),
    Unconfirmed {
        request: DsgAdmissionRequest,
        source: anyhow::Error,
    },
}

impl fmt::Display for DsgAdmissionCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotSent(_) => "DSGA admission request was not sent",
            Self::Unconfirmed { .. } => "DSGA original admission result remains unconfirmed",
        })
    }
}
impl std::error::Error for DsgAdmissionCallError {}

struct AdmissionFlight<'a> {
    state: &'a Mutex<ExecutionState>,
    resolved: bool,
    predecessor: Option<RetainedAdmission>,
    fenced_out: bool,
}

// 仅接续派发保存前一完整本地状态。原生fence明确返回Stale时，后继setter
// 尚未发送，且该旧epoch已被封住；只恢复此前状态，绝不清除此前已有未知。
struct RetainedAdmission {
    original: Option<DsgAdmissionRequest>,
    close: Option<DsgAdmissionRequest>,
    continuation: Option<DsgAdmissionContinuationRequest>,
}
impl Drop for AdmissionFlight<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.admission_in_flight = false;
            if self.fenced_out {
                if let Some(previous) = self.predecessor.take() {
                    state.admission_pending = previous.original;
                    state.admission_close = previous.close;
                    state.admission_continuation = previous.continuation;
                }
            } else if self.resolved {
                state.admission_pending = None;
                state.admission_close = None;
                state.admission_continuation = None;
            }
        }
    }
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

    /// 原生成本只读观察，原启动总量已含四数组。完整成本来自同一次native快照，
    /// 不用两次memory/SHOW拼接，也不因一次成功观察解除任何未知请求栅栏。
    pub async fn shared_cost(&self) -> Result<DsgSharedCostResponse> {
        self.formal_function("neon_dsg_shared_cost", 0).await?;
        let response: DsgSharedCostResponse = read_json(
            &self.observer,
            &sql(&self.observer_schema, "neon_dsg_shared_cost", false),
            None,
            self.response_timeout,
        )
        .await?;
        response.validate_for(&self.instance)?;
        Ok(response)
    }

    /// 真实前台登记的独立只读观察；不清pending、不生成Quiesce或预算回执。
    /// 正式扩展成员检查与API版本独立于PG大版本，旧内核/旧SQL明确拒绝该能力。
    pub async fn work_state(&self) -> Result<DsgWorkResponse> {
        self.formal_function("neon_dsg_work_state", 0).await?;
        // 固定无参数的唯一观察语句用完整Simple Query。扩展协议在Parse/Bind后
        // 合法保留隐式事务，Native必须保守计数，不能为了观察成功把该事务忽略。
        // 本SDK专用观察连接只发送已转义schema+固定函数，不拼接任何业务参数。
        let response: DsgWorkResponse = read_json_simple(
            &self.observer,
            &sql(&self.observer_schema, "neon_dsg_work_state", false),
            self.response_timeout,
        )
        .await?;
        response.validate_for(&self.instance)?;
        Ok(response)
    }

    /// 固定只读查询消费Postmaster ack与同点Work2；不清未知原意图，不发完整排空证书。
    /// 使用完整Simple Query，避免观察自身Parse/Bind留下隐式事务；旧SQL成员明确拒绝。
    pub async fn connection_state(&self) -> Result<DsgConnectionResponse> {
        self.formal_function("neon_dsg_connection_state", 0).await?;
        let response: DsgConnectionResponse = read_json_simple(
            &self.observer,
            &sql(&self.observer_schema, "neon_dsg_connection_state", false),
            self.response_timeout,
        )
        .await?;
        response.validate_for(&self.instance)?;
        Ok(response)
    }

    /// Prepared1读取同点原生槽与连接/Work，不清Pending，不从前台idle推断2PC为空。
    /// 缺正式SQL成员或旧内核明确拒绝；完整Simple Query不留下观察者隐式事务。
    pub async fn prepared_state(&self) -> Result<DsgPreparedResponse> {
        self.formal_function("neon_dsg_prepared_state", 0).await?;
        let response: DsgPreparedResponse = read_json_simple(
            &self.observer,
            &sql(&self.observer_schema, "neon_dsg_prepared_state", false),
            self.response_timeout,
        )
        .await?;
        response.validate_for(&self.instance)?;
        Ok(response)
    }

    /// DSM1只读同点payload预留，不推断物理释放、不清Pending或代替root跨boot结案。
    /// 正式零参数SQL成员与完整Simple Query保证身份和观察者排除语义不变。
    pub async fn dsm_state(&self) -> Result<DsgDsmResponse> {
        self.formal_function("neon_dsg_dsm_state", 0).await?;
        let response: DsgDsmResponse = read_json_simple(
            &self.observer,
            &sql(&self.observer_schema, "neon_dsg_dsm_state", false),
            self.response_timeout,
        )
        .await?;
        response.validate_for(&self.instance)?;
        Ok(response)
    }

    /// 独立观察连接推进/确认floor，执行连接阻塞时仍可到达PG。成功FENCED也只
    /// 证明PG旧执行者退出，不清本客户端pending，更不清外层原意图/预算。
    pub async fn fence_issuer(
        &self,
        request: &DsgIssuerFenceRequest,
    ) -> Result<DsgIssuerFenceResponse, DsgFenceCallError> {
        request.validate().map_err(DsgFenceCallError::NotSent)?;
        if request.instance != self.instance {
            return Err(DsgFenceCallError::NotSent(anyhow::anyhow!(
                "issuer fence targets another bound instance"
            )));
        }
        let payload =
            serde_json::to_string(request).map_err(|e| DsgFenceCallError::NotSent(e.into()))?;
        let response: DsgIssuerFenceResponse = self
            .call(
                &self.observer,
                &self.observer_schema,
                "neon_dsg_fence_issuer",
                &payload,
            )
            .await
            .map_err(|source| DsgFenceCallError::Unconfirmed {
                request: request.clone(),
                source,
            })?;
        response
            .validate_for(request, &self.instance)
            .map_err(|source| DsgFenceCallError::Unconfirmed {
                request: request.clone(),
                source,
            })?;
        Ok(response)
    }

    /// 完整原准入与可选更高关闭意图的值副本；调用方必须另有持久journal。
    pub fn pending_admission(
        &self,
    ) -> Result<(Option<DsgAdmissionRequest>, Option<DsgAdmissionRequest>)> {
        let state = self
            .execution_state
            .lock()
            .map_err(|_| anyhow::anyhow!("DSGA execution fence is poisoned"))?;
        Ok((
            state.admission_pending.clone(),
            state.admission_close.clone(),
        ))
    }

    /// 只供外层从已认证持久journal恢复；不从一次PG查询推导或生成新意图。
    /// 不能覆盖另一原意图/在途调用，保留并行resize未知栅栏；恢复不产生网络写。
    pub fn restore_admission_pending(
        &self,
        original: &DsgAdmissionRequest,
        close: Option<&DsgAdmissionRequest>,
    ) -> Result<()> {
        self.check_admission_binding(original, false)?;
        if let Some(close) = close {
            self.check_admission_binding(close, false)?;
            close.closes_over(original)?;
        }
        let mut state = self
            .execution_state
            .lock()
            .map_err(|_| anyhow::anyhow!("DSGA execution fence is poisoned"))?;
        ensure!(
            !state.in_flight && !state.admission_in_flight,
            "cannot restore over an in-flight call"
        );
        ensure!(
            state.admission_continuation.is_none(),
            "continuation requires complete restore"
        );
        ensure!(
            state.admission_pending.is_none() || state.admission_pending.as_ref() == Some(original),
            "restore differs from retained admission"
        );
        ensure!(
            state.admission_close.is_none() || state.admission_close.as_ref() == close,
            "restore differs from retained closure"
        );
        state.admission_pending = Some(original.clone());
        state.admission_close = close.cloned();
        Ok(())
    }

    pub async fn set_admission(
        &self,
        request: &DsgAdmissionRequest,
    ) -> Result<DsgAdmissionResponse, DsgAdmissionCallError> {
        self.check_admission_binding(request, false)
            .map_err(DsgAdmissionCallError::NotSent)?;
        self.admission_function("neon_dsg_set_admission")
            .await
            .map_err(DsgAdmissionCallError::NotSent)?;
        let payload =
            serde_json::to_string(request).map_err(|e| DsgAdmissionCallError::NotSent(e.into()))?;
        let mut guard = self
            .reserve_admission(request, None)
            .map_err(DsgAdmissionCallError::NotSent)?;
        // 独立观察连接允许close在resize/Provider阻塞时到达；open在本地与内核
        // 两层拒绝穿过未知resize。未来取消/超时Drop只退出in_flight，不清原意图。
        let response: DsgAdmissionResponse = self
            .call(
                &self.observer,
                &self.observer_schema,
                "neon_dsg_set_admission",
                &payload,
            )
            .await
            .map_err(|source| admission_unconfirmed(request, source))?;
        response
            .validate_for(request, &self.instance, false)
            .map_err(|source| admission_unconfirmed(request, source))?;
        guard.resolved = response.confirms();
        Ok(response)
    }

    /// 只读查询不重发setter；未见/淘汰/普通memory/fence都不清未知栅栏。仅完整
    /// 原回执能确认该门禁写入，且不与正在进行的本地请求/关闭协调争用清理权。
    pub async fn admission_status(
        &self,
        request: &DsgAdmissionRequest,
    ) -> Result<DsgAdmissionResponse, DsgAdmissionCallError> {
        self.check_admission_binding(request, true)
            .map_err(DsgAdmissionCallError::NotSent)?;
        self.admission_function("neon_dsg_admission_status")
            .await
            .map_err(DsgAdmissionCallError::NotSent)?;
        let payload =
            serde_json::to_string(request).map_err(|e| DsgAdmissionCallError::NotSent(e.into()))?;
        let response: DsgAdmissionResponse = self
            .call(
                &self.observer,
                &self.observer_schema,
                "neon_dsg_admission_status",
                &payload,
            )
            .await
            .map_err(|source| admission_unconfirmed(request, source))?;
        response
            .validate_for(request, &self.instance, true)
            .map_err(|source| admission_unconfirmed(request, source))?;
        if response.confirms() {
            let mut state = self.execution_state.lock().map_err(|_| {
                DsgAdmissionCallError::NotSent(anyhow::anyhow!("DSGA execution fence is poisoned"))
            })?;
            if !state.admission_in_flight
                && state.admission_close.is_none()
                && state.admission_pending.as_ref() == Some(request)
            {
                state.admission_pending = None;
            }
        }
        Ok(response)
    }

    /// 显式受控关闭：原意图来自外层持久账本（允许恢复本进程尚未知的原请求），
    /// 若已有本地未知则必须逐字匹配。close成功才解除本地未来门禁写栅栏；返回
    /// prior_may_have_applied恒true，外层仍须实际排空并保留旧预算，不能伪造NoEffect。
    pub async fn close_admission_over_original(
        &self,
        original: &DsgAdmissionRequest,
        close: &DsgAdmissionRequest,
    ) -> Result<DsgAdmissionClosure, DsgAdmissionCallError> {
        self.check_admission_binding(original, false)
            .map_err(DsgAdmissionCallError::NotSent)?;
        self.check_admission_binding(close, false)
            .map_err(DsgAdmissionCallError::NotSent)?;
        close
            .closes_over(original)
            .map_err(DsgAdmissionCallError::NotSent)?;
        self.admission_function("neon_dsg_set_admission")
            .await
            .map_err(DsgAdmissionCallError::NotSent)?;
        let payload =
            serde_json::to_string(close).map_err(|e| DsgAdmissionCallError::NotSent(e.into()))?;
        let mut guard = self
            .reserve_admission(original, Some(close))
            .map_err(DsgAdmissionCallError::NotSent)?;
        let response: DsgAdmissionResponse = self
            .call(
                &self.observer,
                &self.observer_schema,
                "neon_dsg_set_admission",
                &payload,
            )
            .await
            .map_err(|source| admission_unconfirmed(close, source))?;
        response
            .validate_for(close, &self.instance, false)
            .map_err(|source| admission_unconfirmed(close, source))?;
        if !response.confirms() {
            return Err(admission_unconfirmed(
                close,
                anyhow::anyhow!("original admission still needs controlled closure"),
            ));
        }
        guard.resolved = true;
        Ok(DsgAdmissionClosure {
            protocol_version: 1,
            vm_shrink_permit: false,
            original: original.clone(),
            closed_by: response,
            prior_may_have_applied: true,
        })
    }

    /// 关闭回复丢失/换owner后，只读核验同一更高完整关闭回执；不得把Memory、
    /// 不存在、淘汰或floor提升本身当关闭成功。解除的是本地未来写栅栏，旧工作
    /// 可能已开始这一事实恒保留；实际排空/预算撤销仍由持久协调器决定。
    pub async fn admission_closure_status(
        &self,
        original: &DsgAdmissionRequest,
        close: &DsgAdmissionRequest,
    ) -> Result<DsgAdmissionClosure, DsgAdmissionCallError> {
        self.check_admission_binding(original, false)
            .map_err(DsgAdmissionCallError::NotSent)?;
        close
            .closes_over(original)
            .map_err(DsgAdmissionCallError::NotSent)?;
        let response = self.admission_status(close).await?;
        if !response.confirms() {
            return Err(admission_unconfirmed(
                close,
                anyhow::anyhow!("complete closing receipt remains unavailable"),
            ));
        }
        let mut state = self.execution_state.lock().map_err(|_| {
            DsgAdmissionCallError::NotSent(anyhow::anyhow!("DSGA execution fence is poisoned"))
        })?;
        if state.admission_continuation.is_some()
            || state.admission_in_flight
            || state
                .admission_pending
                .as_ref()
                .is_some_and(|value| value != original)
            || state
                .admission_close
                .as_ref()
                .is_some_and(|value| value != close)
        {
            return Err(DsgAdmissionCallError::NotSent(anyhow::anyhow!(
                "local admission differs from verified closure"
            )));
        }
        state.admission_pending = None;
        state.admission_close = None;
        Ok(DsgAdmissionClosure {
            protocol_version: 1,
            vm_shrink_permit: false,
            original: original.clone(),
            closed_by: response,
            prior_may_have_applied: true,
        })
    }

    fn check_admission_binding(&self, request: &DsgAdmissionRequest, query: bool) -> Result<()> {
        request.validate()?;
        ensure!(
            request.instance.vm_uid == self.instance.vm_uid
                && request.instance.compute_instance_id == self.instance.compute_instance_id
                && (query || request.instance == self.instance),
            "admission targets another bound instance"
        );
        Ok(())
    }

    /// 返回完整接续选择的副本，不提供清空入口。重连/进程恢复必须使用外层
    /// 写前WAL的同一值，不能只按最新revision重建而丢掉最初未知和直接前驱。
    pub fn pending_admission_continuation(
        &self,
    ) -> Result<Option<DsgAdmissionContinuationRequest>> {
        Ok(self
            .execution_state
            .lock()
            .map_err(|_| anyhow::anyhow!("DSGA execution fence is poisoned"))?
            .admission_continuation
            .clone())
    }

    pub fn restore_admission_continuation(
        &self,
        request: &DsgAdmissionContinuationRequest,
    ) -> Result<()> {
        request.validate()?;
        self.check_admission_binding(&request.close, false)?;
        let mut state = self
            .execution_state
            .lock()
            .map_err(|_| anyhow::anyhow!("DSGA execution fence is poisoned"))?;
        ensure!(
            !state.in_flight && !state.admission_in_flight,
            "cannot restore an in-flight call"
        );
        ensure!(
            state
                .admission_pending
                .as_ref()
                .is_none_or(|p| p == &request.original)
                && state
                    .admission_close
                    .as_ref()
                    .is_none_or(|p| p == &request.close)
                && state
                    .admission_continuation
                    .as_ref()
                    .is_none_or(|p| p == request),
            "restore changes retained continuation"
        );
        state.admission_pending = Some(request.original.clone());
        state.admission_close = Some(request.close.clone());
        state.admission_continuation = Some(request.clone());
        Ok(())
    }

    /// 显式接续关闭：先在同一Mutex内保留原始/前驱/当前选择，再释放锁并经独立
    /// observer取得新epoch的原生fence，最后才发送关闭。future取消/任何网络错误
    /// 只退出in_flight，保留完整选择；floor推进本身不关闭门禁、不释放原预算。
    pub async fn continue_admission_closure(
        &self,
        request: &DsgAdmissionContinuationRequest,
    ) -> Result<DsgAdmissionContinuationResponse, DsgAdmissionCallError> {
        request.validate().map_err(DsgAdmissionCallError::NotSent)?;
        self.check_admission_binding(&request.close, false)
            .map_err(DsgAdmissionCallError::NotSent)?;
        self.admission_function("neon_dsg_set_admission")
            .await
            .map_err(DsgAdmissionCallError::NotSent)?;
        let payload = serde_json::to_string(&request.close)
            .map_err(|e| DsgAdmissionCallError::NotSent(e.into()))?;
        let mut guard = self
            .reserve_admission_continuation(request)
            .map_err(DsgAdmissionCallError::NotSent)?;
        let fence = self
            .fence_issuer(&DsgIssuerFenceRequest {
                protocol_version: 1,
                instance: request.close.instance.clone(),
                issuer_epoch: request.close.issuer_epoch,
            })
            .await
            .map_err(|e| admission_unconfirmed(&request.close, e.into()))?;
        if fence.disposition == crate::dsg_state::DsgIssuerFenceDisposition::Stale {
            // 完整原生Stale证明本次关闭尚未发送且旧epoch不能迟到生效。
            // 只回退本次新增的SDK保留，原始/前驱未知仍由旧状态及外层WAL保留。
            // 否则过期owner可在新关闭完成后凭尚未到期的签名永久占住共享SDK。
            guard.fenced_out = true;
            return Err(DsgAdmissionCallError::NotSent(anyhow::anyhow!(
                "continuation issuer was already fenced out"
            )));
        }
        if fence.disposition != crate::dsg_state::DsgIssuerFenceDisposition::Fenced {
            return Err(admission_unconfirmed(
                &request.close,
                anyhow::anyhow!("continuation issuer is not fenced"),
            ));
        }
        let response: DsgAdmissionResponse = self
            .call(
                &self.observer,
                &self.observer_schema,
                "neon_dsg_set_admission",
                &payload,
            )
            .await
            .map_err(|e| admission_unconfirmed(&request.close, e))?;
        let proof = DsgAdmissionContinuationResponse {
            protocol_version: 1,
            vm_shrink_permit: false,
            request: request.clone(),
            context_sha256: request
                .context_sha256()
                .map_err(|e| admission_unconfirmed(&request.close, e))?,
            closed_by: response,
            prior_may_have_applied: true,
        };
        proof
            .validate_for(request, &self.instance, false)
            .map_err(|e| admission_unconfirmed(&request.close, e))?;
        guard.resolved = true;
        Ok(proof)
    }

    /// 只读核验最新完整选择，旧选择/普通close-original查询不能清除本地更新后的
    /// 栅栏。没有原生关闭回执时保持未知；不重发setter或合成NotFound/NoEffect。
    pub async fn admission_continuation_status(
        &self,
        request: &DsgAdmissionContinuationRequest,
    ) -> Result<DsgAdmissionContinuationResponse, DsgAdmissionCallError> {
        request.validate().map_err(DsgAdmissionCallError::NotSent)?;
        self.check_admission_binding(&request.close, false)
            .map_err(DsgAdmissionCallError::NotSent)?;
        let native = self.admission_status(&request.close).await?;
        let proof = DsgAdmissionContinuationResponse {
            protocol_version: 1,
            vm_shrink_permit: false,
            request: request.clone(),
            context_sha256: request
                .context_sha256()
                .map_err(|e| admission_unconfirmed(&request.close, e))?,
            closed_by: native,
            prior_may_have_applied: true,
        };
        proof
            .validate_for(request, &self.instance, true)
            .map_err(|e| admission_unconfirmed(&request.close, e))?;
        let mut state = self.execution_state.lock().map_err(|_| {
            DsgAdmissionCallError::NotSent(anyhow::anyhow!("DSGA execution fence is poisoned"))
        })?;
        if state.admission_in_flight
            || state
                .admission_pending
                .as_ref()
                .is_some_and(|p| p != &request.original)
            || state
                .admission_close
                .as_ref()
                .is_some_and(|p| p != &request.close)
            || state
                .admission_continuation
                .as_ref()
                .is_some_and(|p| p != request)
        {
            return Err(DsgAdmissionCallError::NotSent(anyhow::anyhow!(
                "local continuation differs from proof"
            )));
        }
        state.admission_pending = None;
        state.admission_close = None;
        state.admission_continuation = None;
        Ok(proof)
    }

    fn reserve_admission_continuation(
        &self,
        request: &DsgAdmissionContinuationRequest,
    ) -> Result<AdmissionFlight<'_>> {
        let mut state = self
            .execution_state
            .lock()
            .map_err(|_| anyhow::anyhow!("DSGA execution fence is poisoned"))?;
        ensure!(
            !state.admission_in_flight,
            "another admission call is in flight"
        );
        ensure!(
            state
                .admission_pending
                .as_ref()
                .is_none_or(|p| p == &request.original),
            "different original admission"
        );
        if let Some(prior) = &state.admission_continuation {
            ensure!(
                prior == request
                    || (prior.original == request.original
                        && prior.close == request.previous_close),
                "continuation changed immediate predecessor"
            );
        } else if let Some(close) = &state.admission_close {
            ensure!(
                close == &request.previous_close,
                "continuation changed predecessor close"
            );
        } else if state.admission_pending.is_some() {
            ensure!(
                request.previous_close == request.original,
                "continuation lacks original predecessor"
            );
        }
        // 最初未知永不改写；新的选择保留直接前驱。更早前驱通过严格递增epoch
        // 和固定boot/容量/预算被传递封住，无需无限累积链表，也不声称它们未执行。
        let predecessor = RetainedAdmission {
            original: state.admission_pending.clone(),
            close: state.admission_close.clone(),
            continuation: state.admission_continuation.clone(),
        };
        state.admission_pending = Some(request.original.clone());
        state.admission_close = Some(request.close.clone());
        state.admission_continuation = Some(request.clone());
        state.admission_in_flight = true;
        Ok(AdmissionFlight {
            state: &self.execution_state,
            resolved: false,
            predecessor: Some(predecessor),
            fenced_out: false,
        })
    }

    fn reserve_admission(
        &self,
        original: &DsgAdmissionRequest,
        close: Option<&DsgAdmissionRequest>,
    ) -> Result<AdmissionFlight<'_>> {
        let mut state = self
            .execution_state
            .lock()
            .map_err(|_| anyhow::anyhow!("DSGA execution fence is poisoned"))?;
        ensure!(
            !state.admission_in_flight,
            "another admission request is in flight"
        );
        ensure!(
            state.admission_continuation.is_none(),
            "complete continuation requires its own endpoint"
        );
        if let Some(pending) = &state.admission_pending {
            ensure!(
                pending == original,
                "complete original admission must be retained unchanged"
            );
        }
        if let Some(pending_close) = &state.admission_close {
            ensure!(
                close == Some(pending_close),
                "complete original admission closure must be retained unchanged"
            );
        }
        if close.is_none() && original.open {
            ensure!(
                !state.in_flight && state.pending.is_none(),
                "admission cannot open across unresolved resize"
            );
        }
        state.admission_pending = Some(original.clone());
        state.admission_close = close.cloned();
        state.admission_in_flight = true;
        Ok(AdmissionFlight {
            state: &self.execution_state,
            resolved: false,
            predecessor: None,
            fenced_out: false,
        })
    }

    // 基础SDK继续兼容原1.8控制函数；准入调用另核对其真实Neon扩展成员/精确
    // 签名，不将schema中同名SQL或错误升级当成管理能力，无legacy/任意函数兜底。
    async fn admission_function(&self, function: &str) -> Result<()> {
        self.formal_function(function, 1).await
    }

    // 函数名和0/1参数签名仅来自本SDK固定方法，不接受调用方的任意SQL入口。
    async fn formal_function(&self, function: &str, arguments: i16) -> Result<()> {
        timeout(self.response_timeout, async {
            let row = self.observer.query_one("SELECT pg_catalog.count(*)::bigint FROM pg_catalog.pg_extension e \
                JOIN pg_catalog.pg_depend d ON d.refobjid=e.oid AND d.refclassid='pg_catalog.pg_extension'::pg_catalog.regclass \
                    AND d.classid='pg_catalog.pg_proc'::pg_catalog.regclass AND d.deptype='e' \
                JOIN pg_catalog.pg_proc p ON p.oid=d.objid AND p.pronamespace=e.extnamespace \
                JOIN pg_catalog.pg_language l ON l.oid=p.prolang \
                WHERE e.extname='neon' AND e.extnamespace=pg_catalog.to_regnamespace($1) \
                    AND p.proname=$2 AND p.prosrc=p.proname AND p.probin='$libdir/neon' AND l.lanname='c' \
                    AND p.prorettype='pg_catalog.jsonb'::pg_catalog.regtype AND p.pronargs=$3::smallint \
                    AND ($3::smallint=0 OR p.proargtypes[0]='pg_catalog.text'::pg_catalog.regtype) AND p.proisstrict \
                    AND p.provolatile='v' AND p.proparallel='u' AND NOT p.prosecdef AND NOT p.proretset \
                    AND p.pronargdefaults=0", &[&self.observer_schema, &function, &arguments]).await?;
            ensure!(row.get::<_, i64>(0) == 1, "formal Neon management function is missing or incompatible");
            Ok(())
        }).await.context("management function discovery deadline elapsed")?
    }

    fn reserve(&self, request: &DsgResizeRequest) -> Result<InFlight<'_>> {
        let mut state = self
            .execution_state
            .lock()
            .map_err(|_| anyhow::anyhow!("DSGA execution fence is poisoned"))?;
        ensure!(
            !state.in_flight
                && !state.admission_in_flight
                && state.admission_pending.is_none()
                && state.admission_close.is_none(),
            "another DSGA execution or unknown admission needs reconciliation"
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

fn admission_unconfirmed(
    request: &DsgAdmissionRequest,
    source: anyhow::Error,
) -> DsgAdmissionCallError {
    DsgAdmissionCallError::Unconfirmed {
        request: request.clone(),
        source,
    }
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

/// 只供固定无参数只读观察使用，完整消费至ReadyForQuery；不接受动态SQL或载荷。
/// 限一行一列和32KiB，既不以空结果补零，也不在超时后清任何业务Pending。
async fn read_json_simple<T: DeserializeOwned>(
    client: &Client,
    sql: &str,
    wait: Duration,
) -> Result<T> {
    let messages = timeout(wait, client.simple_query(sql))
        .await
        .context("DSGA observation deadline elapsed")??;
    let mut body = None;
    let mut commands = 0;
    for message in &messages {
        match message {
            tokio_postgres::SimpleQueryMessage::Row(row) => {
                ensure!(
                    body.is_none() && row.len() == 1,
                    "DSGA observation row shape differs"
                );
                let value = row.get(0).context("DSGA observation is null")?;
                ensure!(
                    value.len() <= MAX_RESPONSE_BYTES,
                    "DSGA response exceeds 32 KiB"
                );
                body = Some(value);
            }
            tokio_postgres::SimpleQueryMessage::CommandComplete(rows) => {
                ensure!(*rows == 1, "DSGA observation command shape differs");
                commands += 1;
            }
            _ => {}
        }
    }
    ensure!(
        commands == 1,
        "DSGA observation did not complete one command"
    );
    serde_json::from_str(body.context("DSGA observation returned no row")?)
        .context("invalid DSGA observation schema")
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
                  OR (p.proname IN ('neon_dsg_request_resize','neon_dsg_resize_status','neon_dsg_cancel_resize','neon_dsg_fence_issuer') \
                      AND p.pronargs=1 AND p.proargtypes[0]='pg_catalog.text'::pg_catalog.regtype))", &[]).await?;
        // 六个精确签名均须属于真正Neon扩展，不能只看到同名租户SQL函数就启用。
        ensure!(rows.len() == 6, "Neon DSGA management extension functions are missing or incompatible");
        Ok((format!("\"{}\"", schema.replace('"', "\"\"")), pid))
    }).await.context("DSGA capability discovery deadline elapsed")?
}
