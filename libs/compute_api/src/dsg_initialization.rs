//! 原实例初始化的观察回执。认证/角色核验不能代替运行时预算、准入或VM许可。

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::dsg_state::DsgSnapshot;
use crate::dynamic_shared_buffers::DynamicSharedBuffersSpec;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgInitializationResponse {
    pub protocol_version: u32,
    pub vm_shrink_permit: bool,
    pub operation_uuid: String,
    pub tenant_id: String,
    pub timeline_id: String,
    pub startup: DynamicSharedBuffersSpec,
    pub authentication_policy_verified: bool,
    pub ordinary_roles_verified: bool,
    pub observed: DsgSnapshot,
}

impl DsgInitializationResponse {
    /// 接受者必须以原冻结计划核对，不能信任回执自报的目标身份或补缺省字段。
    /// boot是本次真实观察结果；新boot使旧许可失效，不能凭此回执自动重新准入。
    pub fn validate_for(
        &self,
        startup: &DynamicSharedBuffersSpec,
        operation_uuid: &str,
        tenant: &str,
        timeline: &str,
    ) -> Result<()> {
        startup.validate()?;
        self.observed.validate()?;
        ensure!(
            self.protocol_version == 1
                && !self.vm_shrink_permit
                && self.authentication_policy_verified
                && self.ordinary_roles_verified,
            "DSGA initialization proof is incomplete or grants a VM permit"
        );
        ensure!(
            self.startup == *startup
                && self.operation_uuid == operation_uuid
                && !operation_uuid.is_empty()
                && self.tenant_id == tenant
                && self.timeline_id == timeline
                && tenant.len() == 32
                && timeline.len() == 32
                && [tenant, timeline].iter().all(|id| id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
            "DSGA initialization differs from original creation plan"
        );
        ensure!(
            self.observed.instance.vm_uid == startup.vm_uid
                && self.observed.instance.compute_instance_id == startup.compute_instance_id
                && u64::from(self.observed.maximum_buffers) * 8192 == startup.maximum_buffer_bytes
                && u64::from(self.observed.low_buffers) * 8192 >= startup.bootstrap_buffer_bytes,
            "DSGA initialization PG identity or capacity differs"
        );
        Ok(())
    }
}
