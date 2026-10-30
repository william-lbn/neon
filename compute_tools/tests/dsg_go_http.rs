//! 真正Go客户端→TLS测试终结点→生产Compute Router→管理SQL→隔离PG闭环。
//! 不构建镜像/部署，不接受用户数据库；TLS终结点仅提供本机测试证书，不模拟回执。
#![cfg(unix)]

use std::process::Stdio;
use std::time::Duration;

use anyhow::{Result, ensure};
use compute_api::{
    dsg_control::{DsgResizeRequest, DsgUint64},
    dsg_state::DsgMemoryResponse,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

#[path = "support/dsg_http.rs"]
mod support;
use support::{Signer, native};

struct HttpTask(tokio::task::JoinHandle<()>);
impl Drop for HttpTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn run_go(binary: &str, input: &[u8]) -> Result<()> {
    run_go_test(binary, input, "TestNativeComputeHTTPChain").await
}

async fn run_go_test(binary: &str, input: &[u8], name: &str) -> Result<()> {
    let mut child = tokio::process::Command::new(binary)
        .args([
            "-test.run",
            &format!("^{name}$"),
            "-test.v",
            "-test.timeout",
            "40s",
        ])
        .env("NEON_DSG_NATIVE_TEST", "1")
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input).await?;
    drop(stdin);
    let output = timeout(Duration::from_secs(50), child.wait_with_output()).await??;
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    ensure!(
        output.status.success(),
        "Go native gate failed: {stdout}\n{stderr}"
    );
    ensure!(
        stdout.contains(&format!("--- PASS: {name}"))
            && !stdout.contains("--- SKIP:")
            && !stdout.contains("--- FAIL:"),
        "Go gate did not execute required test"
    );
    println!("{stdout}");

    Ok(())
}

#[tokio::test]
#[ignore = "strict gate requires exact PG, Neon and precompiled Go race binary"]
async fn native_go_connection_ack_keeps_raw_management_and_original_pending() -> Result<()> {
    let binary = std::env::var("NEON_TEST_DSG_GO_BIN")?;
    let signer = Signer::new();
    let (mut cluster, _, app) = support::native_connections(&signer).await?;
    let mut raw =
        tokio::net::UnixStream::connect(cluster.data.join("dsg_socket/.s.PGSQL.55432")).await?;
    let mut request = 8u32.to_be_bytes().to_vec();
    request.extend_from_slice(&80877104u32.to_be_bytes());
    raw.write_all(&request).await?;
    let mut answer = [0u8; 1];
    timeout(Duration::from_secs(3), raw.read_exact(&mut answer)).await??;
    ensure!(answer == *b"N", "raw private GSS did not reach backend");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = HttpTask(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let token = signer.token(&signer.claims());
    let initial = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(format!("http://{address}/dsg/v1/memory"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json::<DsgMemoryResponse>()
        .await?;
    let mut original: DsgResizeRequest = serde_json::from_str(include_str!(
        "../../libs/compute_api/testdata/dsg_request_v1.json"
    ))?;
    original.key.instance = initial.observed.instance.clone();
    original.key.issuer_epoch = DsgUint64::new(1);
    original.expected_capacity_generation = initial.observed.capacity_generation;
    let input = json!({"http_base_url":format!("http://{address}"),"instance":initial.observed.instance,
        "issuer_epoch":"1","token":token,"original":original});
    run_go_test(
        &binary,
        &serde_json::to_vec(&input)?,
        "TestNativeConnectionState",
    )
    .await?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate requires exact PG, Neon Prepared1 and precompiled Go race binary"]
async fn native_go_prepared_state_keeps_retained_slot_and_original_pending() -> Result<()> {
    let binary = std::env::var("NEON_TEST_DSG_GO_BIN")?;
    let signer = Signer::new();
    let (mut cluster, _, app) = support::native_prepared(&signer).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = HttpTask(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let token = signer.token(&signer.claims());
    let initial = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(format!("http://{address}/dsg/v1/memory"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json::<DsgMemoryResponse>()
        .await?;
    let mut original: DsgResizeRequest = serde_json::from_str(include_str!(
        "../../libs/compute_api/testdata/dsg_request_v1.json"
    ))?;
    original.key.instance = initial.observed.instance.clone();
    original.key.issuer_epoch = DsgUint64::new(1);
    original.expected_capacity_generation = initial.observed.capacity_generation;
    let input = json!({"http_base_url":format!("http://{address}"),"instance":initial.observed.instance,
        "issuer_epoch":"1","token":token,"original":original});
    run_go_test(
        &binary,
        &serde_json::to_vec(&input)?,
        "TestNativePreparedState",
    )
    .await?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict cross-repository work gate requires exact PG and Go race binary"]
async fn native_go_work_state_reads_actual_sql_without_clearing_pending() -> Result<()> {
    let binary = std::env::var("NEON_TEST_DSG_GO_BIN")?;
    let signer = Signer::new();
    let (mut cluster, _compute, app) = native(&signer).await?;
    cluster
        .connect()
        .await?
        .batch_execute("ALTER EXTENSION neon UPDATE TO '1.12'")
        .await?;
    // 真实GSS协商已得到N但尚无startup包/PGPROC，Go必须读到该在途启动。
    let mut raw = tokio::net::UnixStream::connect(cluster.socket.join(".s.PGSQL.55432")).await?;
    let mut request = 8u32.to_be_bytes().to_vec();
    request.extend_from_slice(&80877104u32.to_be_bytes());
    raw.write_all(&request).await?;
    let mut answer = [0u8; 1];
    timeout(Duration::from_secs(3), raw.read_exact(&mut answer)).await??;
    ensure!(answer == *b"N", "raw GSS negotiation did not reach backend");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = HttpTask(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let token = signer.token(&signer.claims());
    let initial = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(format!("http://{address}/dsg/v1/memory"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json::<DsgMemoryResponse>()
        .await?;
    let mut original: DsgResizeRequest = serde_json::from_str(include_str!(
        "../../libs/compute_api/testdata/dsg_request_v1.json"
    ))?;
    original.key.instance = initial.observed.instance.clone();
    original.key.issuer_epoch = DsgUint64::new(1);
    original.expected_capacity_generation = initial.observed.capacity_generation;
    let input = json!({"http_base_url":format!("http://{address}"),
        "instance":initial.observed.instance,"issuer_epoch":"1","token":token,"original":original});
    run_go_test(&binary, &serde_json::to_vec(&input)?, "TestNativeWorkState").await?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict cross-repository gate requires exact PG and precompiled Go race test binary"]
async fn native_go_terminal_reconcile_without_resize_replay() -> Result<()> {
    let binary = std::env::var("NEON_TEST_DSG_GO_BIN")?;
    ensure!(
        std::path::Path::new(&binary).is_file(),
        "Go test binary is missing"
    );
    let signer = Signer::new();
    let (mut cluster, _compute, app) = native(&signer).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = HttpTask(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let token = signer.token(&signer.claims());
    let initial = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(format!("http://{address}/dsg/v1/memory"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json::<DsgMemoryResponse>()
        .await?;
    let mut original: DsgResizeRequest = serde_json::from_str(include_str!(
        "../../libs/compute_api/testdata/dsg_request_v1.json"
    ))?;
    original.key.instance = initial.observed.instance.clone();
    original.key.issuer_epoch = DsgUint64::new(1);
    original.key.revision = DsgUint64::new(1);
    original.expected_capacity_generation = initial.observed.capacity_generation;
    original.validate()?;
    let mut next = signer.claims();
    next["issuer_epoch"] = json!("2");
    let input = json!({"http_base_url":format!("http://{address}"),
        "instance":initial.observed.instance,"issuer_epoch":"1","token":token,
        "next_token":signer.token(&next),"original":original});
    // 仅执行本批新增Go用例，不重复原全链路/重启/业务数据门禁。TLS为本机
    // 测试终结，Router/JWT/SQL/PG均为实际实现，私密输入只走匿名stdin。
    run_go_test(
        &binary,
        &serde_json::to_vec(&input)?,
        "TestNativeTerminalReconcile",
    )
    .await?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict cross-repository gate requires exact PG and precompiled Go race test binary"]
async fn native_go_resize_recovery_keeps_unknown_and_isolates_expired_result() -> Result<()> {
    let binary = std::env::var("NEON_TEST_DSG_GO_BIN")?;
    ensure!(
        std::path::Path::new(&binary).is_file(),
        "Go binary is missing"
    );
    for mode in ["unreceived", "committed", "expired"] {
        // 三个独立准确PG实例；不以peer或修改历史缓存代替实际原生淘汰。
        // JWT只走匿名stdin，原生恢复不授予Guest/VM/Host释放许可。
        let signer = Signer::new();
        let (mut cluster, _compute, app) = native(&signer).await?;
        // 本根新增完整实际预算的SharedCost读回；基础夹具仍从1.8启动，
        // 只为本根升级到提供正式扩展成员函数的最小1.11，不放宽SDK发现。
        cluster
            .connect()
            .await?
            .batch_execute("ALTER EXTENSION neon UPDATE TO '1.11'")
            .await?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let _server = HttpTask(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        let token = signer.token(&signer.claims());
        let initial = reqwest::Client::builder()
            .no_proxy()
            .build()?
            .get(format!("http://{address}/dsg/v1/memory"))
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?
            .json::<DsgMemoryResponse>()
            .await?;
        let mut original: DsgResizeRequest = serde_json::from_str(include_str!(
            "../../libs/compute_api/testdata/dsg_request_v1.json"
        ))?;
        original.key.instance = initial.observed.instance.clone();
        original.key.issuer_epoch = DsgUint64::new(1);
        original.key.revision = DsgUint64::new(1);
        original.expected_capacity_generation = initial.observed.capacity_generation;
        original.validate()?;
        let mut next = signer.claims();
        next["issuer_epoch"] = json!("2");
        let input = json!({"http_base_url":format!("http://{address}"),
            "instance":initial.observed.instance,"issuer_epoch":"1","token":token,
            "next_token":signer.token(&next),"original":original,"mode":mode});
        run_go_test(
            &binary,
            &serde_json::to_vec(&input)?,
            "TestNativeResizeRecovery",
        )
        .await?;
        cluster.passed = true;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "strict cross-repository gate requires exact PG and precompiled Go race test binary"]
async fn native_go_rejected_resize_barrier_preserves_original() -> Result<()> {
    let binary = std::env::var("NEON_TEST_DSG_GO_BIN")?;
    ensure!(
        std::path::Path::new(&binary).is_file(),
        "Go test binary is missing"
    );
    let signer = Signer::new();
    let (mut cluster, _compute, app) = native(&signer).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = HttpTask(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let token = signer.token(&signer.claims());
    let initial = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(format!("http://{address}/dsg/v1/memory"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json::<DsgMemoryResponse>()
        .await?;
    let mut original: DsgResizeRequest = serde_json::from_str(include_str!(
        "../../libs/compute_api/testdata/dsg_request_v1.json"
    ))?;
    original.key.instance = initial.observed.instance.clone();
    original.key.issuer_epoch = DsgUint64::new(1);
    original.key.revision = DsgUint64::new(1);
    original.expected_capacity_generation = initial.observed.capacity_generation;
    original.validate()?;
    let mut next = signer.claims();
    next["issuer_epoch"] = json!("2");
    let input = json!({"http_base_url":format!("http://{address}"),
        "instance":initial.observed.instance,"issuer_epoch":"1","token":token,
        "next_token":signer.token(&next),"original":original});
    // 仅执行本批新增Go用例，不重复原全链路/重启/业务数据门禁。TLS为本机
    // 测试终结，Router/JWT/SQL/PG均为实际实现，私密输入只走匿名stdin。
    run_go_test(
        &binary,
        &serde_json::to_vec(&input)?,
        "TestNativeRejectedResizeBarrier",
    )
    .await?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict cross-repository gate requires exact PG and precompiled Go race test binary"]
async fn native_go_compute_sql_round_trip_and_lost_response() -> Result<()> {
    let binary = std::env::var("NEON_TEST_DSG_GO_BIN")?;
    ensure!(
        std::path::Path::new(&binary).is_file(),
        "Go test binary is missing"
    );
    let signer = Signer::new();
    let (mut cluster, _compute, app) = native(&signer).await?;
    let token = signer.token(&signer.claims());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = HttpTask(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let http = reqwest::Client::builder().no_proxy().build()?;
    let initial = http
        .get(format!("http://{address}/dsg/v1/memory"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json::<DsgMemoryResponse>()
        .await?;
    let mut original: DsgResizeRequest = serde_json::from_str(include_str!(
        "../../libs/compute_api/testdata/dsg_request_v1.json"
    ))?;
    original.key.instance = initial.observed.instance.clone();
    original.key.issuer_epoch = DsgUint64::new(1);
    original.key.revision = DsgUint64::new(1);
    original.expected_capacity_generation = initial.observed.capacity_generation;
    original.budget_commitment_id = "quote\"-slash\\-html<>&/".into();
    original.validate()?;

    let sql = cluster.connect().await?;
    sql.batch_execute("CREATE TABLE public.dsg_data(i int,pad text); ALTER TABLE public.dsg_data ALTER pad SET STORAGE PLAIN;\
        INSERT INTO public.dsg_data SELECT i,repeat(md5(i::text),150) FROM generate_series(1,600) i").await?;
    let before = sql.query_one("SELECT count(*)::bigint,sum(i)::bigint,md5(string_agg(pad,'' ORDER BY i)),pg_relation_size('public.dsg_data') FROM public.dsg_data",&[]).await?;
    ensure!(
        before.get::<_, i64>(3) > 2 * 1024 * 1024,
        "data must exceed bootstrap pool"
    );
    let expected = (
        before.get::<_, i64>(0),
        before.get::<_, i64>(1),
        before.get::<_, String>(2),
    );
    let mut rejected = signer.claims();
    rejected["scope"] = json!("compute_ctl:admin");
    let mut next_owner = signer.claims();
    next_owner["issuer_epoch"] = json!("2");

    // 一次性JWT只经匿名stdin管道传入Go，不放argv/env/测试输出或持久账本。Go
    // 会按真实header送达生产验签中间件；原始输入及失败stderr不能输出配置对象。
    let mut input = json!({
        "http_base_url":format!("http://{address}"), "instance":initial.observed.instance,
        "issuer_epoch":"1", "token":token,
        "rejected_token":signer.raw_token(&serde_json::to_string(&rejected)?),
        "next_token":signer.token(&next_owner), "original":original, "after_restart":false
    });
    run_go(&binary, &serde_json::to_vec(&input)?).await?;

    // 校验在Go进程结束之后直接读取真实PG数据，不能以HTTP响应或人工checksum替代。
    let after = sql.query_one("SELECT count(*)::bigint,sum(i)::bigint,md5(string_agg(pad,'' ORDER BY i)) FROM public.dsg_data",&[]).await?;
    let actual = (
        after.get::<_, i64>(0),
        after.get::<_, i64>(1),
        after.get::<_, String>(2),
    );
    assert_eq!(actual, expected);
    // 保留原Compute的OnceCell连接/栅栏，再真实换Postmaster；禁止为了通过测试
    // 新建Compute并静默丢弃旧上下文。新的Go进程仍必须从同一生产端点得到503。
    cluster.restart()?;
    let unavailable = http
        .get(format!("http://{address}/dsg/v1/capabilities"))
        .bearer_auth(signer.token(&signer.claims()))
        .send()
        .await?;
    ensure!(
        unavailable.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "old Compute must not reconnect silently"
    );
    input["after_restart"] = json!(true);
    input["token"] = json!(signer.token(&signer.claims()));
    run_go(&binary, &serde_json::to_vec(&input)?).await?;
    let reopened = cluster.connect().await?;
    let durable = reopened.query_one("SELECT count(*)::bigint,sum(i)::bigint,md5(string_agg(pad,'' ORDER BY i)) FROM public.dsg_data",&[]).await?;
    assert_eq!(
        (
            durable.get::<_, i64>(0),
            durable.get::<_, i64>(1),
            durable.get::<_, String>(2)
        ),
        expected
    );
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate requires exact PG, Neon DSM1 and precompiled Go race binary"]
async fn native_go_dsm_state_keeps_payload_budget_and_original_pending() -> Result<()> {
    let binary = std::env::var("NEON_TEST_DSG_GO_BIN")?;
    let signer = Signer::new();
    let (mut cluster, _, app) = support::native_dsm(&signer).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = HttpTask(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let token = signer.token(&signer.claims());
    let initial = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(format!("http://{address}/dsg/v1/memory"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json::<DsgMemoryResponse>()
        .await?;
    let mut original: DsgResizeRequest = serde_json::from_str(include_str!(
        "../../libs/compute_api/testdata/dsg_request_v1.json"
    ))?;
    original.key.instance = initial.observed.instance.clone();
    original.key.issuer_epoch = DsgUint64::new(1);
    original.expected_capacity_generation = initial.observed.capacity_generation;
    let input = json!({"http_base_url":format!("http://{address}"),"instance":initial.observed.instance,
        "issuer_epoch":"1","token":token,"original":original});
    run_go_test(&binary, &serde_json::to_vec(&input)?, "TestNativeDSMState").await?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate requires exact PG, Neon1.15 and precompiled Go budget race binary"]
async fn native_go_quiesce_consumes_real_close_and_keeps_retained_costs() -> Result<()> {
    let binary = std::env::var("NEON_TEST_DSG_GO_BIN")?;
    let signer = Signer::new();
    let (mut cluster, _, app) = support::native_dsm(&signer).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = HttpTask(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let token = signer.token(&signer.claims());
    let initial = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(format!("http://{address}/dsg/v1/memory"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json::<DsgMemoryResponse>()
        .await?;
    let mut original: DsgResizeRequest = serde_json::from_str(include_str!(
        "../../libs/compute_api/testdata/dsg_request_v1.json"
    ))?;
    original.key.instance = initial.observed.instance.clone();
    original.key.issuer_epoch = DsgUint64::new(1);
    original.expected_capacity_generation = initial.observed.capacity_generation;
    let input = json!({"http_base_url":format!("http://{address}"),"instance":initial.observed.instance,
        "issuer_epoch":"1","token":token,"original":original});
    // 真实关闭/原生同点事实交新预算消费者。Go内root/外部来源明确为协议模型，
    // 本门禁不声称Linux cgroup、完整预算生产者或整个集群已经验收。
    run_go_test(
        &binary,
        &serde_json::to_vec(&input)?,
        "TestNativeQuiesceConsumptionKeepsRetainedPayload",
    )
    .await?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict retained service gate requires exact PG17 and a precompiled Go race binary"]
async fn native_go_retained_budget_reopens_original_id_and_fences_late_close() -> Result<()> {
    let binary = std::env::var("NEON_TEST_DSG_GO_BIN")?;
    ensure!(
        std::path::Path::new(&binary).is_file(),
        "Go binary is missing"
    );
    let signer = Signer::new();
    // 本根实际启用连接门禁及有限原生期限，不以领域peer代替PG开放。
    // 仅为本私有PG升级到同时提供SharedCost与有限期限正式成员的1.16；
    // 不改生产扩展或旧矩阵，也不能把缺SQL成员的503当作期限检查通过。
    let (mut cluster, _compute, app) = support::native_connections(&signer).await?;
    cluster
        .connect()
        .await?
        .batch_execute("ALTER EXTENSION neon UPDATE TO '1.16'")
        .await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = HttpTask(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let token = signer.token(&signer.claims());
    let initial = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(format!("http://{address}/dsg/v1/memory"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json::<DsgMemoryResponse>()
        .await?;
    let input = json!({"http_base_url":format!("http://{address}"),
        "instance":initial.observed.instance,"issuer_epoch":"1","token":token});
    run_go_test(
        &binary,
        &serde_json::to_vec(&input)?,
        "TestNativeRetainedServiceUsesOriginalBudgetWithoutResize",
    )
    .await?;
    cluster.passed = true;
    Ok(())
}
