//! Postmaster持续连接门禁的同点确认，不授权完整Quiesce、预算或VM动作。
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{dsg_control::DsgInstanceIdentity, dsg_work::DsgWorkResponse};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgConnectionResponse {
    pub protocol_version: u32,
    pub connection_api_version: u32,
    pub vm_shrink_permit: bool,
    pub enabled: bool,
    pub requested_serial: u32,
    pub applied_serial: u32,
    pub requested_open: bool,
    pub applied_open: bool,
    pub acknowledged: bool,
    pub private_management_ready: bool,
    pub public_connections_blocked: bool,
    pub work: DsgWorkResponse,
}

impl DsgConnectionResponse {
    pub fn validate_for(&self, bound: &DsgInstanceIdentity) -> Result<()> {
        self.work.validate_for(bound)?;
        ensure!(
            self.protocol_version == 1
                && self.connection_api_version == 1
                && !self.vm_shrink_permit,
            "connection observation protocol or permission differs"
        );
        // 序号是原生word的高31位。比较开闭位才能区分饱和序号下仍允许的最后关门，
        // 不可仅比较serial或把PG接受原意图当Postmaster已执行。
        ensure!(
            self.requested_serial <= u32::MAX >> 1 && self.applied_serial <= self.requested_serial,
            "connection serial is invalid or moves beyond requested state"
        );
        if self.enabled {
            ensure!(
                self.requested_serial > 0
                    && self.requested_open == self.work.observed.admission_open
                    && (self.applied_serial != 0 || !self.applied_open),
                "connection gate contradicts same-point native admission"
            );
        } else {
            ensure!(
                self.requested_serial == 0
                    && self.applied_serial == 0
                    && !self.requested_open
                    && !self.applied_open
                    && !self.private_management_ready,
                "disabled gate cannot claim an applied private channel"
            );
        }
        let acknowledged = self.enabled
            && self.requested_serial > 0
            && self.requested_serial == self.applied_serial
            && self.requested_open == self.applied_open;
        ensure!(
            self.acknowledged == acknowledged
                && self.public_connections_blocked == (acknowledged && !self.requested_open),
            "connection acknowledgement or public blockade contradicts native state"
        );
        Ok(())
    }
}
