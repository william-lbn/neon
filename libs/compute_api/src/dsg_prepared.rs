//! 原生两阶段保留槽的同点事实；运行期关门不阻止WAL/磁盘恢复旧槽，不发VM许可。
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{dsg_connection::DsgConnectionResponse, dsg_control::DsgInstanceIdentity};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgPreparedResponse {
    pub protocol_version: u32,
    pub prepared_api_version: u32,
    pub vm_shrink_permit: bool,
    pub prepared_slots: u32,
    pub maximum_prepared_slots: u32,
    pub runtime_prepares_blocked: bool,
    pub connections: DsgConnectionResponse,
}

impl DsgPreparedResponse {
    pub fn validate_for(&self, bound: &DsgInstanceIdentity) -> Result<()> {
        self.connections.validate_for(bound)?;
        ensure!(
            self.protocol_version == 1 && self.prepared_api_version == 1 && !self.vm_shrink_permit,
            "prepared observation protocol or permission differs"
        );
        // 最大槽是本boot固定PG GUC；0表示功能禁用，不由客户端补默认值。
        // frontend_idle可以与非零槽共存，不能把两者强行等同或丢掉已PREPARED资源。
        ensure!(
            self.maximum_prepared_slots <= i32::MAX as u32
                && self.prepared_slots <= self.maximum_prepared_slots
                && self.runtime_prepares_blocked != self.connections.work.observed.admission_open,
            "prepared slots or runtime gate contradict same-point native state"
        );
        Ok(())
    }
}
