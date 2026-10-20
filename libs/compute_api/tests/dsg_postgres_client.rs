//! 真正Neon扩展/PG中的Rust客户端端到端，默认显式ignored，由严格宿主门禁运行。
//! 总是新建0700临时集群，不接受已有数据库连接串；失败保留日志与数据以便恢复。
#![cfg(all(feature = "postgres-client", unix))]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use compute_api::dsg_client::{DsgCallError, DsgSqlClient};
use compute_api::dsg_control::{DsgOperationQuery, DsgResizeKind, DsgResizeRequest, DsgUint64};
use compute_api::dsg_state::{DsgDisposition, DsgPhase, DsgReason};
use tempfile::TempDir;
use tokio::time::{sleep, timeout};
use tokio_postgres::{Client, NoTls};

const VM: &str = "dsg-vm-1";
const COMPUTE: &str = "dsg-compute-1";
const NAMESPACE: &str = "\"dsg-\"\"admin\"";
const WAIT: Duration = Duration::from_secs(5);

struct Cluster {
    directory: Option<TempDir>,
    bin: PathBuf,
    data: PathBuf,
    socket: PathBuf,
    log: PathBuf,
    started: bool,
    passed: bool,
}

impl Cluster {
    fn new() -> Result<Self> {
        let bin = PathBuf::from(
            std::env::var("NEON_TEST_PG_BIN").context("strict DSGA gate must provide PG bin")?,
        );
        let expected = std::env::var("NEON_TEST_DSG_EXPECTED_SHA")
            .context("strict gate must provide exact PG SHA")?;
        ensure!(
            expected.len() == 40 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid expected PG SHA"
        );
        let version = Command::new(bin.join("postgres"))
            .arg("--version")
            .output()?;
        ensure!(
            version.status.success()
                && String::from_utf8_lossy(&version.stdout).contains(&format!("({expected})")),
            "test PG binary does not match source lock"
        );
        let directory = tempfile::Builder::new()
            .prefix("neon-dsg-rust-")
            // tempfile目录默认0777再受umask影响；必须在创建时显式收窄，
            // 避免Linux /tmp中的trust socket被其他本机用户访问。
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()?;
        let root = directory.path();
        ensure!(
            root.metadata()?.permissions().mode() & 0o777 == 0o700,
            "isolated DSGA fixture requires private 0700 directory"
        );
        let mut cluster = Self {
            bin,
            data: root.join("data"),
            socket: root.join("socket"),
            log: root.join("postgres.log"),
            directory: Some(directory),
            started: false,
            passed: false,
        };
        std::fs::create_dir(&cluster.socket)?;
        cluster.command(
            "initdb",
            &[
                "-D",
                cluster.data.to_str().unwrap(),
                "-U",
                "dsg_admin",
                "-A",
                "trust",
                "--no-locale",
                "--encoding=UTF8",
                "--data-checksums",
                "--no-sync",
            ],
        )?;
        let socket = cluster
            .socket
            .to_str()
            .unwrap()
            .replace('\\', "\\\\")
            .replace('\'', "''");
        std::fs::write(
            cluster.data.join("postgresql.conf"),
            format!(
                "listen_addresses=''\nport=55432\nunix_socket_directories='{socket}'\n\
             shared_preload_libraries='test_dsg_region,neon'\nwal_consistency_checking='neon'\n\
             neon.last_written_lsn_cache_size=1024\nneon.dsg_vm_uid='{VM}'\n\
             neon.dsg_compute_instance_id='{COMPUTE}'\nenable_dynamic_shared_buffers=on\n\
             shared_buffers='2MB'\nmax_shared_buffers='64MB'\n"
            ),
        )?;
        cluster.start()?;
        cluster.command("psql", &["-h", cluster.socket.to_str().unwrap(), "-p", "55432", "-U", "dsg_admin",
            "-d", "postgres", "-X", "-v", "ON_ERROR_STOP=1", "-c", &format!(
                "CREATE SCHEMA {NAMESPACE}; CREATE EXTENSION neon SCHEMA {NAMESPACE} VERSION '1.7';\
                 CREATE FUNCTION public.dsg_control(text) RETURNS text AS '$libdir/test_dsg_region',\
                    'test_dsg_grow_control' LANGUAGE C STRICT;\
                 SELECT public.dsg_control('attach');")])?;
        Ok(cluster)
    }

    fn command(&self, executable: &str, arguments: &[&str]) -> Result<()> {
        let output = Command::new(self.bin.join(executable))
            .args(arguments)
            .output()?;
        ensure!(
            output.status.success(),
            "{executable} failed: {}\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            std::fs::read_to_string(&self.log).unwrap_or_default()
        );
        Ok(())
    }

    fn start(&mut self) -> Result<()> {
        // pg_ctl观察超时也可能已经启动，先保守登记，Drop总会尝试关闭本临时集群。
        self.started = true;
        self.command(
            "pg_ctl",
            &[
                "-D",
                self.data.to_str().unwrap(),
                "-l",
                self.log.to_str().unwrap(),
                "-w",
                "-t",
                "15",
                "start",
            ],
        )
    }

    fn restart(&mut self) -> Result<()> {
        self.command(
            "pg_ctl",
            &[
                "-D",
                self.data.to_str().unwrap(),
                "-m",
                "fast",
                "-w",
                "stop",
            ],
        )?;
        self.started = false;
        self.start()
    }

    async fn connect(&self) -> Result<Client> {
        let mut config = tokio_postgres::Config::new();
        config
            .host_path(&self.socket)
            .port(55432)
            .user("dsg_admin")
            .dbname("postgres");
        let (client, driver) = config.connect(NoTls).await?;
        tokio::spawn(async move {
            let _ = driver.await;
        });
        Ok(client)
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        if self.started {
            // 只针对本夹具创建的datadir；immediate可结束故意停在FATAL退出门禁的进程。
            // 不能在测试panic后等待该进程配合，亦不触碰用户已有数据库。
            let _ = Command::new(self.bin.join("pg_ctl"))
                .args([
                    "-D",
                    self.data.to_str().unwrap(),
                    "-m",
                    "immediate",
                    "-w",
                    "-t",
                    "10",
                    "stop",
                ])
                .output();
        }
        if !self.passed || std::thread::panicking() {
            if let Some(directory) = self.directory.take() {
                eprintln!(
                    "DSGA failure fixture retained: {}",
                    directory.into_path().display()
                );
            }
        }
    }
}

fn request(
    client: &DsgSqlClient<'_>,
    revision: u64,
    generation: u64,
    target: u32,
    kind: DsgResizeKind,
) -> DsgResizeRequest {
    let mut request: DsgResizeRequest =
        serde_json::from_str(include_str!("../testdata/dsg_request_v1.json")).unwrap();
    request.key.instance = client.instance().clone();
    request.key.issuer_epoch = DsgUint64::new(1);
    request.key.revision = DsgUint64::new(revision);
    request.key.operation_id.low = DsgUint64::new(revision);
    request.expected_capacity_generation = DsgUint64::new(generation);
    request.target_buffers = target;
    request.kind = kind;
    request
}

fn query(request: &DsgResizeRequest) -> DsgOperationQuery {
    DsgOperationQuery {
        protocol_version: 1,
        key: request.key.clone(),
    }
}

async fn arm(client: &Client, action: &str) -> Result<()> {
    client
        .query_one("SELECT public.dsg_control($1::text)", &[&action])
        .await?;
    Ok(())
}

async fn checksum(client: &Client) -> Result<(i64, i64, String)> {
    let row = client
        .query_one(
            "SELECT count(*)::bigint,sum(i)::bigint,\
        md5(string_agg(pad,',' ORDER BY i)) FROM public.dsg_data",
            &[],
        )
        .await?;
    Ok((row.get(0), row.get(1), row.get(2)))
}

async fn wait_gate(client: &Client) -> Result<()> {
    timeout(WAIT, async {
        loop {
            let row = client
                .query_one("SELECT public.dsg_control('prepare_waiting')", &[])
                .await?;
            if row.get::<_, &str>(0) == "yes" {
                return Ok(());
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("real PG gate was not reached")?
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn round_trip_history_and_quoted_schema() -> Result<()> {
    let mut cluster = Cluster::new()?;
    let execution = cluster.connect().await?;
    let observer = cluster.connect().await?;
    execution
        .batch_execute(
            "CREATE SCHEMA evil; CREATE FUNCTION evil.neon_dsg_capabilities() RETURNS jsonb \
        LANGUAGE plpgsql AS $$BEGIN RAISE EXCEPTION 'untrusted function was called'; END$$;\
        CREATE TEMP TABLE pg_extension(nope integer);\
        SET search_path=evil,pg_catalog",
        )
        .await?;
    let client = DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, WAIT).await?;
    assert!(!client.capabilities().production_ready);
    let original = request(&client, 1, 1, 1024, DsgResizeKind::Grow);
    assert_eq!(
        client.request_resize(&original).await?.receipt.disposition,
        DsgDisposition::Accepted
    );
    execution
        .batch_execute(
            "CREATE TABLE public.dsg_data(i integer,pad text); ALTER TABLE public.dsg_data ALTER pad SET STORAGE PLAIN;\
             INSERT INTO public.dsg_data SELECT i,repeat(md5(i::text),150) FROM generate_series(1,600) i",
        )
        .await?;
    let size: i64 = execution
        .query_one("SELECT pg_relation_size('public.dsg_data')", &[])
        .await?
        .get(0);
    assert!(
        size > 2 * 1024 * 1024,
        "real working set must exceed the bootstrap pool"
    );
    let before = checksum(&execution).await?;
    client
        .request_resize(&request(&client, 2, 2, 256, DsgResizeKind::Shrink))
        .await?;
    client
        .request_resize(&request(&client, 3, 3, 2048, DsgResizeKind::Grow))
        .await?;
    let result = client.resize_status(&query(&original)).await?;
    assert_eq!(result.receipt.result.unwrap().high_buffers, 1024);
    assert_eq!(result.observed.high_buffers, 2048);
    assert!(!result.vm_shrink_permit);
    let after = checksum(&observer).await?;
    assert_eq!(before, after);
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn observer_cancels_while_execution_is_blocked() -> Result<()> {
    let mut cluster = Cluster::new()?;
    let execution = cluster.connect().await?;
    let observer = cluster.connect().await?;
    let control = cluster.connect().await?;
    let client = DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, WAIT).await?;
    let original = request(&client, 1, 1, 1024, DsgResizeKind::Grow);
    arm(&execution, "arm_pause").await?;
    let cancel = async {
        wait_gate(&control).await?;
        let status = client.resize_status(&query(&original)).await?;
        assert_eq!(
            status.receipt.result.unwrap().operation.unwrap().phase,
            DsgPhase::Preparing
        );
        let cancelled = client.cancel_resize(&query(&original)).await?;
        assert_eq!(
            cancelled.receipt.disposition,
            DsgDisposition::CancelRequested
        );
        arm(&control, "release_prepare").await
    };
    let (result, cancelled) = tokio::join!(client.request_resize(&original), cancel);
    cancelled?;
    assert_eq!(
        result?.receipt.result.unwrap().operation.unwrap().reason,
        DsgReason::Cancelled
    );
    client
        .request_resize(&request(&client, 2, 1, 2048, DsgResizeKind::Grow))
        .await?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn dropped_future_preserves_original_intent_fence() -> Result<()> {
    let mut cluster = Cluster::new()?;
    let execution = cluster.connect().await?;
    let observer = cluster.connect().await?;
    let control = cluster.connect().await?;
    let client = DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, WAIT).await?;
    let original = request(&client, 1, 1, 1024, DsgResizeKind::Grow);
    arm(&execution, "arm_pause").await?;
    let mut in_flight = Box::pin(client.request_resize(&original));
    tokio::select! {
        result = &mut in_flight => panic!("resize unexpectedly finished: {result:?}"),
        reached = wait_gate(&control) => reached?,
    }
    drop(in_flight);
    let next = request(&client, 2, 1, 2048, DsgResizeKind::Grow);
    assert!(matches!(
        client.request_resize(&next).await,
        Err(DsgCallError::NotSent(_))
    ));
    client.cancel_resize(&query(&original)).await?;
    arm(&control, "release_prepare").await?;
    let replay = client.request_resize(&original).await?;
    assert_eq!(replay.receipt.disposition, DsgDisposition::Replayed);
    assert_eq!(
        replay.receipt.result.unwrap().operation.unwrap().reason,
        DsgReason::Cancelled
    );
    client.request_resize(&next).await?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn sql_error_after_commit_is_unconfirmed_until_full_replay() -> Result<()> {
    let mut cluster = Cluster::new()?;
    let execution = cluster.connect().await?;
    let observer = cluster.connect().await?;
    let control = cluster.connect().await?;
    let client = DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, WAIT).await?;
    client
        .request_resize(&request(&client, 1, 1, 1024, DsgResizeKind::Grow))
        .await?;
    let shrink = request(&client, 2, 2, 256, DsgResizeKind::Shrink);
    arm(&control, "attach_shrink_publish").await?;
    arm(&execution, "arm_error").await?;
    let failed = client.request_resize(&shrink).await;
    assert!(matches!(failed, Err(DsgCallError::Unconfirmed { ref key, .. }) if *key == shrink.key));
    let status = client.resize_status(&query(&shrink)).await?;
    assert_eq!(
        status.receipt.result.unwrap().operation.unwrap().phase,
        DsgPhase::Committed
    );
    assert_eq!(status.observed.high_buffers, 256);
    assert!(
        status
            .observed
            .arrays
            .iter()
            .all(|array| !array.provider.has_result)
    );
    let next = request(&client, 3, 3, 2048, DsgResizeKind::Grow);
    assert!(matches!(
        client.request_resize(&next).await,
        Err(DsgCallError::NotSent(_))
    ));
    assert_eq!(
        client.request_resize(&shrink).await?.receipt.disposition,
        DsgDisposition::Replayed
    );
    client.request_resize(&next).await?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn independent_sessions_expected_identity_and_extension_membership_are_required() -> Result<()>
{
    let mut cluster = Cluster::new()?;
    let execution = cluster.connect().await?;
    let observer = cluster.connect().await?;
    assert!(
        DsgSqlClient::bind(&execution, &execution, VM, COMPUTE, WAIT)
            .await
            .is_err()
    );
    assert!(
        DsgSqlClient::bind(&execution, &observer, "another-vm", COMPUTE, WAIT)
            .await
            .is_err()
    );
    assert!(
        DsgSqlClient::bind(&execution, &observer, VM, "another-compute", WAIT)
            .await
            .is_err()
    );
    let client = DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, WAIT).await?;
    assert_eq!(client.memory_status().await?.observed.high_buffers, 256);
    execution
        .batch_execute(&format!(
            "CREATE ROLE dsg_tenant; GRANT CREATE ON SCHEMA {NAMESPACE} TO dsg_tenant"
        ))
        .await?;
    assert!(
        DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, WAIT)
            .await
            .is_err()
    );
    execution
        .batch_execute(&format!(
            "REVOKE CREATE ON SCHEMA {NAMESPACE} FROM dsg_tenant; \
        GRANT dsg_admin TO dsg_tenant"
        ))
        .await?;
    assert!(
        DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, WAIT)
            .await
            .is_err()
    );
    execution
        .batch_execute("REVOKE dsg_admin FROM dsg_tenant")
        .await?;
    DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, WAIT).await?;
    execution
        .batch_execute(&format!(
            "ALTER EXTENSION neon DROP FUNCTION {NAMESPACE}.neon_dsg_capabilities()"
        ))
        .await?;
    // 同名C函数仍存在，但已不是Neon扩展成员；SDK不能通过名称猜测其可信性。
    assert!(
        DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, WAIT)
            .await
            .is_err()
    );
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn response_timeout_keeps_exact_original_body_and_does_not_cancel_execution() -> Result<()> {
    let mut cluster = Cluster::new()?;
    let execution = cluster.connect().await?;
    let observer = cluster.connect().await?;
    let control = cluster.connect().await?;
    let client =
        DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, Duration::from_secs(1)).await?;
    let original = request(&client, 1, 1, 1024, DsgResizeKind::Grow);
    arm(&execution, "arm_pause").await?;
    let (failed, reached) = tokio::join!(client.request_resize(&original), wait_gate(&control));
    reached?;
    assert!(
        matches!(failed, Err(DsgCallError::Unconfirmed { ref key, .. }) if *key == original.key)
    );
    let status = client.resize_status(&query(&original)).await?;
    assert_eq!(
        status.receipt.result.unwrap().operation.unwrap().phase,
        DsgPhase::Preparing
    );
    let mut changed = original.clone();
    changed.deadline_unix_ms = DsgUint64::new(u64::MAX - 1);
    assert!(matches!(
        client.request_resize(&changed).await,
        Err(DsgCallError::NotSent(_))
    ));
    client.cancel_resize(&query(&original)).await?;
    arm(&control, "release_prepare").await?;
    let replay = client.request_resize(&original).await?;
    assert_eq!(
        replay.receipt.result.unwrap().operation.unwrap().reason,
        DsgReason::Cancelled
    );
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn fatal_execution_reconnects_only_with_original_immutable_intent() -> Result<()> {
    let mut cluster = Cluster::new()?;
    let execution = cluster.connect().await?;
    let observer = cluster.connect().await?;
    let client = DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, WAIT).await?;
    let original = request(&client, 1, 1, 1024, DsgResizeKind::Grow);
    arm(&execution, "arm_fatal_unpaused").await?;
    assert!(matches!(
        client.request_resize(&original).await,
        Err(DsgCallError::Unconfirmed { .. })
    ));
    let status = client.resize_status(&query(&original)).await?;
    assert_eq!(
        status.receipt.result.unwrap().operation.unwrap().reason,
        DsgReason::Interrupted
    );
    let new_execution = cluster.connect().await?;
    let new_observer = cluster.connect().await?;
    let replacement = DsgSqlClient::bind(&new_execution, &new_observer, VM, COMPUTE, WAIT).await?;
    assert_eq!(replacement.instance(), client.instance());
    let replay = replacement.request_resize(&original).await?;
    assert_eq!(replay.receipt.disposition, DsgDisposition::Replayed);
    assert_eq!(
        replay.receipt.result.unwrap().operation.unwrap().phase,
        DsgPhase::Denied
    );
    replacement
        .request_resize(&request(&replacement, 2, 1, 2048, DsgResizeKind::Grow))
        .await?;
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn new_boot_returns_stale_and_cannot_silently_release_old_intent() -> Result<()> {
    let mut cluster = Cluster::new()?;
    let original = {
        let execution = cluster.connect().await?;
        let observer = cluster.connect().await?;
        let client = DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, WAIT).await?;
        let original = request(&client, 1, 1, 1024, DsgResizeKind::Grow);
        client.request_resize(&original).await?;
        original
    };
    cluster.restart()?;
    let execution = cluster.connect().await?;
    let observer = cluster.connect().await?;
    let client = DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, WAIT).await?;
    assert_ne!(
        client.instance().pg_boot_id,
        original.key.instance.pg_boot_id
    );
    let status = client.resize_status(&query(&original)).await?;
    assert_eq!(status.receipt.disposition, DsgDisposition::Stale);
    assert!(!status.receipt.has_result);
    assert_eq!(
        client.request_resize(&original).await?.receipt.disposition,
        DsgDisposition::Stale
    );
    // STALE不证明过去未执行；需要外层持久记录/预算对账，不能在同SDK直接换ID。
    assert!(matches!(
        client
            .request_resize(&request(&client, 2, 1, 2048, DsgResizeKind::Grow))
            .await,
        Err(DsgCallError::NotSent(_))
    ));
    cluster.passed = true;
    Ok(())
}
