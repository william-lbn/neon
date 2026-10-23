use super::*;
use crate::compute::ComputeNode;
use crate::dsg_roles::tests::{fixture, parameters};
use compute_api::responses::{ComputeConfig, ComputeCtlConfig};
use compute_api::spec::{JwksSettings, LocalProxySpec, Role};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::Arc;

use crate::dsg_roles::tests::cluster;

fn node(spec: ComputeSpec, data: &Path, pgbin: Option<&Path>) -> Arc<ComputeNode> {
    let mut params = parameters();
    params.pgdata = data.to_str().unwrap().into();
    params.connstr =
        "postgres://wrong:stale@unreachable.invalid:55432/tenant?options=-c%20role=postgres"
            .parse()
            .unwrap();
    if let Some(bin) = pgbin {
        params.pgbin = bin.join("postgres").to_str().unwrap().into();
    }
    Arc::new(
        ComputeNode::new(
            params,
            ComputeConfig {
                spec: Some(spec),
                compute_ctl_config: ComputeCtlConfig {
                    jwks: jsonwebtoken::jwk::JwkSet { keys: vec![] },
                    tls: None,
                },
            },
        )
        .unwrap(),
    )
}

#[test]
fn private_endpoint_port_respects_query_and_rejects_ambiguous_targets() -> Result<()> {
    assert_eq!(
        port(&"postgres://cloud_admin@localhost/postgres?host=/tmp/owned&port=55432".parse()?)?,
        55432
    );
    assert_eq!(
        port(&"postgres://cloud_admin@localhost/postgres".parse()?)?,
        5432
    );
    for raw in [
        "postgres://localhost:5432/postgres?port=55432",
        "postgres://localhost/postgres?port=5432&port=5432",
        "postgres://localhost/postgres?port=5432,55432",
        "postgres://localhost/postgres?port=0",
        "postgres://localhost/postgres?port=70000",
        "postgres://localhost/postgres?port=%2B5432",
    ] {
        assert!(port(&raw.parse()?).is_err());
    }
    Ok(())
}

#[test]
fn bootstrap_socket_cleanup_requires_fresh_data_and_preserves_unknown_entries() -> Result<()> {
    let root = tempfile::Builder::new()
        .prefix("db-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")?;
    initialize_socket(root.path(), 5432)?;
    let lock = socket(root.path()).join(".s.PGSQL.5432.lock");
    fs::write(&lock, "copied source PID may still be alive")?;
    fs::write(root.path().join("postmaster.pid"), "must refuse live data")?;
    assert!(initialize_socket(root.path(), 5432).is_err());
    assert!(lock.exists());
    fs::remove_file(root.path().join("postmaster.pid"))?;
    let unknown = socket(root.path()).join("business-data");
    fs::write(&unknown, "preserve")?;
    assert!(initialize_socket(root.path(), 5432).is_err());
    assert!(lock.exists() && unknown.exists());
    fs::remove_file(unknown)?;
    let target = root.path().join("outside");
    fs::write(&target, "keep target")?;
    symlink(&target, socket(root.path()).join(".s.PGSQL.5433.lock"))?;
    assert!(initialize_socket(root.path(), 5432).is_err());
    assert_eq!(fs::read_to_string(&target)?, "keep target");
    fs::remove_file(socket(root.path()).join(".s.PGSQL.5433.lock"))?;
    initialize_socket(root.path(), 5432)?;
    assert!(!lock.exists());
    Ok(())
}

#[test]
fn management_configs_have_one_private_host_and_no_legacy_credentials() {
    let data = Path::new("/tmp/dsg-private-config");
    let node = node(fixture(), data, None);
    let config = node.get_tokio_conn_conf(Some("new-hba-test"));
    assert_eq!(config.get_user(), Some("cloud_admin"));
    assert_eq!(config.get_dbname(), Some("postgres"));
    assert!(config.get_password().is_none());
    assert_eq!(config.get_ports(), &[55432]);
    assert!(
        matches!(config.get_hosts(), [tokio_postgres::config::Host::Unix(p)] if p == &data.join(SOCKET))
    );
    let sync = node.get_conn_conf(None);
    assert_eq!(sync.get_user(), Some("cloud_admin"));
    assert!(sync.get_password().is_none());
    let parsed: Config = private_dsn(data, 55432).parse().unwrap();
    assert_eq!(parsed.get_hosts(), config.get_hosts());
    assert_eq!(parsed.get_ports(), config.get_ports());
    assert_eq!(parsed.get_user(), config.get_user());
    assert_eq!(parsed.get_options(), config.get_options());
    assert!(parsed.get_password().is_none());
}

#[test]
fn live_mode_changes_are_rejected_before_files_or_network_and_legacy_getters_stay_legacy() {
    let data = Path::new("/tmp/dsg-mode-guard-unused");
    let managed = node(fixture(), data, None);
    managed
        .state
        .lock()
        .unwrap()
        .pspec
        .as_mut()
        .unwrap()
        .spec
        .dynamic_shared_buffers = None;
    assert!(
        managed
            .reconfigure()
            .unwrap_err()
            .to_string()
            .contains("cannot change")
    );
    let mut old = fixture();
    old.dynamic_shared_buffers = None;
    let legacy = node(old, data, None);
    let config = legacy.get_tokio_conn_conf(None);
    assert_eq!(config.get_user(), Some("wrong"));
    assert!(config.get_password().is_some());
    legacy
        .state
        .lock()
        .unwrap()
        .pspec
        .as_mut()
        .unwrap()
        .spec
        .dynamic_shared_buffers = fixture().dynamic_shared_buffers;
    assert!(
        legacy
            .reconfigure()
            .unwrap_err()
            .to_string()
            .contains("cannot change")
    );
    for field in 0..7 {
        let changed = node(fixture(), data, None);
        {
            let mut state = changed.state.lock().unwrap();
            let startup = state
                .pspec
                .as_mut()
                .unwrap()
                .spec
                .dynamic_shared_buffers
                .as_mut()
                .unwrap();
            match field {
                0 => startup.vm_uid = "recreated-vm".into(),
                1 => startup.compute_instance_id = "restarted-compute".into(),
                2 => startup.bootstrap_buffer_bytes += 8192,
                3 => startup.maximum_buffer_bytes += 8192,
                4 => startup.minimum_guest_memory_bytes += 8192,
                5 => startup.non_buffer_reserve_bytes += 8192,
                _ => startup.protocol_version += 1,
            }
        }
        assert!(
            changed
                .reconfigure()
                .unwrap_err()
                .to_string()
                .contains("cannot change")
        );
        changed.state.lock().unwrap().status = compute_api::responses::ComputeStatus::Running;
        assert_eq!(
            changed.dsg_spec().unwrap(),
            fixture().dynamic_shared_buffers.unwrap()
        );
        assert!(changed.verify_dsg_running().is_err());
    }
}

#[test]
fn owned_auth_files_replace_links_without_touching_targets_and_reject_unsafe_roots() -> Result<()> {
    let root = tempfile::Builder::new()
        .prefix("dh-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")?;
    let target = root.path().join("outside");
    fs::write(&target, "must remain untouched")?;
    symlink(&target, root.path().join(HBA))?;
    let first = prepare(root.path(), 5432, &fixture(), false)?;
    assert_eq!(fs::read_to_string(&target)?, "must remain untouched");
    assert!(
        !fs::symlink_metadata(root.path().join(HBA))?
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::metadata(root.path().join(HBA))?.mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(root.path().join(IDENT))?.mode() & 0o777, 0o600);
    let second = prepare(root.path(), 5432, &fixture(), true)?;
    assert_ne!(first.nonce_database, second.nonce_database);
    let before = fs::read(root.path().join(HBA))?;
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755))?;
    assert!(prepare(root.path(), 5432, &fixture(), false).is_err());
    assert_eq!(fs::read(root.path().join(HBA))?, before);
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))?;
    fs::remove_dir(root.path().join(SOCKET))?;
    symlink(root.path(), root.path().join(SOCKET))?;
    assert!(prepare(root.path(), 5432, &fixture(), false).is_err());
    assert_eq!(fs::read(root.path().join(HBA))?, before);
    assert!(startup_settings(Path::new("relative"), 5432).is_err());
    assert!(startup_settings(Path::new("/tmp").join("x".repeat(200)).as_path(), 5432).is_err());
    Ok(())
}

async fn sql_config(config: &Config) -> Result<Client> {
    connect(config).await
}

async fn denied(config: &Config, code: &str) -> Result<()> {
    match config.connect(NoTls).await {
        Err(e) => ensure!(
            e.code().is_some_and(|s| s.code() == code),
            "unexpected authentication refusal"
        ),
        Ok(_) => bail!("forbidden authentication unexpectedly succeeded"),
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires exact PG17 and installed full Neon/test libraries"]
async fn native_hba_peer_hold_scram_jwt_reload_proof_and_startup_overrides() -> Result<()> {
    let mut pg = cluster::Cluster::new()?;
    let old = pg.connect().await?;
    old.batch_execute("SET password_encryption='scram-sha-256'; CREATE ROLE cloud_admin SUPERUSER LOGIN; CREATE ROLE neon_superuser NOLOGIN IN ROLE pg_read_all_data,pg_write_all_data; CREATE ROLE seed LOGIN PASSWORD 'Hba-fixture-only-2026';").await?;
    let verifier: Option<String> = old
        .query_one(
            "SELECT rolpassword FROM pg_catalog.pg_authid WHERE rolname='seed'",
            &[],
        )
        .await?
        .get(0);
    let mut spec = fixture();
    spec.cluster.roles[1].encrypted_password = verifier.clone();
    let quoted = "a,\"\\role";
    spec.cluster.roles.push(Role {
        name: quoted.into(),
        encrypted_password: verifier,
        options: None,
    });
    spec.cluster.roles.push(Role {
        name: "jwt_reader".into(),
        encrypted_password: None,
        options: None,
    });
    spec.local_proxy_config = Some(LocalProxySpec {
        tls: None,
        jwks: Some(vec![JwksSettings {
            id: "hba-fixture".into(),
            role_names: vec!["jwt_reader".into()],
            jwks_url: "https://auth.example.test/jwks".into(),
            provider_name: "fixture".into(),
            jwt_audience: None,
        }]),
    });
    let node = node(spec.clone(), &pg.data, Some(&pg.bin));
    let config = node.get_tokio_conn_conf(None);
    // 先生成hold。备份/auto.conf故意放旧trust和外部socket；真正调用生产
    // start_postgres验证-c覆盖，而不是只断言生成字符串包含预期项。
    prepare(&pg.data, 55432, &spec, false)?;
    fs::write(
        pg.data.join("poison_hba"),
        "local all all trust\nhost all all all trust\n",
    )?;
    fs::write(
        pg.data.join("postgresql.auto.conf"),
        format!(
            "hba_file='{}'\nunix_socket_directories='{}'\n",
            pg.data.join("poison_hba").display(),
            pg.socket.display()
        ),
    )?;
    let mut conf = fs::read_to_string(pg.data.join("postgresql.conf"))?;
    conf.push_str("\nlisten_addresses='127.0.0.1'\n");
    fs::write(pg.data.join("postgresql.conf"), conf)?;
    let stopped = std::process::Command::new(pg.bin.join("pg_ctl"))
        .args(["-D", pg.data.to_str().unwrap(), "-w", "-m", "fast", "stop"])
        .output()?;
    ensure!(
        stopped.status.success(),
        "test-owned original PG did not stop"
    );
    drop(old);
    let starting = node.clone();
    let _handle = tokio::task::spawn_blocking(move || starting.start_postgres(None)).await??;
    let bin = pg.bin.clone();
    let data = pg.data.clone();
    scopeguard::defer! {
        let _ = std::process::Command::new(bin.join("pg_ctl")).args(["-D",data.to_str().unwrap(),"-w","-m","fast","stop"]).output();
    }
    let admin = sql_config(&config).await?;
    // Linux LFC消费的是URI而非Config；在Mac真实PG上验证同一URI的peer登录
    // 与GUC options，防止form的+编码令监控路径只在Linux启动后才暴露错误。
    let lfc: Config = private_dsn(&pg.data, 55432).parse()?;
    assert_eq!(
        sql_config(&lfc)
            .await?
            .query_one("SELECT current_user::text", &[])
            .await?
            .get::<_, String>(0),
        "cloud_admin"
    );
    assert_eq!(
        admin
            .query_one("SELECT current_user::text", &[])
            .await?
            .get::<_, String>(0),
        "cloud_admin"
    );
    assert!(
        !admin
            .query_one("SELECT pg_catalog.pg_is_in_recovery()", &[])
            .await?
            .get::<_, bool>(0)
    );
    let mut tcp = Config::new();
    tcp.host("127.0.0.1")
        .port(55432)
        .user("cloud_admin")
        .dbname("postgres");
    denied(&tcp, "28000").await?;
    tcp.user("dsg_app").password("Hba-fixture-only-2026");
    denied(&tcp, "28000").await?; // 未核验角色前HBA仍hold。
    crate::dsg_roles::apply(&admin, &spec, "neon_superuser").await?;
    let publish = node.clone();
    let source = spec.clone();
    tokio::task::spawn_blocking(move || publish.refresh_dsg_hba(&source, true)).await??;
    let app = sql_config(&tcp).await?;
    // HBA已开放但PG预算准入仍关闭；不能把认证成功冒充预算完成。
    assert_eq!(
        app.query_one("SELECT 1", &[])
            .await
            .unwrap_err()
            .code()
            .map(|c| c.code()),
        Some("55000")
    );
    admin.batch_execute("CREATE FUNCTION public.hba_boot() RETURNS bigint AS '$libdir/test_dsg_region','test_dsg_boot_id' LANGUAGE C; CREATE FUNCTION public.hba_admit(bigint,bigint,bigint,bool) RETURNS bool AS '$libdir/test_dsg_region','test_dsg_admission' LANGUAGE C STRICT; SELECT public.hba_admit(public.hba_boot(),1,1,true)").await?;
    assert_eq!(
        app.query_one("SELECT current_user::text", &[])
            .await?
            .get::<_, String>(0),
        "dsg_app"
    );
    let mut wrong = tcp.clone();
    wrong.password("wrong-password");
    denied(&wrong, "28P01").await?;
    let mut unusual = tcp.clone();
    unusual.user(quoted);
    assert_eq!(
        sql_config(&unusual)
            .await?
            .query_one("SELECT current_user::text", &[])
            .await?
            .get::<_, String>(0),
        quoted
    );
    let mut jwt = Config::new();
    jwt.host("127.0.0.1")
        .port(55432)
        .user("jwt_reader")
        .dbname("postgres");
    assert_eq!(
        sql_config(&jwt)
            .await?
            .query_one("SELECT current_user::text", &[])
            .await?
            .get::<_, String>(0),
        "jwt_reader"
    );
    let mut unknown = tcp.clone();
    unknown.user("seed");
    denied(&unknown, "28000").await?;
    let mut su = tcp.clone();
    su.user("cloud_admin");
    denied(&su, "28000").await?;
    // 故意构造语法错误：SIGHUP返回true但PG保留旧HBA。新nonce必须识别失败，
    // 不把磁盘pg_hba_file_rules或信号发送当作实际生效回执。
    let stale = prepare(&pg.data, 55432, &spec, false)?;
    let mut bytes = fs::read_to_string(pg.data.join(HBA))?;
    bytes.push_str("invalid_hba_entry\n");
    fs::write(pg.data.join(HBA), bytes)?;
    assert!(reload_and_verify(&config, &stale).await.is_err());
    let held = prepare(&pg.data, 55432, &spec, false)?;
    reload_and_verify(&config, &held).await?;
    denied(&tcp, "28000").await?;
    assert_eq!(app.query_one("SELECT 1", &[]).await?.get::<_, i32>(0), 1); // HBA不撤销既有业务连接；完整quiesce另验。
    pg.passed = true;
    Ok(())
}
