//! DSGA HTTP管理面的专用授权契约；签名证明调用身份，不代替资源承诺或issuer租约。

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::dsg_control::{DsgOperationKey, DsgUint64, validate_id};

pub const DSG_HTTP_AUDIENCE: &str = "neon-dsga";
pub const DSG_HTTP_SCOPE: &str = "compute_ctl:dsga";
/// 短期凭据限制暴露窗口；过期不撤销已发送SQL或预算，必须继续按原key对账。
pub const DSG_HTTP_MAX_TOKEN_LIFETIME_SECONDS: u64 = 300;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgHttpClaims {
    pub aud: String,
    pub scope: String,
    pub compute_id: String,
    pub vm_uid: String,
    pub compute_instance_id: String,
    pub issuer_epoch: DsgUint64,
    // JWT标准时间是JSON整数秒，区别于业务协议中字符串毫秒的deadline。
    pub exp: u64,
    pub nbf: u64,
}

impl DsgHttpClaims {
    /// 只有验签成功后才可使用本校验；结构合法不表示签名或issuer租约有效。
    pub fn validate_for_compute(&self, compute_id: &str) -> Result<()> {
        ensure!(self.aud == DSG_HTTP_AUDIENCE, "wrong DSGA audience");
        ensure!(self.scope == DSG_HTTP_SCOPE, "wrong DSGA scope");
        for (value, name) in [
            (&self.compute_id, "HTTP Compute"),
            (&self.vm_uid, "VM"),
            (&self.compute_instance_id, "Compute instance"),
        ] {
            validate_id(value, name)?;
        }
        ensure!(self.compute_id == compute_id, "wrong HTTP Compute identity");
        ensure!(self.issuer_epoch.get() != 0, "zero DSGA issuer epoch");
        let lifetime = self.exp.checked_sub(self.nbf);
        ensure!(
            lifetime.is_some_and(
                |seconds| seconds > 0 && seconds <= DSG_HTTP_MAX_TOKEN_LIFETIME_SECONDS
            ),
            "invalid DSGA token lifetime"
        );
        Ok(())
    }

    /// 新执行只能使用当前授权epoch；新owner可查询/取消旧epoch，不能重发旧执行。
    /// lease真伪及VM动作仍由后续持久owner/最终执行者核验，不能只信此数值。
    pub fn authorize_key(&self, key: &DsgOperationKey, execution: bool) -> Result<()> {
        key.validate()?;
        ensure!(
            key.instance.vm_uid == self.vm_uid
                && key.instance.compute_instance_id == self.compute_instance_id,
            "DSGA request and credential instance differ"
        );
        ensure!(
            if execution {
                key.issuer_epoch == self.issuer_epoch
            } else {
                key.issuer_epoch <= self.issuer_epoch
            },
            "DSGA request epoch is not authorized"
        );
        Ok(())
    }
}
