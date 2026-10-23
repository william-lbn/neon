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
        state.pspec.as_mut().unwrap().spec.dynamic_shared_buffers.as_mut().unwrap().vm_uid = "recreated-config-vm".into();
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
    let (status, _) = invoke(app.clone(), Method::POST, "/dsg/v1/resize", Some(&token), Some(&serde_json::to_string(&next)?)).await?;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED);
    let after = memory(&app, &token).await?;
    assert_eq!(after.observed.capacity_generation, before.observed.capacity_generation);
    assert_eq!(after.observed.high_buffers, before.observed.high_buffers);
    assert_eq!(after.observed.operation.as_ref().map(|o| &o.key), before.observed.operation.as_ref().map(|o| &o.key));
    cluster.passed = true;
    Ok(())
}
