//! DSGA HTTP管理面的专用授权契约；签名证明调用身份，不代替资源承诺或issuer租约。

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::dsg_admission::DsgAdmissionRequest;
use crate::dsg_control::{DsgIssuerFenceRequest, DsgOperationKey, DsgUint64, validate_id};

pub const DSG_HTTP_AUDIENCE: &str = "neon-dsga";
pub const DSG_HTTP_SCOPE: &str = "compute_ctl:dsga";
/// 专用原请求凭据，不能借给普通PG/资源动作。签名覆盖准确method/path/body摘要。
pub const DSG_HTTP_ARRAY_AUTHORITY_SCOPE: &str = "compute_ctl:dsga:array-authority";
/// 创建控制面只读回执，不能借该token执行resize、接管或取消。
pub const DSG_HTTP_INITIALIZATION_SCOPE: &str = "compute_ctl:dsga_initialization";
pub const DSG_HTTP_SERVICE_LOGIN_SCOPE: &str = "compute_ctl:dsga:service-login";
pub const DSG_SERVICE_LOGIN_PATH: &str = "/dsg/v1/catalog/service-login";
/// 短期凭据限制暴露窗口；过期不撤销已发送SQL或预算，必须继续按原key对账。
pub const DSG_HTTP_MAX_TOKEN_LIFETIME_SECONDS: u64 = 300;

pub fn is_array_authority_path(path: &str) -> bool {
    matches!(
        path,
        "/dsg/v1/resource/array-authorize"
            | "/dsg/v1/resource/array-observe"
            | "/dsg/v1/resource/array-replay-original"
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgHttpRequestBinding {
    pub protocol_version: u32,
    pub method: String,
    pub path: String,
    pub body_sha256: String,
}

impl DsgHttpRequestBinding {
    pub fn validate(&self) -> Result<()> {
        self.validate_for_path(is_array_authority_path(&self.path))
    }

    fn validate_for_path(&self, allowed: bool) -> Result<()> {
        ensure!(
            self.protocol_version == 1 && self.method == "POST" && allowed,
            "unsupported bound request"
        );
        ensure!(
            self.body_sha256.len() == 64
                && self.body_sha256.bytes().any(|b| b != b'0')
                && self
                    .body_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "noncanonical bound body digest"
        );
        Ok(())
    }
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<DsgHttpRequestBinding>,
}

impl DsgHttpClaims {
    pub fn validate_service_login_for_compute(&self, compute_id: &str) -> Result<()> {
        self.validate_for_scope(compute_id, DSG_HTTP_SERVICE_LOGIN_SCOPE)?;
        let binding = self
            .request
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("bound request missing"))?;
        binding.validate_for_path(binding.path == DSG_SERVICE_LOGIN_PATH)
    }

    pub fn authorize_service_login(&self, body: &[u8]) -> Result<()> {
        self.validate_service_login_for_compute(&self.compute_id)?;
        ensure!(
            body.len() <= 4096
                && self.request.as_ref().is_some_and(
                    |binding| binding.body_sha256 == format!("{:x}", Sha256::digest(body))
                ),
            "original service login request differs"
        );
        Ok(())
    }

    pub fn validate_array_authority_for_compute(&self, compute_id: &str) -> Result<()> {
        self.validate_for_scope(compute_id, DSG_HTTP_ARRAY_AUTHORITY_SCOPE)?;
        self.request
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("bound request missing"))?
            .validate()
    }

    /// 仅在真实JWT验签/compute绑定之后使用；原字节不能经代理JSON归一化或改正文。
    /// Root独立执行同一签名和原文校验，网关通过不代表任何预算/数组已完成。
    pub fn authorize_array_request(&self, method: &str, path: &str, body: &[u8]) -> Result<()> {
        ensure!(
            self.scope == DSG_HTTP_ARRAY_AUTHORITY_SCOPE,
            "array authority scope required"
        );
        let binding = self
            .request
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("bound request missing"))?;
        binding.validate()?;
        ensure!(
            body.len() <= 16 * 1024
                && binding.method == method
                && binding.path == path
                && binding.body_sha256 == format!("{:x}", Sha256::digest(body)),
            "original bound request differs"
        );
        Ok(())
    }

    /// 新owner可查询原rearm，旧owner不能查询未来请求；独立只读入口不能
    /// 使用初始化token，也不能将更早epoch重写成当前epoch再执行setter。
    pub fn authorize_rearm_status(
        &self,
        request: &crate::dsg_admission_lease::DsgAdmissionLeaseRequest,
    ) -> Result<()> {
        request.validate()?;
        ensure!(
            self.scope == DSG_HTTP_SCOPE
                && request.action == crate::dsg_admission_lease::DsgAdmissionLeaseAction::Rearm
                && request.instance.vm_uid == self.vm_uid
                && request.instance.compute_instance_id == self.compute_instance_id
                && request.issuer_epoch <= self.issuer_epoch,
            "DSGA rearm status instance, action or epoch is not authorized"
        );
        Ok(())
    }

    /// 普通准入执行必须等于已验签的当前epoch；原意图只读对账允许更早epoch。
    /// boot由绑定实际PG的SDK再次核对；本方法不能把签名当作预算、租约或排空证明。
    pub fn authorize_admission(
        &self,
        request: &DsgAdmissionRequest,
        execution: bool,
    ) -> Result<()> {
        ensure!(self.scope == DSG_HTTP_SCOPE, "DSGA control scope required");
        request.validate()?;
        ensure!(
            request.instance.vm_uid == self.vm_uid
                && request.instance.compute_instance_id == self.compute_instance_id
                && if execution {
                    request.issuer_epoch == self.issuer_epoch
                } else {
                    request.issuer_epoch <= self.issuer_epoch
                },
            "DSGA admission instance or epoch is not authorized"
        );
        Ok(())
    }
    /// 仅在验签、时间、aud/scope及Compute绑定均由中间件核验之后调用。接管
    /// 必须等于签名中的当前epoch，不像历史查询允许更低epoch；数值本身不是租约。
    pub fn authorize_fence(&self, request: &DsgIssuerFenceRequest) -> Result<()> {
        ensure!(
            self.scope == DSG_HTTP_SCOPE,
            "DSGA execution scope required"
        );
        request.validate()?;
        ensure!(
            request.instance.vm_uid == self.vm_uid
                && request.instance.compute_instance_id == self.compute_instance_id
                && request.issuer_epoch == self.issuer_epoch,
            "DSGA fence instance or epoch is not authorized"
        );
        Ok(())
    }
    /// 只有验签成功后才可使用本校验；结构合法不表示签名或issuer租约有效。
    pub fn validate_for_compute(&self, compute_id: &str) -> Result<()> {
        self.validate_for_scope(compute_id, DSG_HTTP_SCOPE)
    }

    pub fn validate_initialization_for_compute(&self, compute_id: &str) -> Result<()> {
        self.validate_for_scope(compute_id, DSG_HTTP_INITIALIZATION_SCOPE)
    }

    fn validate_for_scope(&self, compute_id: &str, scope: &str) -> Result<()> {
        ensure!(self.aud == DSG_HTTP_AUDIENCE, "wrong DSGA audience");
        ensure!(self.scope == scope, "wrong DSGA scope");
        ensure!(
            matches!(
                scope,
                DSG_HTTP_ARRAY_AUTHORITY_SCOPE | DSG_HTTP_SERVICE_LOGIN_SCOPE
            ) || self.request.is_none(),
            "bound request cannot borrow legacy scope"
        );
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
        ensure!(self.scope == DSG_HTTP_SCOPE, "DSGA control scope required");
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

#[cfg(test)]
mod array_authority_tests {
    use super::*;

    fn claims() -> DsgHttpClaims {
        DsgHttpClaims {
            aud: DSG_HTTP_AUDIENCE.into(),
            scope: DSG_HTTP_ARRAY_AUTHORITY_SCOPE.into(),
            compute_id: "compute".into(),
            vm_uid: "vm".into(),
            compute_instance_id: "instance".into(),
            issuer_epoch: DsgUint64::new(1),
            nbf: 1,
            exp: 120,
            request: Some(DsgHttpRequestBinding {
                protocol_version: 1,
                method: "POST".into(),
                path: "/dsg/v1/resource/array-authorize".into(),
                body_sha256: format!("{:x}", Sha256::digest(b"original bytes")),
            }),
        }
    }
    #[test]
    fn bound_array_claims_separate_scope_and_exact_body() {
        let c = claims();
        c.validate_array_authority_for_compute("compute").unwrap();
        c.authorize_array_request(
            "POST",
            "/dsg/v1/resource/array-authorize",
            b"original bytes",
        )
        .unwrap();
        for (method, path, body) in [
            (
                "GET",
                "/dsg/v1/resource/array-authorize",
                b"original bytes".as_slice(),
            ),
            (
                "POST",
                "/dsg/v1/resource/array-observe",
                b"original bytes".as_slice(),
            ),
            (
                "POST",
                "/dsg/v1/resource/array-authorize",
                b"original bytes ".as_slice(),
            ),
        ] {
            assert!(c.authorize_array_request(method, path, body).is_err());
        }
        assert!(c.validate_for_compute("compute").is_err());
        let mut generic = c.clone();
        generic.scope = DSG_HTTP_SCOPE.into();
        assert!(generic.validate_for_compute("compute").is_err());
        generic.request = None;
        generic.validate_for_compute("compute").unwrap();
        assert!(
            generic
                .authorize_array_request(
                    "POST",
                    "/dsg/v1/resource/array-authorize",
                    b"original bytes"
                )
                .is_err()
        );
    }

    #[test]
    fn service_login_claims_bind_only_the_original_service_request() {
        let mut c = claims();
        c.scope = DSG_HTTP_SERVICE_LOGIN_SCOPE.into();
        c.request.as_mut().unwrap().path = DSG_SERVICE_LOGIN_PATH.into();
        c.validate_service_login_for_compute("compute").unwrap();
        c.authorize_service_login(b"original bytes").unwrap();
        assert!(c.authorize_service_login(b"original bytes ").is_err());
        assert!(c.validate_service_login_for_compute("other").is_err());
        assert!(c.validate_array_authority_for_compute("compute").is_err());
        assert!(c.validate_for_compute("compute").is_err());
        c.request.as_mut().unwrap().path = "/dsg/v1/resource/array-authorize".into();
        assert!(c.validate_service_login_for_compute("compute").is_err());
    }
    #[test]
    fn bound_array_claims_reject_invalid_binding_and_identity() {
        let good = claims();
        assert!(good.validate_array_authority_for_compute("other").is_err());
        for mode in [
            "version",
            "method",
            "path",
            "uppercase",
            "short",
            "zero",
            "missing",
        ] {
            let mut c = good.clone();
            if mode == "missing" {
                c.request = None;
            } else {
                let r = c.request.as_mut().unwrap();
                match mode {
                    "version" => r.protocol_version = 2,
                    "method" => r.method = "post".into(),
                    "path" => r.path = "/dsg/v1/resource/apply".into(),
                    "uppercase" => r.body_sha256 = r.body_sha256.to_uppercase(),
                    "zero" => r.body_sha256 = "0".repeat(64),
                    _ => r.body_sha256 = "a".into(),
                }
            }
            assert!(
                c.validate_array_authority_for_compute("compute").is_err(),
                "{mode}"
            );
        }
    }
    #[test]
    fn bound_array_claims_strict_json_and_legacy_wire() {
        let text = serde_json::to_string(&claims()).unwrap();
        for bad in [
            text.replace(
                "\"method\":\"POST\"",
                "\"method\":\"POST\",\"method\":\"POST\"",
            ),
            text.replace(
                "\"method\":\"POST\"",
                "\"method\":\"POST\",\"verified\":true",
            ),
        ] {
            assert!(serde_json::from_str::<DsgHttpClaims>(&bad).is_err());
        }
        let mut old = claims();
        old.scope = DSG_HTTP_SCOPE.into();
        old.request = None;
        let text = serde_json::to_string(&old).unwrap();
        assert!(!text.contains("request"));
        serde_json::from_str::<DsgHttpClaims>(&text)
            .unwrap()
            .validate_for_compute("compute")
            .unwrap();
    }
}
