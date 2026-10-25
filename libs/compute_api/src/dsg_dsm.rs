//! 同一PG boot的外部DSM payload预留：创建前即计费，未知不归零，主段去重。
//! 这不是RSS/PTE/THP/Host上界或跨boot清理证明，不能单独许可VM缩容。
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    dsg_control::{DsgInstanceIdentity, DsgUint64},
    dsg_prepared::DsgPreparedResponse,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgDsmResponse {
    pub protocol_version: u32,
    pub dsm_api_version: u32,
    pub vm_shrink_permit: bool,
    pub current_boot_only: bool,
    pub reserved_upper_bytes: DsgUint64,
    pub control_upper_bytes: DsgUint64,
    pub external_budget_bytes: DsgUint64,
    pub external_segments: u32,
    pub pending_creations: u32,
    pub unknown_segments: u32,
    pub page_bytes: u32,
    pub limit_enforced: bool,
    pub accounting_valid: bool,
    pub prepared: DsgPreparedResponse,
}

impl DsgDsmResponse {
    pub fn validate_for(&self, bound: &DsgInstanceIdentity) -> Result<()> {
        self.prepared.validate_for(bound)?;
        ensure!(
            self.protocol_version == 1
                && self.dsm_api_version == 1
                && !self.vm_shrink_permit
                && self.current_boot_only
                && self.accounting_valid,
            "DSM observation version, scope or accounting is unavailable"
        );
        let page = u64::from(self.page_bytes);
        let used = self.reserved_upper_bytes.get();
        let control = self.control_upper_bytes.get();
        let budget = self.external_budget_bytes.get();
        // 页取实际OS基础粒度，不硬编码4KB；控制段独立于main和外部payload额度。
        // 未知是已保留份额，pending/unknown互斥；不能从foreground_idle补零。
        ensure!(
            self.page_bytes.is_power_of_two()
                && self.external_segments <= i32::MAX as u32
                && self
                    .pending_creations
                    .checked_add(self.unknown_segments)
                    .is_some_and(|n| n <= self.external_segments),
            "DSM slot accounting differs"
        );
        ensure!(
            control > 0
                && control % page == 0
                && used % page == 0
                && used >= u64::from(self.external_segments) * page
                && (used == 0) == (self.external_segments == 0),
            "DSM payload alignment or occupancy differs"
        );
        // GUC按KB固定于本boot，0明确未约束。未知段可以保留整个原额度，不能删字段
        // 换取通过；只有有效且正数的持续cap才可被未来完整预算生产者另外核验使用。
        ensure!(
            self.limit_enforced == (budget > 0)
                && budget <= (i32::MAX as u64) * 1024
                && budget % 1024 == 0
                && (!self.limit_enforced || used <= budget),
            "DSM configured payload limit contradicts native reservation"
        );
        Ok(())
    }
}
