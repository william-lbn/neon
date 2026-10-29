//! 有期限的原生准入防线；续租只维持原授权，绝不开放业务或授予VM许可。
use crate::dsg_control::{DsgInstanceIdentity, DsgUint64, validate_id};
use crate::dsg_state::DsgSnapshot;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DsgAdmissionLeaseAction {
    Observe,
    Renew,
    Rearm,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgAdmissionLeaseRequest {
    pub protocol_version: u32,
    pub instance: DsgInstanceIdentity,
    pub issuer_epoch: DsgUint64,
    pub expected_capacity_generation: DsgUint64,
    pub expected_admission_revision: DsgUint64,
    pub expected_deadline_ns: DsgUint64,
    pub action: DsgAdmissionLeaseAction,
}
impl DsgAdmissionLeaseRequest {
    pub fn validate(&self) -> Result<()> {
        validate_id(&self.instance.vm_uid, "VM")?;
        validate_id(&self.instance.compute_instance_id, "Compute")?;
        ensure!(
            self.protocol_version == 1
                && self.instance.pg_boot_id.get() > 0
                && self.issuer_epoch.get() > 0
                && self.expected_capacity_generation.get() > 0,
            "invalid admission lease identity"
        );
        ensure!(
            self.action == DsgAdmissionLeaseAction::Observe || self.expected_deadline_ns.get() > 0,
            "admission lease write requires original native deadline"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgAdmissionLeaseResponse {
    pub protocol_version: u32,
    pub vm_shrink_permit: bool,
    pub request: DsgAdmissionLeaseRequest,
    pub renewed: bool,
    pub armed: bool,
    pub expired: bool,
    pub deadline_ns: DsgUint64,
    pub lease_epoch: DsgUint64,
    pub timeout_ms: u32,
    pub observed: DsgSnapshot,
}
impl DsgAdmissionLeaseResponse {
    pub fn validate_for(
        &self,
        request: &DsgAdmissionLeaseRequest,
        bound: &DsgInstanceIdentity,
    ) -> Result<()> {
        request.validate()?;
        self.observed.validate()?;
        ensure!(
            self.protocol_version == 1
                && !self.vm_shrink_permit
                && self.request == *request
                && request.instance == *bound
                && self.observed.instance == *bound
                && (1000..=60_000).contains(&self.timeout_ms)
                && self.deadline_ns.get() <= i64::MAX as u64
                && self.lease_epoch <= self.observed.issuer_floor_epoch,
            "admission lease response differs from bound native context"
        );
        ensure!(
            if self.armed {
                self.deadline_ns.get() > 0
            } else {
                self.deadline_ns.get() == 0 && self.lease_epoch.get() == 0 && !self.expired
            },
            "invalid admission lease lifetime"
        );
        ensure!(
            !self.expired || !self.observed.admission_open,
            "expired admission remains open"
        );
        ensure!(
            !self.observed.admission_open || self.armed,
            "open admission lacks native deadline"
        );
        if self.renewed {
            ensure!(
                request.action != DsgAdmissionLeaseAction::Observe
                    && self.armed
                    && !self.expired
                    && self.lease_epoch == request.issuer_epoch
                    && self.observed.issuer_floor_epoch == request.issuer_epoch
                    && self.observed.capacity_generation == request.expected_capacity_generation
                    && self.observed.admission_revision == request.expected_admission_revision
                    && self.deadline_ns > request.expected_deadline_ns
                    && (request.action != DsgAdmissionLeaseAction::Rearm
                        || !self.observed.admission_open),
                "renewal contradicts original native CAS"
            );
        }
        Ok(())
    }
}
