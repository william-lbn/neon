//! Work2含Postmaster fork前至PGPROC交接的启动工作。不是完整Quiesce或VM执行权。
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{dsg_control::DsgInstanceIdentity, dsg_state::DsgSnapshot};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgWorkResponse {
    pub protocol_version: u32,
    pub work_api_version: u32,
    pub vm_shrink_permit: bool,
    pub client_processes: u32,
    pub startup_clients: u32,
    pub idle_clients: u32,
    pub frontend_reads: u32,
    pub frontend_executes: u32,
    pub bounded_inputs: u32,
    pub retained_transactions: u32,
    pub background_processes: u32,
    pub auxiliary_processes: u32,
    pub observer_excluded: bool,
    pub foreground_idle: bool,
    pub observed: DsgSnapshot,
}

impl DsgWorkResponse {
    pub fn validate_for(&self, bound: &DsgInstanceIdentity) -> Result<()> {
        self.observed.validate()?;
        ensure!(
            self.protocol_version == 1
                && self.work_api_version == 2
                && !self.vm_shrink_permit
                && self.observed.instance == *bound,
            "work observation envelope or native instance differs"
        );
        // 各互斥阶段必须覆盖全部已登记client；用u64求和，拒绝u32回绕伪零。
        let clients: u64 = [
            self.startup_clients,
            self.idle_clients,
            self.frontend_reads,
            self.frontend_executes,
            self.bounded_inputs,
        ]
        .iter()
        .map(|value| u64::from(*value))
        .sum();
        ensure!(
            clients == u64::from(self.client_processes)
                && self.retained_transactions <= self.client_processes,
            "work phase or retained transaction counts contradict native totals"
        );
        // 有界拒绝输入仍占固定连接预算；前台空闲不能代替连接/后台/DSM/Guest证明。
        let idle = !self.observed.admission_open
            && self.startup_clients == 0
            && self.frontend_reads == 0
            && self.frontend_executes == 0
            && self.retained_transactions == 0;
        ensure!(
            self.foreground_idle == idle,
            "work idle contradicts native gate or activity"
        );
        Ok(())
    }
}
