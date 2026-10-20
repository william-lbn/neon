//! Neon DSGA管理SQL的有类型事实及跨字段校验。
//!
//! 这些类型不产生VM执行权。即使PG已COMMITTED，Provider事实也可能尚未返回；
//! 历史result与当前observed可以属于不同操作，不能互相填补或拼接预算证明。

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::dsg_control::{
    DSG_CONTROL_PROTOCOL_VERSION, DsgInstanceIdentity, DsgIssuerFenceRequest, DsgOperationKey,
    DsgResizeKind, DsgResizeRequest, DsgUint64, validate_id,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DsgDisposition {
    Accepted,
    Replayed,
    Busy,
    Stale,
    Conflict,
    ResultExpired,
    NotFound,
    CancelRequested,
    TooLate,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DsgPhase {
    Preparing,
    Committed,
    Denied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DsgReason {
    None,
    Timeout,
    PopulateFailed,
    Interrupted,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DsgArrayKind {
    Descriptors,
    Blocks,
    Iocv,
    Checkpoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DsgReleaseState {
    None,
    Pending,
    WaitCheckpoint,
    Verified,
    Unsupported,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DsgProviderStatus {
    Unknown,
    Ok,
    Noop,
    Unsupported,
    Failed,
}

/// Provider回执属于release_key，不一定属于当前操作或当前分配。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgProviderFact {
    pub has_result: bool,
    pub status: DsgProviderStatus,
    pub system_errno: i32,
    pub requested_offset: DsgUint64,
    pub requested_bytes: DsgUint64,
    pub affected_offset: DsgUint64,
    pub affected_bytes: DsgUint64,
    pub physical_release_verified: bool,
    pub allocation_may_be_partial: bool,
    pub release_may_be_partial: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgArrayFact {
    pub array: DsgArrayKind,
    pub committed_upper_bytes: DsgUint64,
    pub allocation_unknown: bool,
    pub release_state: DsgReleaseState,
    #[serde(deserialize_with = "required_nullable")]
    pub release_key: Option<DsgOperationKey>,
    pub release_capacity_generation: DsgUint64,
    pub provider: DsgProviderFact,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgOperationFact {
    pub key: DsgOperationKey,
    pub kind: DsgResizeKind,
    pub phase: DsgPhase,
    pub reason: DsgReason,
    pub target_buffers: u32,
    pub expected_capacity_generation: DsgUint64,
    pub context_sha256: String,
    pub cancel_requested: bool,
    pub cancel_closed: bool,
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgSnapshot {
    pub instance: DsgInstanceIdentity,
    pub issuer_floor_epoch: DsgUint64,
    pub capacity_generation: DsgUint64,
    pub low_buffers: u32,
    pub high_buffers: u32,
    pub maximum_buffers: u32,
    pub admission_open: bool,
    pub admission_revision: DsgUint64,
    /// 含bootstrap数组，不能再与全部当前数组直接求和，否则重复计费。
    pub startup_shared_upper_bytes: DsgUint64,
    #[serde(deserialize_with = "required_nullable")]
    pub operation: Option<DsgOperationFact>,
    pub arrays: Vec<DsgArrayFact>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgReceipt {
    pub key: DsgOperationKey,
    pub disposition: DsgDisposition,
    pub has_result: bool,
    #[serde(deserialize_with = "required_nullable")]
    pub result: Option<DsgSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgControlResponse {
    pub protocol_version: u32,
    pub vm_shrink_permit: bool,
    pub receipt: DsgReceipt,
    pub observed: DsgSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgMemoryResponse {
    pub protocol_version: u32,
    pub vm_shrink_permit: bool,
    pub observed: DsgSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DsgIssuerFenceDisposition {
    Fenced,
    Pending,
    Stale,
}

/// Fenced证明PG串行执行者已退出，不证明Provider物理成功/VM完成；Pending已推进
/// floor但还有旧执行者。两者都不能清除外层原意图、预算或客户端未知请求。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgIssuerFenceResponse {
    pub protocol_version: u32,
    pub vm_shrink_permit: bool,
    pub requested_instance: DsgInstanceIdentity,
    pub requested_epoch: DsgUint64,
    pub disposition: DsgIssuerFenceDisposition,
    pub observed: DsgSnapshot,
}

impl DsgIssuerFenceResponse {
    pub fn validate_for(
        &self,
        request: &DsgIssuerFenceRequest,
        bound: &DsgInstanceIdentity,
    ) -> Result<()> {
        request.validate()?;
        validate_envelope(self.protocol_version, self.vm_shrink_permit)?;
        self.observed.validate()?;
        ensure!(
            self.requested_instance == request.instance
                && self.requested_epoch == request.issuer_epoch
                && self.observed.instance == *bound,
            "issuer fence request or observed binding differs"
        );
        match self.disposition {
            DsgIssuerFenceDisposition::Fenced | DsgIssuerFenceDisposition::Pending => {
                ensure!(
                    request.instance == *bound
                        && self.observed.issuer_floor_epoch == request.issuer_epoch,
                    "issuer fence did not establish requested floor"
                );
                if self.disposition == DsgIssuerFenceDisposition::Fenced {
                    ensure!(
                        !self
                            .observed
                            .operation
                            .as_ref()
                            .is_some_and(|op| op.phase == DsgPhase::Preparing),
                        "fenced response still has an unreconciled operation"
                    );
                }
            }
            DsgIssuerFenceDisposition::Stale => ensure!(
                request.instance != *bound
                    || self.observed.issuer_floor_epoch > request.issuer_epoch,
                "stale fence lacks a conflicting boot/instance or newer floor"
            ),
        }
        Ok(())
    }
}

/// 禁用/不支持的能力响应允许无boot或没有instance字段，不能补成合法运行实例。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgDiscoveryIdentity {
    pub vm_uid: String,
    pub compute_instance_id: String,
    #[serde(deserialize_with = "required_nullable")]
    pub pg_boot_id: Option<DsgUint64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgCapabilities {
    pub protocol_version: u32,
    pub production_ready: bool,
    pub vm_shrink_permit: bool,
    pub supported: bool,
    pub buffer_api_version: u32,
    pub enabled: bool,
    pub instance_bound: bool,
    pub history_capacity: Option<u32>,
    pub provider_can_release: Option<bool>,
    pub instance: Option<DsgDiscoveryIdentity>,
}

// 原生协议总会写这些nullable字段；缺字段与显式null不同，不能由Option默认补齐。
fn required_nullable<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

fn validate_instance(instance: &DsgInstanceIdentity) -> Result<()> {
    validate_id(&instance.vm_uid, "VM")?;
    validate_id(&instance.compute_instance_id, "Compute")?;
    ensure!(instance.pg_boot_id.get() != 0, "DSGA boot must be nonzero");
    Ok(())
}

fn validate_envelope(version: u32, vm_shrink_permit: bool) -> Result<()> {
    ensure!(
        version == DSG_CONTROL_PROTOCOL_VERSION,
        "unsupported DSGA response version"
    );
    ensure!(
        !vm_shrink_permit,
        "PG facts cannot issue a VM shrink permit"
    );
    Ok(())
}

impl DsgCapabilities {
    /// 仅协商逻辑控制能力，不将production_ready或Provider能力当资源授权。
    pub fn bind_instance(
        &self,
        vm_uid: &str,
        compute_instance_id: &str,
    ) -> Result<DsgInstanceIdentity> {
        validate_envelope(self.protocol_version, self.vm_shrink_permit)?;
        validate_id(vm_uid, "VM")?;
        validate_id(compute_instance_id, "Compute")?;
        ensure!(
            self.supported && self.buffer_api_version == 6,
            "DSGA Buffer ABI6 is required"
        );
        ensure!(
            self.enabled && self.instance_bound,
            "DSGA is disabled or not bound"
        );
        ensure!(
            self.history_capacity.is_some_and(|n| n > 0),
            "missing DSGA bounded history capability"
        );
        ensure!(
            self.provider_can_release.is_some(),
            "missing DSGA Provider capability"
        );
        let instance = self
            .instance
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing DSGA instance"))?;
        ensure!(
            instance.vm_uid == vm_uid && instance.compute_instance_id == compute_instance_id,
            "DSGA server belongs to a different VM or Compute instance"
        );
        let result = DsgInstanceIdentity {
            vm_uid: instance.vm_uid.clone(),
            compute_instance_id: instance.compute_instance_id.clone(),
            pg_boot_id: instance
                .pg_boot_id
                .ok_or_else(|| anyhow::anyhow!("missing DSGA boot"))?,
        };
        validate_instance(&result)?;
        Ok(result)
    }
}

impl DsgSnapshot {
    /// 校验一次完整快照，不要求其operation等于另一个快照里的operation。
    pub fn validate(&self) -> Result<()> {
        validate_instance(&self.instance)?;
        ensure!(
            self.capacity_generation.get() != 0,
            "missing DSGA capacity generation"
        );
        ensure!(
            16 <= self.low_buffers
                && self.low_buffers <= self.high_buffers
                && self.high_buffers <= self.maximum_buffers
                && self.maximum_buffers <= (i32::MAX / 2) as u32,
            "inconsistent DSGA capacity bounds"
        );
        ensure!(
            self.startup_shared_upper_bytes.get() > 0,
            "missing DSGA startup cost"
        );
        ensure!(
            self.arrays.len() == 4,
            "DSGA requires four independent array facts"
        );
        let mut array_mask = 0;
        for array in &self.arrays {
            let bit = match array.array {
                DsgArrayKind::Descriptors => 1,
                DsgArrayKind::Blocks => 2,
                DsgArrayKind::Iocv => 4,
                DsgArrayKind::Checkpoint => 8,
            };
            ensure!(array_mask & bit == 0, "duplicate DSGA array fact");
            array_mask |= bit;
            ensure!(
                array.committed_upper_bytes.get() > 0,
                "missing DSGA array cost"
            );
            match (&array.release_state, &array.release_key) {
                (DsgReleaseState::None, None) => ensure!(
                    array.release_capacity_generation.get() == 0,
                    "unstarted release has a generation"
                ),
                (DsgReleaseState::None, Some(_)) | (_, None) => {
                    anyhow::bail!("inconsistent DSGA release identity")
                }
                (_, Some(key)) => {
                    key.validate()?;
                    ensure!(
                        key.instance == self.instance,
                        "release belongs to another instance"
                    );
                    ensure!(
                        key.issuer_epoch <= self.issuer_floor_epoch,
                        "release epoch exceeds issuer floor"
                    );
                    ensure!(
                        array.release_capacity_generation.get() > 0
                            && array.release_capacity_generation <= self.capacity_generation,
                        "inconsistent DSGA release generation"
                    );
                }
            }
            let known = matches!(
                array.release_state,
                DsgReleaseState::Verified | DsgReleaseState::Unsupported | DsgReleaseState::Failed
            );
            ensure!(
                array.provider.has_result == known,
                "inconsistent Provider result presence"
            );
            ensure!(array.provider.system_errno >= 0, "invalid Provider errno");
            ensure!(
                array
                    .provider
                    .requested_offset
                    .get()
                    .checked_add(array.provider.requested_bytes.get())
                    .is_some()
                    && array
                        .provider
                        .affected_offset
                        .get()
                        .checked_add(array.provider.affected_bytes.get())
                        .is_some(),
                "Provider range overflows uint64"
            );
            if !known {
                ensure!(
                    array.provider.status == DsgProviderStatus::Unknown
                        && !array.provider.physical_release_verified,
                    "unconfirmed Provider result cannot report success"
                );
            } else {
                ensure!(
                    matches!(
                        (array.release_state, array.provider.status),
                        (
                            DsgReleaseState::Verified,
                            DsgProviderStatus::Ok | DsgProviderStatus::Noop
                        ) | (DsgReleaseState::Unsupported, DsgProviderStatus::Unsupported)
                            | (
                                DsgReleaseState::Failed,
                                DsgProviderStatus::Failed | DsgProviderStatus::Noop
                            )
                    ),
                    "inconsistent Provider terminal result"
                );
            }
            if array.provider.physical_release_verified {
                ensure!(
                    known
                        && array.provider.status == DsgProviderStatus::Ok
                        && array.provider.affected_bytes.get() > 0
                        && !array.provider.release_may_be_partial,
                    "invalid physical release proof"
                );
            }
            // 旧release可仍VERIFIED，而新的Grow已预触/unknown；绝不能据旧回执
            // 推断当前成本已回收，所以不把旧release与当前allocation_unknown强行等同。
        }
        if let Some(operation) = &self.operation {
            operation.key.validate()?;
            ensure!(
                operation.key.issuer_epoch <= self.issuer_floor_epoch,
                "operation epoch exceeds issuer floor"
            );
            ensure!(
                operation.key.instance == self.instance,
                "operation belongs to another instance"
            );
            ensure!(
                (16..=self.maximum_buffers).contains(&operation.target_buffers),
                "invalid operation target"
            );
            ensure!(
                operation.expected_capacity_generation.get() > 0,
                "missing operation generation"
            );
            ensure!(
                operation.context_sha256.len() == 64
                    && operation
                        .context_sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "invalid canonical intent hash"
            );
            ensure!(
                !(operation.cancel_requested && operation.cancel_closed),
                "cancel window is contradictory"
            );
            match operation.phase {
                DsgPhase::Preparing | DsgPhase::Denied => {
                    ensure!(
                        self.capacity_generation == operation.expected_capacity_generation,
                        "uncommitted operation changed generation"
                    );
                    ensure!(
                        match operation.kind {
                            DsgResizeKind::Grow => operation.target_buffers >= self.high_buffers,
                            DsgResizeKind::Shrink => operation.target_buffers <= self.high_buffers,
                        },
                        "operation direction conflicts with capacity"
                    );
                }
                DsgPhase::Committed => {
                    ensure!(
                        self.high_buffers == operation.target_buffers
                            && self.low_buffers == self.high_buffers
                            && operation.cancel_closed,
                        "committed operation did not publish target"
                    );
                    let expected = operation.expected_capacity_generation.get();
                    ensure!(
                        self.capacity_generation.get() == expected
                            || expected.checked_add(1) == Some(self.capacity_generation.get()),
                        "inconsistent committed generation"
                    );
                }
            }
            if operation.phase == DsgPhase::Denied {
                ensure!(
                    operation.reason != DsgReason::None && self.low_buffers == self.high_buffers,
                    "denied operation lacks restored capacity or reason"
                );
                if operation.reason == DsgReason::Cancelled {
                    ensure!(
                        operation.cancel_requested && !operation.cancel_closed,
                        "cancelled operation lacks a cancellation request"
                    );
                }
            } else {
                ensure!(
                    operation.reason == DsgReason::None,
                    "active/committed operation has denial reason"
                );
            }
        }
        Ok(())
    }
}

impl DsgControlResponse {
    /// 原请求与当前实例分别核验；旧历史结果不能被当前observed替换。
    pub fn validate_for(&self, key: &DsgOperationKey, bound: &DsgInstanceIdentity) -> Result<()> {
        validate_envelope(self.protocol_version, self.vm_shrink_permit)?;
        key.validate()?;
        ensure!(
            self.receipt.key == *key,
            "receipt belongs to a different request"
        );
        self.observed.validate()?;
        ensure!(
            self.observed.instance == *bound,
            "DSGA instance or PG boot changed"
        );
        ensure!(
            self.receipt.has_result == self.receipt.result.is_some(),
            "inconsistent receipt presence"
        );
        let needs_result = matches!(
            self.receipt.disposition,
            DsgDisposition::Accepted
                | DsgDisposition::Replayed
                | DsgDisposition::CancelRequested
                | DsgDisposition::TooLate
        );
        ensure!(
            self.receipt.has_result == needs_result,
            "receipt disposition contradicts result"
        );
        if let Some(result) = &self.receipt.result {
            result.validate()?;
            ensure!(
                result.instance == key.instance,
                "historical result belongs to another instance"
            );
            let operation = result
                .operation
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("receipt lacks operation"))?;
            ensure!(
                operation.key == *key,
                "historical result belongs to another operation"
            );
            if self.receipt.disposition == DsgDisposition::CancelRequested {
                ensure!(
                    operation.phase == DsgPhase::Preparing
                        && operation.cancel_requested
                        && !operation.cancel_closed,
                    "cancellation receipt lacks an open cancelled operation"
                );
            }
            if self.receipt.disposition == DsgDisposition::TooLate {
                ensure!(
                    operation.phase != DsgPhase::Preparing || operation.cancel_closed,
                    "too-late receipt still has an open cancellation window"
                );
            }
        }
        Ok(())
    }

    pub fn validate_resize(
        &self,
        request: &DsgResizeRequest,
        bound: &DsgInstanceIdentity,
    ) -> Result<()> {
        request.validate()?;
        self.validate_for(&request.key, bound)?;
        if let Some(result) = &self.receipt.result {
            let operation = result
                .operation
                .as_ref()
                .expect("validated receipt operation");
            ensure!(
                operation.kind == request.kind
                    && operation.target_buffers == request.target_buffers
                    && operation.expected_capacity_generation
                        == request.expected_capacity_generation
                    && operation.context_sha256 == request.context_sha256()?,
                "receipt result changed the original resize intent"
            );
        }
        Ok(())
    }
}

impl DsgMemoryResponse {
    pub fn validate_for(&self, bound: &DsgInstanceIdentity) -> Result<()> {
        validate_envelope(self.protocol_version, self.vm_shrink_permit)?;
        self.observed.validate()?;
        ensure!(
            self.observed.instance == *bound,
            "DSGA instance or PG boot changed"
        );
        Ok(())
    }
}
