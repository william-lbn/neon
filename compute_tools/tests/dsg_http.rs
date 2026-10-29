//! 真实生产Router的认证/输入门禁，以及显式ignored的原生HTTP→SQL→PG集成。
//! 原生用例只创建自己的0700临时PG；不需要Pageserver，不冒充存储/VM端到端。
#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, ensure};
use axum::{Router, body::Body};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use compute_api::{
    dsg_admission::{
        DsgAdmissionCloseRequest, DsgAdmissionClosure, DsgAdmissionDisposition,
        DsgAdmissionRequest, DsgAdmissionResponse,
    },
    dsg_control::{DsgOperationQuery, DsgResizeKind, DsgResizeRequest, DsgUint64},
    dsg_state::{DsgCapabilities, DsgControlResponse, DsgMemoryResponse},
    responses::ComputeStatus,
};
use compute_tools::http::server::Server;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, jwk::JwkSet};
use serde_json::{Value, json};
use tokio::time::{sleep, timeout};
use tokio_postgres::Client;
use tower::ServiceExt;

const LIMIT: Duration = Duration::from_secs(10);

#[path = "support/dsg_http.rs"]
mod support;
use support::{COMPUTE, Cluster, Signer, VM, native, node, router};

async fn invoke(
    router: Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> Result<(StatusCode, String)> {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = router
        .oneshot(request.body(Body::from(body.unwrap_or_default().to_owned()))?)
        .await?;
    let status = response.status();
    Ok((
        status,
        String::from_utf8(response.into_body().collect().await?.to_bytes().to_vec())?,
    ))
}

/// 本批只检查新增资源路由的正式认证/spec绑定。valid+query在连接固定UDS前
/// 拒绝，不依赖本机特权socket；实际UDS转发由同批独立单元测试覆盖。
#[tokio::test]
async fn resource_routes_require_control_scope_and_frozen_instance_before_uds() -> Result<()> {
    let signer = Signer::new();
    let app = router(&node(signer.jwks.clone(), true, None));
    for (method, path) in [
        (Method::GET, "/dsg/v1/resource/identity"),
        (Method::POST, "/dsg/v1/resource/fence"),
        (Method::POST, "/dsg/v1/resource/apply"),
        (Method::POST, "/dsg/v1/resource/observe"),
        (Method::POST, "/dsg/v1/resource/replay-original"),
        (Method::POST, "/dsg/v1/resource/repair-held"),
        (Method::POST, "/dsg/v1/resource/cancel-original"),
    ] {
        assert_eq!(
            invoke(app.clone(), method.clone(), path, None, None)
                .await?
                .0,
            StatusCode::UNAUTHORIZED
        );
        let mut claims = signer.claims();
        claims["scope"] = json!("compute_ctl:dsga_initialization");
        assert_eq!(
            invoke(
                app.clone(),
                method.clone(),
                path,
                Some(&signer.token(&claims)),
                None
            )
            .await?
            .0,
            StatusCode::UNAUTHORIZED
        );
        claims = signer.claims();
        claims["vm_uid"] = json!("foreign-vm");
        assert_eq!(
            invoke(
                app.clone(),
                method.clone(),
                path,
                Some(&signer.token(&claims)),
                None
            )
            .await?
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            invoke(
                app.clone(),
                method,
                &format!("{path}?override=1"),
                Some(&signer.token(&signer.claims())),
                None
            )
            .await?
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    Ok(())
}

#[tokio::test]
async fn process_birth_route_requires_control_scope_before_root_query() -> Result<()> {
    let signer = Signer::new();
    let app = router(&node(signer.jwks.clone(), true, None));
    let path = "/dsg/v1/resource/process";
    assert_eq!(
        invoke(app.clone(), Method::GET, path, None, None).await?.0,
        StatusCode::UNAUTHORIZED
    );
    let mut claims = signer.claims();
    claims["scope"] = json!("compute_ctl:dsga_initialization");
    assert_eq!(
        invoke(
            app.clone(),
            Method::GET,
            path,
            Some(&signer.token(&claims)),
            None
        )
        .await?
        .0,
        StatusCode::UNAUTHORIZED
    );
    claims = signer.claims();
    claims["vm_uid"] = json!("foreign-vm");
    assert_eq!(
        invoke(
            app.clone(),
            Method::GET,
            path,
            Some(&signer.token(&claims)),
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        invoke(
            app,
            Method::GET,
            &format!("{path}?override=1"),
            Some(&signer.token(&signer.claims())),
            None
        )
        .await?
        .0,
        StatusCode::BAD_REQUEST
    );
    Ok(())
}

#[tokio::test]
async fn resource_state_route_has_separate_control_scope() -> Result<()> {
    let signer = Signer::new();
    let app = router(&node(signer.jwks.clone(), true, None));
    let path = "/dsg/v1/resource/state";
    assert_eq!(
        invoke(app.clone(), Method::GET, path, None, None).await?.0,
        StatusCode::UNAUTHORIZED
    );
    let mut claims = signer.claims();
    claims["scope"] = json!("compute_ctl:dsga_initialization");
    assert_eq!(
        invoke(
            app.clone(),
            Method::GET,
            path,
            Some(&signer.token(&claims)),
            None
        )
        .await?
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        invoke(
            app,
            Method::GET,
            &format!("{path}?override=1"),
            Some(&signer.token(&signer.claims())),
            None
        )
        .await?
        .0,
        StatusCode::BAD_REQUEST
    );
    Ok(())
}

#[tokio::test]
async fn initialization_route_requires_dedicated_auth_and_published_native_proof() -> Result<()> {
    let signer = Signer::new();
    let app = router(&node(signer.jwks.clone(), true, None));
    assert_eq!(
        invoke(
            app.clone(),
            Method::GET,
            "/dsg/v1/initialization",
            None,
            None
        )
        .await?
        .0,
        StatusCode::UNAUTHORIZED
    );
    let mut creation_claims = signer.claims();
    creation_claims["scope"] = json!(compute_api::dsg_auth::DSG_HTTP_INITIALIZATION_SCOPE);
    let creation_token = signer.token(&creation_claims);
    assert_eq!(
        invoke(
            app.clone(),
            Method::GET,
            "/dsg/v1/initialization",
            Some(&creation_token),
            None
        )
        .await?
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    for (method, path, body) in [
        (Method::GET, "/dsg/v1/memory", None),
        (Method::POST, "/dsg/v1/resize", Some("{}")),
        (Method::POST, "/dsg/v1/issuer/fence", Some("{}")),
    ] {
        assert_eq!(
            invoke(app.clone(), method, path, Some(&creation_token), body)
                .await?
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let token = signer.token(&signer.claims());
    // 合法签名和伪装的Running仍不够：没有真实成功加载的认证策略，不能补发回执。
    let (code, body) = invoke(
        app.clone(),
        Method::GET,
        "/dsg/v1/initialization",
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        serde_json::from_str::<Value>(&body)?["state"],
        "initialization_unconfirmed"
    );
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/initialization",
            Some(&token),
            Some("{}")
        )
        .await?
        .0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    let mut wrong = signer.claims();
    wrong["vm_uid"] = json!("replacement");
    assert_eq!(
        invoke(
            app,
            Method::GET,
            "/dsg/v1/initialization",
            Some(&signer.token(&wrong)),
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    Ok(())
}

#[tokio::test]
async fn missing_legacy_and_wrong_signature_never_gain_hadron_bypass() -> Result<()> {
    let signer = Signer::new();
    let app = router(&node(signer.jwks.clone(), false, None));
    assert_eq!(
        invoke(app.clone(), Method::GET, "/status", None, None)
            .await?
            .0,
        StatusCode::OK
    );
    assert_eq!(
        invoke(app.clone(), Method::GET, "/dsg/v1/capabilities", None, None)
            .await?
            .0,
        StatusCode::UNAUTHORIZED
    );
    let mut legacy = signer.claims();
    legacy["scope"] = json!("compute_ctl:admin");
    legacy["aud"] = json!(["compute"]);
    let other = Signer::new();
    for token in [
        "garbage".into(),
        signer.token(&legacy),
        other.token(&signer.claims()),
    ] {
        assert_eq!(
            invoke(
                app.clone(),
                Method::GET,
                "/dsg/v1/capabilities",
                Some(&token),
                None
            )
            .await?
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        invoke(
            app,
            Method::GET,
            "/dsg/v1/capabilities",
            Some(&signer.token(&signer.claims())),
            None
        )
        .await?
        .0,
        StatusCode::PRECONDITION_FAILED
    );
    let request = Request::builder()
        .uri("/dsg/v1/memory")
        .header(
            "authorization",
            format!("Bearer {}", signer.token(&signer.claims())),
        )
        .header(
            "authorization",
            format!("Bearer {}", signer.token(&signer.claims())),
        )
        .body(Body::empty())?;
    let response = router(&node(signer.jwks.clone(), false, None))
        .oneshot(request)
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["cache-control"], "no-store");
    Ok(())
}

#[tokio::test]
async fn signed_claims_require_strict_scope_time_audience_compute_and_structure() -> Result<()> {
    let signer = Signer::new();
    let app = router(&node(signer.jwks.clone(), false, None));
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    for (field, value) in [
        ("aud", json!("compute")),
        ("scope", json!("compute_ctl:admin")),
        ("compute_id", json!("other")),
        ("issuer_epoch", json!(0)),
        ("issuer_epoch", json!("0")),
        ("nbf", json!(now + 90)),
        ("exp", json!(now - 10)),
        ("exp", json!(now + 999)),
        ("vm_uid", json!("bad id")),
        ("unexpected", json!(true)),
    ] {
        let mut claims = signer.claims();
        claims[field] = value;
        assert_eq!(
            invoke(
                app.clone(),
                Method::GET,
                "/dsg/v1/memory",
                Some(&signer.token(&claims)),
                None
            )
            .await?
            .0,
            StatusCode::UNAUTHORIZED,
            "{field}"
        );
    }
    for field in ["exp", "nbf", "aud", "compute_id", "issuer_epoch"] {
        let mut claims = signer.claims();
        claims.as_object_mut().unwrap().remove(field);
        assert_eq!(
            invoke(
                app.clone(),
                Method::GET,
                "/dsg/v1/memory",
                Some(&signer.token(&claims)),
                None
            )
            .await?
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let body = serde_json::to_string(&signer.claims())?;
    let duplicate = body.replacen('{', "{\"issuer_epoch\":\"1\",", 1);
    assert_eq!(
        invoke(
            app,
            Method::GET,
            "/dsg/v1/memory",
            Some(&signer.raw_token(&duplicate)),
            None
        )
        .await?
        .0,
        StatusCode::UNAUTHORIZED
    );
    Ok(())
}

#[tokio::test]
async fn key_selection_is_unique_algorithm_bound_and_fail_closed() -> Result<()> {
    let signer = Signer::new();
    let token = signer.token(&signer.claims());
    let mut duplicate = signer.jwks.clone();
    duplicate.keys.push(duplicate.keys[0].clone());
    let mut no_alg = signer.jwks.clone();
    no_alg.keys[0].common.key_algorithm = None;
    let mut encrypt = signer.jwks.clone();
    encrypt.keys[0].common.public_key_use = Some(jsonwebtoken::jwk::PublicKeyUse::Encryption);
    let mut sign_only = signer.jwks.clone();
    sign_only.keys[0].common.key_operations = Some(vec![jsonwebtoken::jwk::KeyOperations::Sign]);
    for jwks in [
        JwkSet { keys: vec![] },
        duplicate,
        no_alg,
        encrypt,
        sign_only,
    ] {
        assert_eq!(
            invoke(
                router(&node(jwks, false, None)),
                Method::GET,
                "/dsg/v1/memory",
                Some(&token),
                None
            )
            .await?
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let app = router(&node(signer.jwks.clone(), false, None));
    let mut header = Header::new(Algorithm::EdDSA);
    for kid in [None, Some("unknown".into())] {
        header.kid = kid;
        let token = encode(&header, &signer.claims(), &signer.encoding)?;
        assert_eq!(
            invoke(
                app.clone(),
                Method::GET,
                "/dsg/v1/memory",
                Some(&token),
                None
            )
            .await?
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    header = Header::new(Algorithm::HS256);
    header.kid = Some("dsg-test".into());
    let wrong_alg = encode(
        &header,
        &signer.claims(),
        &EncodingKey::from_secret(b"not-a-dsga-key"),
    )?;
    assert_eq!(
        invoke(app, Method::GET, "/dsg/v1/memory", Some(&wrong_alg), None)
            .await?
            .0,
        StatusCode::UNAUTHORIZED
    );
    Ok(())
}

#[tokio::test]
async fn rsa_and_mixed_jwks_use_only_the_matching_declared_algorithm() -> Result<()> {
    // 仅测试时用宿主openssl生成一次性RSA密钥，在内存管道传递，不写仓库/日志。
    // 覆盖Hadron常用RS256与EdDSA混合轮换，不能只测试EdDSA后声称两种均可用。
    let generated = Command::new("openssl")
        .args([
            "genpkey",
            "-algorithm",
            "RSA",
            "-pkeyopt",
            "rsa_keygen_bits:2048",
            "-pkeyopt",
            "rsa_keygen_pubexp:65537",
        ])
        .output()?;
    ensure!(
        generated.status.success(),
        "ephemeral test RSA key generation failed"
    );
    let mut modulus = Command::new("openssl")
        .args(["rsa", "-modulus", "-noout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    modulus.stdin.take().unwrap().write_all(&generated.stdout)?;
    let modulus = modulus.wait_with_output()?;
    ensure!(
        modulus.status.success(),
        "ephemeral test RSA modulus extraction failed"
    );
    let hex = std::str::from_utf8(&modulus.stdout)?
        .trim()
        .strip_prefix("Modulus=")
        .ok_or_else(|| anyhow::anyhow!("test RSA modulus is missing"))?;
    ensure!(hex.len() == 512, "test RSA modulus must be 2048 bits");
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<Result<_, _>>()?;
    let signer = Signer::new();
    let mut jwks = signer.jwks.clone();
    jwks.keys.push(serde_json::from_value(
        json!({"kty":"RSA","alg":"RS256","kid":"rsa-test",
        "key_ops":["verify"],"n":URL_SAFE_NO_PAD.encode(bytes),"e":"AQAB"}),
    )?);
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("rsa-test".into());
    let token = encode(
        &header,
        &signer.claims(),
        &EncodingKey::from_rsa_pem(&generated.stdout)?,
    )?;
    let app = router(&node(jwks, false, None));
    for token in [token, signer.token(&signer.claims())] {
        // 无DSGA配置故返回412，证明已经通过同一真实Router的签名/claims验证。
        assert_eq!(
            invoke(
                app.clone(),
                Method::GET,
                "/dsg/v1/memory",
                Some(&token),
                None
            )
            .await?
            .0,
            StatusCode::PRECONDITION_FAILED
        );
    }
    Ok(())
}

#[tokio::test]
async fn body_limits_duplicates_types_and_unknown_fields_are_rejected_before_sql() -> Result<()> {
    let signer = Signer::new();
    let app = router(&node(signer.jwks.clone(), true, None));
    let token = signer.token(&signer.claims());
    let body = include_str!("../../libs/compute_api/testdata/dsg_request_v1.json");
    for malformed in [
        body.replacen('{', "{\"protocol_version\":1,", 1),
        body.replacen('{', "{\"unknown\":1,", 1),
        body.replace("\"9007199254740993\"", "9007199254740993"),
    ] {
        assert_eq!(
            invoke(
                app.clone(),
                Method::POST,
                "/dsg/v1/resize",
                Some(&token),
                Some(&malformed)
            )
            .await?
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/resize",
            Some(&token),
            Some(&" ".repeat(16385))
        )
        .await?
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let request = Request::builder()
        .method(Method::POST)
        .uri("/dsg/v1/resize")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body))?;
    assert_eq!(
        app.oneshot(request).await?.status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    Ok(())
}

#[tokio::test]
async fn credential_and_request_instance_epoch_are_bound_before_sql() -> Result<()> {
    let signer = Signer::new();
    let app = router(&node(signer.jwks.clone(), true, None));
    let token = signer.token(&signer.claims());
    let mut request: DsgResizeRequest = serde_json::from_str(include_str!(
        "../../libs/compute_api/testdata/dsg_request_v1.json"
    ))?;
    request.key.instance.vm_uid = VM.into();
    request.key.instance.compute_instance_id = COMPUTE.into();
    for epoch in [0, 2] {
        request.key.issuer_epoch = DsgUint64::new(epoch);
        let expected = if epoch == 0 {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::FORBIDDEN
        };
        assert_eq!(
            invoke(
                app.clone(),
                Method::POST,
                "/dsg/v1/resize",
                Some(&token),
                Some(&serde_json::to_string(&request)?)
            )
            .await?
            .0,
            expected
        );
    }
    let query = DsgOperationQuery {
        protocol_version: 1,
        key: request.key,
    };
    for path in ["/dsg/v1/resize/status", "/dsg/v1/resize/cancel"] {
        assert_eq!(
            invoke(
                app.clone(),
                Method::POST,
                path,
                Some(&token),
                Some(&serde_json::to_string(&query)?)
            )
            .await?
            .0,
            StatusCode::FORBIDDEN
        );
    }
    let mut claims = signer.claims();
    claims["compute_instance_id"] = json!("other-instance");
    assert_eq!(
        invoke(
            app,
            Method::GET,
            "/dsg/v1/memory",
            Some(&signer.token(&claims)),
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    Ok(())
}

#[tokio::test]
async fn internal_server_never_exposes_dsg_routes() -> Result<()> {
    let signer = Signer::new();
    let compute = node(signer.jwks.clone(), true, None);
    let internal = Router::from(&Server::Internal { port: 0 }).with_state(compute);
    assert_eq!(
        invoke(
            internal.clone(),
            Method::GET,
            "/dsg/v1/capabilities",
            Some(&signer.token(&signer.claims())),
            None
        )
        .await?
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        invoke(
            internal,
            Method::POST,
            "/dsg/v1/issuer/fence",
            Some(&signer.token(&signer.claims())),
            Some("{}")
        )
        .await?
        .0,
        StatusCode::NOT_FOUND
    );
    Ok(())
}

#[tokio::test]
async fn issuer_fence_route_binds_signed_epoch_before_sql() -> Result<()> {
    let signer = Signer::new();
    let app = router(&node(signer.jwks.clone(), true, None));
    let token = signer.token(&signer.claims());
    let payload = json!({"protocol_version":1,"instance":{"vm_uid":VM,"compute_instance_id":COMPUTE,"pg_boot_id":"1"},"issuer_epoch":"2"});
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/issuer/fence",
            Some(&token),
            Some(&payload.to_string())
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/issuer/fence",
            None,
            Some(&payload.to_string())
        )
        .await?
        .0,
        StatusCode::UNAUTHORIZED
    );
    let duplicate = payload
        .to_string()
        .replacen('{', "{\"issuer_epoch\":\"1\",", 1);
    assert_eq!(
        invoke(
            app,
            Method::POST,
            "/dsg/v1/issuer/fence",
            Some(&token),
            Some(&duplicate)
        )
        .await?
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    Ok(())
}

#[tokio::test]
async fn managed_configure_rejects_storage_rebinding_before_state_change() -> Result<()> {
    let signer = Signer::new();
    let compute = node(signer.jwks.clone(), true, None);
    let original = compute
        .state
        .lock()
        .unwrap()
        .pspec
        .as_ref()
        .unwrap()
        .spec
        .clone();
    let mut changed = original.clone();
    changed.timeline_id = Some("ffffffffffffffffffffffffffffffff".parse()?);
    changed.operation_uuid = Some("storage-rebind-must-fail".into());
    // 此夹具的Hadron一般配置路由免认证，DSGA管理路由仍独立验签；本项只验证
    // 合法解析后的重配在修改state之前拒绝改绑，不启动PG/后台配置线程。
    let (code, body) = invoke(
        router(&compute),
        Method::POST,
        "/configure",
        None,
        Some(&serde_json::to_string(
            &json!({"spec":changed,"compute_ctl_config":compute.compute_ctl_config}),
        )?),
    )
    .await?;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    let state = compute.state.lock().unwrap();
    assert_eq!(state.status, ComputeStatus::Running);
    assert_eq!(
        serde_json::to_value(&state.pspec.as_ref().unwrap().spec)?,
        serde_json::to_value(original)?
    );
    Ok(())
}

#[tokio::test]
async fn admission_routes_require_execution_scope_and_strict_original_intents() -> Result<()> {
    let signer = Signer::new();
    let app = router(&node(signer.jwks.clone(), true, None));
    let token = signer.token(&signer.claims());
    let mut creation = signer.claims();
    creation["scope"] = json!(compute_api::dsg_auth::DSG_HTTP_INITIALIZATION_SCOPE);
    for path in [
        "/dsg/v1/admission",
        "/dsg/v1/admission/status",
        "/dsg/v1/admission/close-original",
        "/dsg/v1/admission/close-original/status",
        "/dsg/v1/admission/continue-close",
        "/dsg/v1/admission/continue-close/status",
    ] {
        for credential in [None, Some(signer.token(&creation))] {
            assert_eq!(
                invoke(
                    app.clone(),
                    Method::POST,
                    path,
                    credential.as_deref(),
                    Some("{}")
                )
                .await?
                .0,
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            invoke(
                app.clone(),
                Method::POST,
                path,
                Some(&token),
                Some("{\"protocol_version\":1,\"protocol_version\":1}")
            )
            .await?
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let future = json!({"protocol_version":1,"instance":{"vm_uid":VM,"compute_instance_id":COMPUTE,"pg_boot_id":"1"},
        "issuer_epoch":"2","revision":"1","expected_capacity_generation":"1","open":true,"budget_commitment_id":"budget"});
    for path in ["/dsg/v1/admission", "/dsg/v1/admission/status"] {
        assert_eq!(
            invoke(
                app.clone(),
                Method::POST,
                path,
                Some(&token),
                Some(&future.to_string())
            )
            .await?
            .0,
            StatusCode::FORBIDDEN
        );
    }
    let body = json!({"protocol_version":1,"original":future,"close":{}});
    assert_eq!(
        invoke(
            app,
            Method::POST,
            "/dsg/v1/admission/close-original",
            Some(&token),
            Some(&body.to_string())
        )
        .await?
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn native_admission_http_original_receipts_and_fenced_closure() -> Result<()> {
    let signer = Signer::new();
    let (mut cluster, _compute, app) = native(&signer).await?;
    cluster
        .connect()
        .await?
        .batch_execute("ALTER EXTENSION neon UPDATE TO '1.10'")
        .await?;
    let mut claims = signer.claims();
    claims["issuer_epoch"] = json!("2");
    let token = signer.token(&claims);
    let observed = memory(&app, &token).await?.observed;
    let fence = json!({"protocol_version":1,"instance":observed.instance,"issuer_epoch":"2"});
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/issuer/fence",
            Some(&token),
            Some(&fence.to_string())
        )
        .await?
        .0,
        StatusCode::OK
    );
    let request = DsgAdmissionRequest {
        protocol_version: 1,
        instance: observed.instance,
        issuer_epoch: DsgUint64::new(2),
        revision: DsgUint64::new(1),
        expected_capacity_generation: observed.capacity_generation,
        open: true,
        budget_commitment_id: "http-budget-1".into(),
    };
    let raw = serde_json::to_string(&request)?;
    let (code, body) = invoke(
        app.clone(),
        Method::POST,
        "/dsg/v1/admission/status",
        Some(&token),
        Some(&raw),
    )
    .await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let fact: DsgAdmissionResponse = serde_json::from_str(&body)?;
    assert_eq!(fact.disposition, DsgAdmissionDisposition::NotFound);
    // 共享兼容夹具未启用启动强制关闭GUC；查询必须保持其实际门禁，不能假造
    // 生产初始化状态。真实强制关闭启动已由原生启动门禁单独验证。
    assert_eq!(fact.observed.admission_open, observed.admission_open);
    let (code, body) = invoke(
        app.clone(),
        Method::POST,
        "/dsg/v1/admission",
        Some(&token),
        Some(&raw),
    )
    .await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let fact: DsgAdmissionResponse = serde_json::from_str(&body)?;
    fact.validate_for(&request, &request.instance, false)?;
    assert!(fact.observed.admission_open);
    // 模拟原发送不确定后外层重放了另一摘要：冲突必须保留未知，不能自动清空。
    let original = DsgAdmissionRequest {
        budget_commitment_id: "http-original-unknown".into(),
        ..request.clone()
    };
    let (code, body) = invoke(
        app.clone(),
        Method::POST,
        "/dsg/v1/admission",
        Some(&token),
        Some(&serde_json::to_string(&original)?),
    )
    .await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(
        serde_json::from_str::<DsgAdmissionResponse>(&body)?.disposition,
        DsgAdmissionDisposition::Conflict
    );
    let close = DsgAdmissionRequest {
        open: false,
        revision: DsgUint64::new(2),
        ..request.clone()
    };
    let closure = DsgAdmissionCloseRequest {
        protocol_version: 1,
        original: original.clone(),
        close: close.clone(),
    };
    let closure_raw = serde_json::to_string(&closure)?;
    let (code, body) = invoke(
        app.clone(),
        Method::POST,
        "/dsg/v1/admission/close-original",
        Some(&token),
        Some(&closure_raw),
    )
    .await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let fact: DsgAdmissionClosure = serde_json::from_str(&body)?;
    assert_eq!(fact.protocol_version, 1);
    assert!(!fact.vm_shrink_permit);
    assert!(fact.prior_may_have_applied);
    assert_eq!(fact.original, original);
    fact.closed_by
        .validate_for(&close, &close.instance, false)?;
    assert!(!fact.closed_by.observed.admission_open);
    claims["issuer_epoch"] = json!("3");
    let new_token = signer.token(&claims);
    let fence = json!({"protocol_version":1,"instance":request.instance,"issuer_epoch":"3"});
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/issuer/fence",
            Some(&new_token),
            Some(&fence.to_string())
        )
        .await?
        .0,
        StatusCode::OK
    );
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/admission/close-original",
            Some(&new_token),
            Some(&closure_raw)
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    let (code, body) = invoke(
        app,
        Method::POST,
        "/dsg/v1/admission/close-original/status",
        Some(&new_token),
        Some(&closure_raw),
    )
    .await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let fact: DsgAdmissionClosure = serde_json::from_str(&body)?;
    fact.closed_by.validate_for(&close, &close.instance, true)?;
    assert_eq!(
        fact.closed_by.observed.issuer_floor_epoch,
        DsgUint64::new(3)
    );
    assert!(fact.prior_may_have_applied);
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn native_admission_continuation_http_checks_owner_and_retained_predecessor() -> Result<()> {
    use compute_api::dsg_admission::{
        DsgAdmissionContinuationRequest, DsgAdmissionContinuationResponse,
    };
    let signer = Signer::new();
    let (mut cluster, _compute, app) = native(&signer).await?;
    cluster
        .connect()
        .await?
        .batch_execute("ALTER EXTENSION neon UPDATE TO '1.10'")
        .await?;
    let mut claims = signer.claims();
    let token = signer.token(&claims);
    let observed = memory(&app, &token).await?.observed;
    let original = DsgAdmissionRequest {
        protocol_version: 1,
        instance: observed.instance,
        issuer_epoch: DsgUint64::new(1),
        revision: DsgUint64::new(1),
        expected_capacity_generation: observed.capacity_generation,
        open: false,
        budget_commitment_id: "http-continuation-budget".into(),
    };
    let mut first = DsgAdmissionContinuationRequest {
        protocol_version: 1,
        original: original.clone(),
        previous_close: original.clone(),
        close: original,
    };
    first.close.issuer_epoch = DsgUint64::new(2);
    let first_raw = serde_json::to_string(&first)?;
    let path = "/dsg/v1/admission/continue-close";
    // 旧签名不能派发新epoch；未见查询只有503，不能借默认closed凑成功。
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            path,
            Some(&token),
            Some(&first_raw)
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    claims["issuer_epoch"] = json!("2");
    let token = signer.token(&claims);
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/admission/continue-close/status",
            Some(&token),
            Some(&first_raw)
        )
        .await?
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let (code, body) = invoke(
        app.clone(),
        Method::POST,
        path,
        Some(&token),
        Some(&first_raw),
    )
    .await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let proof: DsgAdmissionContinuationResponse = serde_json::from_str(&body)?;
    proof.validate_for(&first, &first.close.instance, false)?;
    assert_eq!(proof.closed_by.observed.issuer_floor_epoch.get(), 2);
    // 已应用前驱再选相同revision会得到真实Conflict并保留完整选择；旧请求只读
    // 即使仍有回执也不能清掉新未知。下一owner显式保留该前驱、选择next revision。
    let mut second = first.clone();
    second.previous_close = first.close.clone();
    second.close.issuer_epoch = DsgUint64::new(3);
    claims["issuer_epoch"] = json!("3");
    let token = signer.token(&claims);
    let second_raw = serde_json::to_string(&second)?;
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            path,
            Some(&token),
            Some(&second_raw)
        )
        .await?
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_ne!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/admission/continue-close/status",
            Some(&token),
            Some(&first_raw)
        )
        .await?
        .0,
        StatusCode::OK
    );
    let mut final_close = second.clone();
    final_close.previous_close = second.close.clone();
    final_close.close.issuer_epoch = DsgUint64::new(4);
    final_close.close.revision = DsgUint64::new(2);
    claims["issuer_epoch"] = json!("4");
    let token = signer.token(&claims);
    let raw = serde_json::to_string(&final_close)?;
    let (code, body) = invoke(app.clone(), Method::POST, path, Some(&token), Some(&raw)).await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let proof: DsgAdmissionContinuationResponse = serde_json::from_str(&body)?;
    proof.validate_for(&final_close, &final_close.close.instance, false)?;
    claims["issuer_epoch"] = json!("2");
    let old_token = signer.token(&claims);
    let (code, body) = invoke(
        app.clone(),
        Method::POST,
        path,
        Some(&old_token),
        Some(&first_raw),
    )
    .await?;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    // 旧owner被原生fence明确拒绝且setter尚未发送，不得把共享SDK永久占住。
    let (code, body) = invoke(app.clone(), Method::POST, path, Some(&token), Some(&raw)).await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let (code, body) = invoke(
        app,
        Method::POST,
        "/dsg/v1/admission/continue-close/status",
        Some(&token),
        Some(&raw),
    )
    .await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    serde_json::from_str::<DsgAdmissionContinuationResponse>(&body)?.validate_for(
        &final_close,
        &final_close.close.instance,
        true,
    )?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn native_shared_cost_http_binds_current_pg_and_preserves_control_state() -> Result<()> {
    use compute_api::dsg_state::DsgSharedCostResponse;
    let signer = Signer::new();
    let (mut cluster, _compute, app) = native(&signer).await?;
    let token = signer.token(&signer.claims());
    let (code, _) = invoke(
        app.clone(),
        Method::GET,
        "/dsg/v1/budget/shared",
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(
        code,
        StatusCode::SERVICE_UNAVAILABLE,
        "1.8不能假报新成本能力"
    );
    cluster
        .connect()
        .await?
        .batch_execute("ALTER EXTENSION neon UPDATE TO '1.11'")
        .await?;
    let before = memory(&app, &token).await?.observed;
    let (code, body) = invoke(
        app.clone(),
        Method::GET,
        "/dsg/v1/budget/shared",
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let fact: DsgSharedCostResponse = serde_json::from_str(&body)?;
    fact.validate_for(&before.instance)?;
    assert_eq!(fact.observed, before);
    assert_eq!(
        fact.current_main_and_arrays_upper_bytes,
        before.startup_shared_upper_bytes
    );
    // 共用formal_function后还需核验1参数签名，避免只让新0参数查询通过。
    let request = DsgAdmissionRequest {
        protocol_version: 1,
        instance: before.instance,
        issuer_epoch: DsgUint64::new(1),
        revision: DsgUint64::new(1),
        expected_capacity_generation: before.capacity_generation,
        open: false,
        budget_commitment_id: "shared-cost-observation-only".into(),
    };
    let (code, body) = invoke(
        app.clone(),
        Method::POST,
        "/dsg/v1/admission/status",
        Some(&token),
        Some(&serde_json::to_string(&request)?),
    )
    .await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(
        serde_json::from_str::<DsgAdmissionResponse>(&body)?.disposition,
        DsgAdmissionDisposition::NotFound
    );
    let mut creation = signer.claims();
    creation["scope"] = json!(compute_api::dsg_auth::DSG_HTTP_INITIALIZATION_SCOPE);
    assert_eq!(
        invoke(
            app.clone(),
            Method::GET,
            "/dsg/v1/budget/shared",
            Some(&signer.token(&creation)),
            None
        )
        .await?
        .0,
        StatusCode::UNAUTHORIZED
    );
    let mut wrong = signer.claims();
    wrong["vm_uid"] = json!("replacement");
    assert_eq!(
        invoke(
            app,
            Method::GET,
            "/dsg/v1/budget/shared",
            Some(&signer.token(&wrong)),
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn native_work_state_http_requires_formal_upgrade_and_same_instance() -> Result<()> {
    use compute_api::dsg_work::DsgWorkResponse;
    let signer = Signer::new();
    let (mut cluster, _compute, app) = native(&signer).await?;
    let token = signer.token(&signer.claims());
    let path = "/dsg/v1/work/state";
    assert_eq!(
        invoke(app.clone(), Method::GET, path, Some(&token), None)
            .await?
            .0,
        StatusCode::SERVICE_UNAVAILABLE,
        "旧1.8不能假报新Native登记"
    );
    let sql = cluster.connect().await?;
    sql.batch_execute("ALTER EXTENSION neon UPDATE TO '1.12'")
        .await?;
    // SQL ACL被误授予后，Native入口仍同时检查当前与session SU。
    let namespace: String = sql.query_one("SELECT pg_catalog.quote_ident(n.nspname) FROM pg_catalog.pg_extension e JOIN pg_catalog.pg_namespace n ON n.oid=e.extnamespace WHERE e.extname='neon'", &[]).await?.get(0);
    sql.batch_execute(&format!("CREATE ROLE work_observer NOSUPERUSER; GRANT USAGE ON SCHEMA {namespace} TO work_observer; GRANT EXECUTE ON FUNCTION {namespace}.neon_dsg_work_state() TO work_observer")).await?;
    let denied = sql
        .batch_execute(&format!(
            "SET ROLE work_observer; SELECT {namespace}.neon_dsg_work_state()"
        ))
        .await;
    assert_eq!(
        denied.unwrap_err().code(),
        Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
    );
    sql.batch_execute("RESET ROLE").await?;
    let initial = memory(&app, &token).await?.observed;
    let fence = json!({"protocol_version":1,"instance":initial.instance,"issuer_epoch":"1"});
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/issuer/fence",
            Some(&token),
            Some(&fence.to_string())
        )
        .await?
        .0,
        StatusCode::OK
    );
    // 仅关闭真实Native门禁；测试引用不是完整资源预算，不能用于开门或VM许可。
    let close = DsgAdmissionRequest {
        protocol_version: 1,
        instance: initial.instance,
        issuer_epoch: DsgUint64::new(1),
        revision: DsgUint64::new(1),
        expected_capacity_generation: initial.capacity_generation,
        open: false,
        budget_commitment_id: "work-observation-fixture".into(),
    };
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/admission",
            Some(&token),
            Some(&serde_json::to_string(&close)?)
        )
        .await?
        .0,
        StatusCode::OK
    );
    let before = memory(&app, &token).await?.observed;
    let (status, body) = invoke(app.clone(), Method::GET, path, Some(&token), None).await?;
    assert_eq!(status, StatusCode::OK, "{body}");
    let fact: DsgWorkResponse = serde_json::from_str(&body)?;
    fact.validate_for(&before.instance)?;
    assert_eq!(fact.observed, before, "只读观察不能改floor/门禁/操作历史");
    assert!(fact.observer_excluded);
    assert!(
        fact.foreground_idle,
        "Simple Query观察不能被自身Parse/Bind隐式事务挡住"
    );
    assert!(!fact.vm_shrink_permit);
    // 只排除当前固定观察；另一个真实SU的长期事务仍必须阻止前台空闲。
    sql.batch_execute("BEGIN").await?;
    let (code, retained) = invoke(app.clone(), Method::GET, path, Some(&token), None).await?;
    assert_eq!(code, StatusCode::OK);
    let retained: DsgWorkResponse = serde_json::from_str(&retained)?;
    assert!(retained.observer_excluded && retained.retained_transactions >= 1);
    assert!(!retained.foreground_idle);
    sql.batch_execute("ROLLBACK").await?;

    let mut creation = signer.claims();
    creation["scope"] = json!(compute_api::dsg_auth::DSG_HTTP_INITIALIZATION_SCOPE);
    assert_eq!(
        invoke(
            app.clone(),
            Method::GET,
            path,
            Some(&signer.token(&creation)),
            None
        )
        .await?
        .0,
        StatusCode::UNAUTHORIZED
    );
    let mut wrong = signer.claims();
    wrong["vm_uid"] = json!("replacement");
    assert_eq!(
        invoke(
            app.clone(),
            Method::GET,
            path,
            Some(&signer.token(&wrong)),
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    // 正式降级移除函数后重新拒绝，不能仅缓存升级前/后的能力判断。
    sql.batch_execute("ALTER EXTENSION neon UPDATE TO '1.11'")
        .await?;
    assert_eq!(
        invoke(app, Method::GET, path, Some(&token), None).await?.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate requires isolated exact PG and installed Neon"]
async fn native_connection_state_requires_formal_upgrade_and_real_postmaster_ack() -> Result<()> {
    use compute_api::dsg_connection::DsgConnectionResponse;
    let signer = Signer::new();
    let (mut old, _, app) = native(&signer).await?;
    let token = signer.token(&signer.claims());
    let path = "/dsg/v1/connections/state";
    assert_eq!(
        invoke(app.clone(), Method::GET, path, Some(&token), None)
            .await?
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    old.passed = true;
    drop(app);
    drop(old);
    let (mut cluster, _, app) = support::native_connections(&signer).await?;
    let (code, body) = invoke(app.clone(), Method::GET, path, Some(&token), None).await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let fact: DsgConnectionResponse = serde_json::from_str(&body)?;
    fact.validate_for(&fact.work.observed.instance)?;
    assert!(
        fact.enabled
            && fact.acknowledged
            && fact.public_connections_blocked
            && fact.private_management_ready
    );
    assert!(!fact.requested_open && fact.work.foreground_idle && !fact.vm_shrink_permit);
    let mut wrong = signer.claims();
    wrong["vm_uid"] = json!("replacement");
    assert_eq!(
        invoke(
            app.clone(),
            Method::GET,
            path,
            Some(&signer.token(&wrong)),
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    let mut creation = signer.claims();
    creation["scope"] = json!(compute_api::dsg_auth::DSG_HTTP_INITIALIZATION_SCOPE);
    assert_eq!(
        invoke(
            app.clone(),
            Method::GET,
            path,
            Some(&signer.token(&creation)),
            None
        )
        .await?
        .0,
        StatusCode::UNAUTHORIZED
    );
    // 正式降级移除新函数，不能依靠缓存旧成功继续报ack。
    cluster
        .connect()
        .await?
        .batch_execute("ALTER EXTENSION neon UPDATE TO '1.12'")
        .await?;
    assert_eq!(
        invoke(app, Method::GET, path, Some(&token), None).await?.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate requires exact PG and installed Neon Prepared1"]
async fn native_prepared_state_requires_formal_member_and_keeps_retained_slot() -> Result<()> {
    use compute_api::dsg_prepared::DsgPreparedResponse;

    let signer = Signer::new();
    let token = signer.token(&signer.claims());
    let path = "/dsg/v1/work/prepared";
    let (mut old, _, old_app) = support::native_connections(&signer).await?;
    assert_eq!(
        invoke(old_app.clone(), Method::GET, path, Some(&token), None)
            .await?
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    old.passed = true;
    drop(old_app);
    drop(old);
    let (mut cluster, _, app) = support::native_prepared(&signer).await?;
    let (code, body) = invoke(app.clone(), Method::GET, path, Some(&token), None).await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let fact: DsgPreparedResponse = serde_json::from_str(&body)?;
    fact.validate_for(&fact.connections.work.observed.instance)?;
    assert!(
        fact.runtime_prepares_blocked
            && fact.connections.acknowledged
            && fact.connections.public_connections_blocked
            && fact.connections.work.foreground_idle
    );
    assert_eq!(fact.prepared_slots, 1, "真实保留事务不能由前台idle补零");
    assert_eq!(fact.maximum_prepared_slots, 4);
    assert!(!fact.vm_shrink_permit);
    let sql = cluster.connect().await?;
    let row = sql
        .query_one("SELECT gid FROM pg_prepared_xacts", &[])
        .await?;
    assert_eq!(row.get::<_, String>(0), "native_retained");
    // 完成由测试中明确的原协调者选择；观察接口不替业务决定提交或回滚。
    sql.simple_query("ROLLBACK PREPARED 'native_retained'")
        .await?;
    let (code, body) = invoke(app.clone(), Method::GET, path, Some(&token), None).await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let empty: DsgPreparedResponse = serde_json::from_str(&body)?;
    empty.validate_for(&fact.connections.work.observed.instance)?;
    assert_eq!(empty.prepared_slots, 0);
    assert!(empty.runtime_prepares_blocked && !empty.vm_shrink_permit);
    // 新函数仍须正式升级成员；降级不能由缓存的原生能力伪造成功。
    sql.simple_query("ALTER EXTENSION neon UPDATE TO '1.13'")
        .await?;
    assert_eq!(
        invoke(app.clone(), Method::GET, path, Some(&token), None)
            .await?
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    // 同点序列化公共helper被复用，旧Connection1直接路由的内容仍须可验证。
    let (code, body) = invoke(
        app,
        Method::GET,
        "/dsg/v1/connections/state",
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let connection: compute_api::dsg_connection::DsgConnectionResponse =
        serde_json::from_str(&body)?;
    connection.validate_for(&fact.connections.work.observed.instance)?;
    cluster.passed = true;
    Ok(())
}

// 真正的正式HTTP准入→普通应用事务→提交回调→正式同点观察，不能由body fixture
// 或pg_stat_activity空闲拼许可。只使用准确PG/已安装Neon及本次独占目录。
#[tokio::test]
#[ignore = "strict gate requires isolated exact PG and installed Neon"]
async fn native_transaction_finish_keeps_commit_callback_inflight_until_done() -> Result<()> {
    use compute_api::dsg_connection::DsgConnectionResponse;

    async fn observation(app: &Router, token: &str) -> Result<DsgConnectionResponse> {
        let (status, body) = invoke(
            app.clone(),
            Method::GET,
            "/dsg/v1/connections/state",
            Some(token),
            None,
        )
        .await?;
        ensure!(
            status == StatusCode::OK,
            "connection observation {status}: {body}"
        );
        let fact: DsgConnectionResponse = serde_json::from_str(&body)?;
        fact.validate_for(&fact.work.observed.instance)?;
        Ok(fact)
    }

    async fn admission(app: &Router, token: &str, request: &DsgAdmissionRequest) -> Result<()> {
        let raw = serde_json::to_string(request)?;
        let (status, body) = invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/admission",
            Some(token),
            Some(&raw),
        )
        .await?;
        ensure!(status == StatusCode::OK, "admission {status}: {body}");
        let fact: DsgAdmissionResponse = serde_json::from_str(&body)?;
        fact.validate_for(request, &request.instance, false)?;
        ensure!(
            fact.disposition == DsgAdmissionDisposition::Applied,
            "admission was not applied"
        );
        Ok(())
    }

    let signer = Signer::new();
    let (mut cluster, _, app) = support::native_connections(&signer).await?;
    let token = signer.token(&signer.claims());
    let native = memory(&app, &token).await?.observed;
    let fence = json!({"protocol_version":1,"instance":native.instance,"issuer_epoch":"1"});
    let (status, body) = invoke(
        app.clone(),
        Method::POST,
        "/dsg/v1/issuer/fence",
        Some(&token),
        Some(&fence.to_string()),
    )
    .await?;
    ensure!(status == StatusCode::OK, "fence {status}: {body}");
    let mut request = DsgAdmissionRequest {
        protocol_version: 1,
        instance: native.instance,
        issuer_epoch: DsgUint64::new(1),
        revision: DsgUint64::new(1),
        expected_capacity_generation: native.capacity_generation,
        open: true,
        budget_commitment_id: "isolated-native-transaction-fixture".into(),
    };
    // 这是测试管理者显式原意图，不把合成预算引用当真实Prepare/Finalize生产证书。
    admission(&app, &token, &request).await?;
    let admin = cluster.connect().await?;
    admin
        .batch_execute(
            "CREATE ROLE finish_app LOGIN; CREATE TABLE public.finish_http(i int); \
        GRANT INSERT ON public.finish_http TO finish_app; \
        CREATE FUNCTION public.finish_http_wait() RETURNS trigger LANGUAGE plpgsql AS $$BEGIN \
          PERFORM pg_advisory_xact_lock(1945013); RETURN NEW; END$$; \
        CREATE CONSTRAINT TRIGGER finish_http_wait AFTER INSERT ON public.finish_http \
          DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.finish_http_wait()",
        )
        .await?;
    admin
        .simple_query("SELECT pg_advisory_lock(1945013)")
        .await?;
    // 应用通过原公共socket连接；受管私有管理socket不能被用作业务旁路。
    let public = cluster.data.parent().unwrap().join("socket");
    let (client, connection) = tokio_postgres::Config::new()
        .host_path(public)
        .port(55432)
        .user("finish_app")
        .dbname("postgres")
        .connect(tokio_postgres::NoTls)
        .await?;
    let connection_task = tokio::spawn(connection);
    let client = std::sync::Arc::new(client);
    let pid: i32 = client
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    client
        .simple_query("BEGIN; INSERT INTO public.finish_http VALUES(42)")
        .await?;
    request.revision = DsgUint64::new(2);
    request.open = false;
    admission(&app, &token, &request).await?;
    let commit_client = client.clone();
    let commit = tokio::spawn(async move { commit_client.simple_query("COMMIT").await });
    timeout(LIMIT, async {
        loop {
            let waiting = admin.query_one("SELECT COALESCE(wait_event='advisory',false) FROM pg_stat_activity WHERE pid=$1", &[&pid]).await?.get::<_, bool>(0);
            let fact = observation(&app, &token).await?;
            if waiting && fact.acknowledged && fact.public_connections_blocked
                && fact.work.frontend_executes == 1 && fact.work.retained_transactions == 1 {
                ensure!(!fact.work.foreground_idle && !fact.vm_shrink_permit, "inflight commit became idle");
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
        Ok::<_, anyhow::Error>(())
    }).await??;
    admin
        .simple_query("SELECT pg_advisory_unlock(1945013)")
        .await?;
    timeout(LIMIT, commit).await???;
    timeout(LIMIT, async {
        loop {
            let fact = observation(&app, &token).await?;
            if fact.work.foreground_idle {
                ensure!(
                    fact.public_connections_blocked
                        && fact.work.retained_transactions == 0
                        && fact.work.frontend_executes == 0
                        && !fact.vm_shrink_permit,
                    "invalid completion fact"
                );
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let rows = admin
        .simple_query("SELECT i FROM public.finish_http")
        .await?;
    ensure!(rows.iter().any(|m| matches!(m, tokio_postgres::SimpleQueryMessage::Row(row) if row.get(0) == Some("42"))), "committed data missing");
    let error = client.simple_query("SELECT 42").await.unwrap_err();
    ensure!(
        error.code() == Some(&tokio_postgres::error::SqlState::OBJECT_NOT_IN_PREREQUISITE_STATE)
    );
    request.revision = DsgUint64::new(3);
    request.open = true;
    admission(&app, &token, &request).await?;
    client.simple_query("SELECT 42").await?;
    drop(client);
    timeout(LIMIT, connection_task).await???;
    cluster.passed = true;
    Ok(())
}

async fn memory(app: &Router, token: &str) -> Result<DsgMemoryResponse> {
    let (status, body) = invoke(
        app.clone(),
        Method::GET,
        "/dsg/v1/memory",
        Some(token),
        None,
    )
    .await?;
    ensure!(status == StatusCode::OK, "memory failed: {status} {body}");
    Ok(serde_json::from_str(&body)?)
}

fn resize_request(
    fact: &DsgMemoryResponse,
    revision: u64,
    target: u32,
    kind: DsgResizeKind,
) -> DsgResizeRequest {
    let mut request: DsgResizeRequest = serde_json::from_str(include_str!(
        "../../libs/compute_api/testdata/dsg_request_v1.json"
    ))
    .unwrap();
    request.key.instance = fact.observed.instance.clone();
    request.key.issuer_epoch = DsgUint64::new(1);
    request.key.revision = DsgUint64::new(revision);
    request.key.operation_id.low = DsgUint64::new(revision);
    request.expected_capacity_generation = fact.observed.capacity_generation;
    request.target_buffers = target;
    request.kind = kind;
    request.deadline_unix_ms = DsgUint64::new(u64::MAX);
    request
}

async fn control(
    app: &Router,
    token: &str,
    path: &str,
    request: &impl serde::Serialize,
) -> Result<DsgControlResponse> {
    let (status, body) = invoke(
        app.clone(),
        Method::POST,
        path,
        Some(token),
        Some(&serde_json::to_string(request)?),
    )
    .await?;
    ensure!(status == StatusCode::OK, "control failed: {status} {body}");
    let response: DsgControlResponse = serde_json::from_str(&body)?;
    ensure!(
        !response.vm_shrink_permit,
        "HTTP cannot grant VM permission"
    );
    Ok(response)
}

fn query(request: &DsgResizeRequest) -> DsgOperationQuery {
    DsgOperationQuery {
        protocol_version: 1,
        key: request.key.clone(),
    }
}

async fn prepare_guard(cluster: &Cluster) -> Result<(Client, Client)> {
    let guard = cluster.connect().await?;
    let control = cluster.connect().await?;
    guard
        .batch_execute(
            "SELECT public.dsg_control('attach_guard'); SELECT public.dsg_control('arm_pause')",
        )
        .await?;
    Ok((guard, control))
}

async fn wait_guard(control: &Client) -> Result<()> {
    timeout(LIMIT, async {
        loop {
            let state: String = control
                .query_one("SELECT public.dsg_control('prepare_waiting')", &[])
                .await?
                .get(0);
            if state == "yes" {
                return Ok::<_, anyhow::Error>(());
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    Ok(())
}

async fn wait_preparing(app: &Router, token: &str, request: &DsgResizeRequest) -> Result<()> {
    timeout(LIMIT, async {
        loop {
            let fact = control(app, token, "/dsg/v1/resize/status", &query(request)).await?;
            if fact
                .receipt
                .result
                .and_then(|s| s.operation)
                .is_some_and(|op| op.phase == compute_api::dsg_state::DsgPhase::Preparing)
            {
                return Ok::<_, anyhow::Error>(());
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    Ok(())
}

struct HttpTask(tokio::task::JoinHandle<()>);
impl Drop for HttpTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
#[ignore = "strict gate creates real isolated Neon PG and TCP HTTP server"]
async fn native_tcp_round_trip_history_and_data_integrity() -> Result<()> {
    let signer = Signer::new();
    let (mut cluster, _compute, app) = native(&signer).await?;
    let token = signer.token(&signer.claims());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let _server = HttpTask(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let http = reqwest::Client::builder().no_proxy().build()?;
    let cap = http
        .get(format!("http://{addr}/dsg/v1/capabilities"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json::<DsgCapabilities>()
        .await?;
    assert!(!cap.vm_shrink_permit && !cap.production_ready);
    let before = http
        .get(format!("http://{addr}/dsg/v1/memory"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json::<DsgMemoryResponse>()
        .await?;
    let first = resize_request(&before, 1, 1024, DsgResizeKind::Grow);
    let sql = cluster.connect().await?;
    sql.batch_execute("CREATE TABLE public.dsg_data(i int,pad text); ALTER TABLE public.dsg_data ALTER pad SET STORAGE PLAIN;\
        INSERT INTO public.dsg_data SELECT i,repeat(md5(i::text),150) FROM generate_series(1,600) i").await?;
    let checksum=sql.query_one("SELECT md5(string_agg(pad,'' ORDER BY i)),pg_relation_size('public.dsg_data') FROM public.dsg_data",&[]).await?;
    let expected: String = checksum.get(0);
    assert!(checksum.get::<_, i64>(1) > 2 * 1024 * 1024);
    let mut intent = first.clone();
    for (revision, target, kind) in [
        (1, 1024, DsgResizeKind::Grow),
        (2, 256, DsgResizeKind::Shrink),
        (3, 2048, DsgResizeKind::Grow),
    ] {
        if revision != 1 {
            let memory = http
                .get(format!("http://{addr}/dsg/v1/memory"))
                .bearer_auth(&token)
                .send()
                .await?
                .error_for_status()?
                .json::<DsgMemoryResponse>()
                .await?;
            intent = resize_request(&memory, revision, target, kind);
        }
        let fact = http
            .post(format!("http://{addr}/dsg/v1/resize"))
            .bearer_auth(&token)
            .json(&intent)
            .send()
            .await?
            .error_for_status()?
            .json::<DsgControlResponse>()
            .await?;
        fact.validate_resize(&intent, &before.observed.instance)?;
        assert_eq!(fact.observed.high_buffers, target);
        assert!(!fact.vm_shrink_permit);
    }
    let historical = http
        .post(format!("http://{addr}/dsg/v1/resize/status"))
        .bearer_auth(&token)
        .json(&query(&first))
        .send()
        .await?
        .error_for_status()?
        .json::<DsgControlResponse>()
        .await?;
    assert_eq!(historical.receipt.result.unwrap().high_buffers, 1024);
    assert_eq!(historical.observed.high_buffers, 2048);
    let actual: String = sql
        .query_one(
            "SELECT md5(string_agg(pad,'' ORDER BY i)) FROM public.dsg_data",
            &[],
        )
        .await?
        .get(0);
    assert_eq!(actual, expected);
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn native_dropped_handler_preserves_fence_and_independent_cancel() -> Result<()> {
    let signer = Signer::new();
    let (mut cluster, compute, app) = native(&signer).await?;
    let token = signer.token(&signer.claims());
    let initial = memory(&app, &token).await?;
    let original = resize_request(&initial, 1, 1024, DsgResizeKind::Grow);
    let (guard, controller) = prepare_guard(&cluster).await?;
    let held = async { guard.query_one("SELECT public.dsg_hold_guard()", &[]).await };
    tokio::pin!(held);
    tokio::select! {r=&mut held=>panic!("guard unexpectedly returned: {r:?}"),r=wait_guard(&controller)=>r?}
    let body = serde_json::to_string(&original)?;
    let mut execution = Box::pin(invoke(
        app.clone(),
        Method::POST,
        "/dsg/v1/resize",
        Some(&token),
        Some(&body),
    ));
    tokio::select! {r=&mut execution=>panic!("resize unexpectedly returned: {r:?}"),r=wait_preparing(&app,&token,&original)=>r?}
    // 显式销毁Box中的handler future，确认跨请求保留的是OnceCell里的原意图。
    drop(execution);
    let next = resize_request(&initial, 2, 2048, DsgResizeKind::Grow);
    let rejected = invoke(
        app.clone(),
        Method::POST,
        "/dsg/v1/resize",
        Some(&token),
        Some(&serde_json::to_string(&next)?),
    )
    .await?;
    assert_eq!(rejected.0, StatusCode::CONFLICT);
    compute.state.lock().unwrap().status = ComputeStatus::Configuration;
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/resize",
            Some(&token),
            Some(&serde_json::to_string(&next)?)
        )
        .await?
        .0,
        StatusCode::PRECONDITION_FAILED
    );
    // 重配期间仍须观察/取消已发操作，不能把生命周期状态当作动作已经撤销。
    memory(&app, &token).await?;
    let cancelled = control(&app, &token, "/dsg/v1/resize/cancel", &query(&original)).await?;
    assert_eq!(
        cancelled.receipt.disposition,
        compute_api::dsg_state::DsgDisposition::CancelRequested
    );
    controller
        .query_one("SELECT public.dsg_control('release_prepare')", &[])
        .await?;
    timeout(LIMIT, held).await??;
    compute.state.lock().unwrap().status = ComputeStatus::Running;
    let replay = control(&app, &token, "/dsg/v1/resize", &original).await?;
    assert_eq!(
        replay.receipt.result.unwrap().operation.unwrap().reason,
        compute_api::dsg_state::DsgReason::Cancelled
    );
    control(&app, &token, "/dsg/v1/resize", &next).await?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn native_backend_fatal_is_unconfirmed_and_does_not_create_new_execution_session()
-> Result<()> {
    let signer = Signer::new();
    let (mut cluster, _compute, app) = native(&signer).await?;
    let token = signer.token(&signer.claims());
    let initial = memory(&app, &token).await?;
    let original = resize_request(&initial, 1, 1024, DsgResizeKind::Grow);
    let (guard, controller) = prepare_guard(&cluster).await?;
    let held = async { guard.query_one("SELECT public.dsg_hold_guard()", &[]).await };
    tokio::pin!(held);
    tokio::select! {r=&mut held=>panic!("guard unexpectedly returned: {r:?}"),r=wait_guard(&controller)=>r?}
    let killer = async {
        wait_preparing(&app, &token, &original).await?;
        let row=controller.query_one("SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
            WHERE application_name='compute_ctl:dsga' AND state='active' AND query LIKE '%neon_dsg_request_resize%'",&[]).await?;
        assert!(row.get::<_, bool>(0));
        Ok::<_, anyhow::Error>(())
    };
    let body = serde_json::to_string(&original)?;
    let (result, killed) = tokio::join!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/resize",
            Some(&token),
            Some(&body)
        ),
        killer
    );
    killed?;
    let (status, body) = result?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let error: Value = serde_json::from_str(&body)?;
    assert_eq!(error["state"], "unconfirmed");
    assert_eq!(error["key"], serde_json::to_value(&original.key)?);
    let next = resize_request(&initial, 2, 2048, DsgResizeKind::Grow);
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/resize",
            Some(&token),
            Some(&serde_json::to_string(&next)?)
        )
        .await?
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        invoke(
            app.clone(),
            Method::POST,
            "/dsg/v1/resize",
            Some(&token),
            Some(&serde_json::to_string(&original)?)
        )
        .await?
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    // 观察连接存活，但终态不擅自重置已死执行连接的原意图/身份。
    memory(&app, &token).await?;
    controller
        .query_one("SELECT public.dsg_control('release_prepare')", &[])
        .await?;
    timeout(LIMIT, held).await??;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn native_restart_rejects_cached_discovery_and_stale_intent_cannot_bypass_fence() -> Result<()>
{
    let signer = Signer::new();
    let (mut cluster, _compute, app) = native(&signer).await?;
    let token = signer.token(&signer.claims());
    let before = memory(&app, &token).await?;
    let old = resize_request(&before, 1, 1024, DsgResizeKind::Grow);
    control(&app, &token, "/dsg/v1/resize", &old).await?;
    cluster.restart()?;
    assert_eq!(
        invoke(app, Method::GET, "/dsg/v1/capabilities", Some(&token), None)
            .await?
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let replacement = node(signer.jwks.clone(), true, Some(&cluster));
    let replacement = router(&replacement);
    let current = memory(&replacement, &token).await?;
    assert_ne!(
        before.observed.instance.pg_boot_id,
        current.observed.instance.pg_boot_id
    );
    let stale = control(&replacement, &token, "/dsg/v1/resize", &old).await?;
    assert_eq!(
        stale.receipt.disposition,
        compute_api::dsg_state::DsgDisposition::Stale
    );
    let next = resize_request(&current, 2, 2048, DsgResizeKind::Grow);
    assert_eq!(
        invoke(
            replacement,
            Method::POST,
            "/dsg/v1/resize",
            Some(&token),
            Some(&serde_json::to_string(&next)?)
        )
        .await?
        .0,
        StatusCode::CONFLICT
    );
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "requires exact PG17; verifies frozen identity through actual Router and SQL"]
async fn native_bad_config_keeps_old_observation_and_blocks_new_resize() -> Result<()> {
    let signer = Signer::new();
    let (mut cluster, compute, app) = native(&signer).await?;
    let token = signer.token(&signer.claims());
    let initial = memory(&app, &token).await?;
    let original = resize_request(&initial, 1, 512, DsgResizeKind::Grow);
    let first = control(&app, &token, "/dsg/v1/resize", &original).await?;
    assert_eq!(first.receipt.key, original.key);
    // 模拟已被拒绝configure留下的错误配置。旧观察/取消必须仍按本Postmaster
    // 原身份认证，不能因为mutable spec指向新VM而把原操作事实藏起来。
    {
        let mut state = compute.state.lock().unwrap();
        state
            .pspec
            .as_mut()
            .unwrap()
            .spec
            .dynamic_shared_buffers
            .as_mut()
            .unwrap()
            .vm_uid = "recreated-config-vm".into();
        state.status = ComputeStatus::Failed;
    }
    let old = control(&app, &token, "/dsg/v1/resize/status", &query(&original)).await?;
    assert_eq!(old.receipt.key, original.key);
    let cancelled = control(&app, &token, "/dsg/v1/resize/cancel", &query(&original)).await?;
    assert_eq!(cancelled.receipt.key, original.key);
    let before = memory(&app, &token).await?;
    assert_eq!(before.observed.instance, initial.observed.instance);
    // 即使上层状态被误置为Running，新resize仍须验证原启动契约，不能只看状态。
    compute.state.lock().unwrap().status = ComputeStatus::Running;
    let next = resize_request(&before, 2, 1024, DsgResizeKind::Grow);
    let (status, _) = invoke(
        app.clone(),
        Method::POST,
        "/dsg/v1/resize",
        Some(&token),
        Some(&serde_json::to_string(&next)?),
    )
    .await?;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED);
    let after = memory(&app, &token).await?;
    assert_eq!(
        after.observed.capacity_generation,
        before.observed.capacity_generation
    );
    assert_eq!(after.observed.high_buffers, before.observed.high_buffers);
    assert_eq!(
        after.observed.operation.as_ref().map(|o| &o.key),
        before.observed.operation.as_ref().map(|o| &o.key)
    );
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict DSM gate requires exact PG5462 and installed Neon1.15"]
async fn native_dsm_state_requires_formal_member_and_retains_payload_budget() -> Result<()> {
    use compute_api::{dsg_dsm::DsgDsmResponse, dsg_prepared::DsgPreparedResponse};
    let signer = Signer::new();
    let token = signer.token(&signer.claims());
    let path = "/dsg/v1/work/dsm";
    let (mut old, _, old_app) = support::native_prepared(&signer).await?;
    assert_eq!(
        invoke(old_app, Method::GET, path, Some(&token), None)
            .await?
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    old.passed = true;
    drop(old);
    let (mut cluster, _, app) = support::native_dsm(&signer).await?;
    assert_eq!(
        invoke(app.clone(), Method::GET, path, None, None).await?.0,
        StatusCode::UNAUTHORIZED
    );
    let (code, body) = invoke(app.clone(), Method::GET, path, Some(&token), None).await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let fact: DsgDsmResponse = serde_json::from_str(&body)?;
    let bound = fact.prepared.connections.work.observed.instance.clone();
    fact.validate_for(&bound)?;
    assert!(
        fact.limit_enforced
            && fact.accounting_valid
            && fact.current_boot_only
            && !fact.vm_shrink_permit
    );
    assert_eq!(fact.external_budget_bytes.get(), 4194304);
    assert!(fact.reserved_upper_bytes.get() >= 2097152 && fact.external_segments > 0);
    assert_eq!(fact.pending_creations, 0);
    assert_eq!(fact.unknown_segments, 0);
    assert!(fact.prepared.connections.work.foreground_idle);
    assert_eq!(fact.prepared.prepared_slots, 1);
    let sql = cluster.connect().await?;
    let namespace: String = sql.query_one("SELECT quote_ident(n.nspname) FROM pg_extension e JOIN pg_namespace n ON n.oid=e.extnamespace WHERE e.extname='neon'", &[]).await?.get(0);
    // 普通角色在关门时先被55000拒绝；权限专项须在本隔离夹具明确开门，才能
    // 证明SQL ACL/C体的42501，而非把前置准入拒绝误当权限证据。检查后原位关门。
    sql.simple_query("SELECT public.prepared_test_gate(public.prepared_test_boot(),3,1,true)")
        .await?;
    // 先证明ACL拒绝；再故意授予/经SU definer包装，仍由C体的原session检查拒绝。
    sql.batch_execute("SET ROLE dsg_app").await?;
    let denied = sql
        .query_one(&format!("SELECT {namespace}.neon_dsg_dsm_state()"), &[])
        .await
        .unwrap_err();
    assert_eq!(
        denied.code(),
        Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
    );
    sql.batch_execute("RESET ROLE").await?;
    sql.batch_execute(&format!("GRANT USAGE ON SCHEMA {namespace} TO dsg_app; GRANT EXECUTE ON FUNCTION {namespace}.neon_dsg_dsm_state() TO dsg_app; CREATE FUNCTION public.dsm_test_definer() RETURNS jsonb LANGUAGE SQL SECURITY DEFINER AS 'SELECT {namespace}.neon_dsg_dsm_state()'; GRANT EXECUTE ON FUNCTION public.dsm_test_definer() TO dsg_app; SET SESSION AUTHORIZATION dsg_app")).await?;
    let denied = sql
        .query_one("SELECT public.dsm_test_definer()", &[])
        .await
        .unwrap_err();
    assert_eq!(
        denied.code(),
        Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
    );
    sql.batch_execute("RESET SESSION AUTHORIZATION").await?;
    sql.simple_query("SELECT public.prepared_test_gate(public.prepared_test_boot(),4,1,false)")
        .await?;
    sql.simple_query("SELECT public.dsm_native_unpin(handle) FROM public.dsm_native_handle")
        .await?;
    let exists: bool = sql
        .query_one(
            "SELECT public.dsm_native_probe(handle) FROM public.dsm_native_handle",
            &[],
        )
        .await?
        .get(0);
    assert!(!exists);
    let (code, body) = invoke(app.clone(), Method::GET, path, Some(&token), None).await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let released: DsgDsmResponse = serde_json::from_str(&body)?;
    released.validate_for(&bound)?;
    assert_eq!(
        released.reserved_upper_bytes.get() + 2097152,
        fact.reserved_upper_bytes.get()
    );
    assert_eq!(released.external_segments + 1, fact.external_segments);
    // 新序列化helper复用后，原Prepared1直接路由依然能保留实际2PC槽。
    let (code, body) = invoke(
        app.clone(),
        Method::GET,
        "/dsg/v1/work/prepared",
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(code, StatusCode::OK, "{body}");
    let previous: DsgPreparedResponse = serde_json::from_str(&body)?;
    previous.validate_for(&bound)?;
    assert_eq!(previous.prepared_slots, 1);
    sql.batch_execute(
        "DROP FUNCTION public.dsm_test_definer(); ALTER EXTENSION neon UPDATE TO '1.14'",
    )
    .await?;
    assert_eq!(
        invoke(app, Method::GET, path, Some(&token), None).await?.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    cluster.passed = true;
    Ok(())
}
