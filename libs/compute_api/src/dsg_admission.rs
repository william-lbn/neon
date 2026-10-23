//! 准入完整意图/回执，不把门禁开关、未见/淘汰或PG栅栏当预算/排空/VM完成。
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::dsg_control::{DsgInstanceIdentity, DsgUint64, canonical_context_sha256, validate_id};
use crate::dsg_state::DsgSnapshot;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgAdmissionRequest {
    pub protocol_version: u32,
    pub instance: DsgInstanceIdentity,
    pub issuer_epoch: DsgUint64,
    pub revision: DsgUint64,
    pub expected_capacity_generation: DsgUint64,
    pub open: bool,
    pub budget_commitment_id: String,
}

impl DsgAdmissionRequest {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.protocol_version == 1, "unsupported admission protocol");
        validate_id(&self.instance.vm_uid, "VM")?;
        validate_id(&self.instance.compute_instance_id, "Compute")?;
        validate_id(&self.budget_commitment_id, "budget commitment")?;
        ensure!(
            self.instance.pg_boot_id.get() != 0
                && self.issuer_epoch.get() != 0
                && self.revision.get() != 0
                && self.expected_capacity_generation.get() != 0,
            "admission identity must be nonzero"
        );
        Ok(())
    }

    pub fn context_sha256(&self) -> Result<String> {
        self.validate()?;
        canonical_context_sha256(self)
    }

    /// 完整关闭回执必须以更高floor或同epoch更高准入revision封住旧请求。
    /// 它只证明旧请求以后不能再改门禁，不否定旧门禁曾开放或已有工作占用。
    pub fn closes_over(&self, original: &Self) -> Result<()> {
        self.validate()?;
        original.validate()?;
        ensure!(
            !self.open
                && self.instance == original.instance
                && (self.issuer_epoch > original.issuer_epoch
                    || (self.issuer_epoch == original.issuer_epoch
                        && self.revision > original.revision)),
            "admission closure does not fence original request"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DsgAdmissionDisposition {
    Applied,
    Replayed,
    Busy,
    Stale,
    Conflict,
    NotFound,
    ResultExpired,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgAdmissionResponse {
    pub protocol_version: u32,
    pub vm_shrink_permit: bool,
    pub request: DsgAdmissionRequest,
    pub disposition: DsgAdmissionDisposition,
    pub context_sha256: String,
    pub observed: DsgSnapshot,
}

impl DsgAdmissionResponse {
    pub fn validate_for(
        &self,
        request: &DsgAdmissionRequest,
        bound: &DsgInstanceIdentity,
        query: bool,
    ) -> Result<()> {
        request.validate()?;
        self.observed.validate()?;
        ensure!(
            self.protocol_version == 1
                && !self.vm_shrink_permit
                && self.request == *request
                && self.context_sha256 == request.context_sha256()?
                && self.observed.instance == *bound,
            "admission response differs from complete original request or bound instance"
        );
        let state = &self.observed;
        let same_boot = request.instance == *bound;
        let rev = request.revision;
        use DsgAdmissionDisposition::*;
        let valid = match self.disposition {
            Applied => {
                !query
                    && same_boot
                    && state.issuer_floor_epoch == request.issuer_epoch
                    && state.capacity_generation == request.expected_capacity_generation
                    && state.admission_revision == rev
                    && state.admission_open == request.open
            }
            // 更高floor/后续resize不抹除已应用的最后准入回执；不能按当前generation
            // 否定原完整摘要，但setter本身仍要求相等floor，不准跨owner执行。
            Replayed => {
                same_boot
                    && state.admission_revision == rev
                    && state.admission_open == request.open
                    && state.issuer_floor_epoch >= request.issuer_epoch
                    && state.capacity_generation >= request.expected_capacity_generation
                    && (query || state.issuer_floor_epoch == request.issuer_epoch)
            }
            NotFound => query && same_boot && rev > state.admission_revision,
            ResultExpired => query && same_boot && rev < state.admission_revision,
            Busy => !query && request.open,
            Conflict => {
                same_boot
                    && (rev == state.admission_revision
                        || (!query
                            && state
                                .admission_revision
                                .get()
                                .checked_add(1)
                                .is_some_and(|next| rev.get() > next)))
            }
            Stale => {
                !same_boot
                    || (!query
                        && (request.issuer_epoch != state.issuer_floor_epoch
                            || rev < state.admission_revision
                            || request.expected_capacity_generation != state.capacity_generation))
            }
        };
        ensure!(
            valid,
            "admission disposition is inconsistent with action or observed fact"
        );
        Ok(())
    }

    pub fn confirms(&self) -> bool {
        matches!(
            self.disposition,
            DsgAdmissionDisposition::Applied | DsgAdmissionDisposition::Replayed
        )
    }
}

/// 仅本地SDK未知门禁写入已被实际更高关闭回执封住；外层仍要按原意图核验
/// 已开始工作/预算/Guest/VM，不能把此值当“旧请求未执行”或预算可撤销。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgAdmissionClosure {
    pub protocol_version: u32,
    pub vm_shrink_permit: bool,
    pub original: DsgAdmissionRequest,
    pub closed_by: DsgAdmissionResponse,
    pub prior_may_have_applied: bool,
}

/// 关闭未知原意图必须同时携带原完整请求和显式更高关闭意图。不能仅给一个
/// revision或“重置pending”标志；外层在网络发送前须持久保存两份不可变意图。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgAdmissionCloseRequest {
    pub protocol_version: u32,
    pub original: DsgAdmissionRequest,
    pub close: DsgAdmissionRequest,
}

impl DsgAdmissionCloseRequest {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.protocol_version == 1,
            "unsupported admission close protocol"
        );
        self.close.closes_over(&self.original)
    }
}
