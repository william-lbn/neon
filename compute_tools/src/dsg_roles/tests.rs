use super::*;
use compute_api::dynamic_shared_buffers::DynamicSharedBuffersSpec;
use compute_api::spec::{DeltaOp, GenericOption, JwksSettings, LocalProxySpec};
use std::sync::{Arc, atomic::AtomicU64};
use tokio::sync::RwLock;

#[path = "../../../libs/compute_api/tests/support/dsg_cluster.rs"]
// 通用夹具的restart由其他原生门禁使用，本角色专项不需要重启该实例。
#[allow(dead_code)]
pub(crate) mod cluster;

fn verifier() -> String {
    format!(
        "SCRAM-SHA-256$4096:{}${}:{}",
        STANDARD.encode([1; 16]),
        STANDARD.encode([2; 32]),
        STANDARD.encode([3; 32])
    )
}

fn role(name: &str, password: Option<String>) -> Role {
    Role {
        name: name.into(),
        encrypted_password: password,
        options: None,
    }
}

pub(crate) fn fixture() -> ComputeSpec {
    let mut spec: ComputeSpec =
        serde_json::from_str(include_str!("../../tests/cluster_spec.json")).unwrap();
    spec.dynamic_shared_buffers = Some(DynamicSharedBuffersSpec {
        protocol_version: 2,
        vm_uid: cluster::VM.into(),
        compute_instance_id: cluster::COMPUTE.into(),
        bootstrap_buffer_bytes: 2 << 20,
        maximum_buffer_bytes: 64 << 20,
        minimum_guest_memory_bytes: 128 << 20,
        non_buffer_reserve_bytes: 64 << 20,
    });
    spec.cluster.roles = vec![role("aux_owner", None), role("dsg_app", Some(verifier()))];
    spec.cluster.databases.clear();
    spec.delta_operations = None;
    spec.safekeeper_connstrings = vec!["127.0.0.1:6501".into(), "127.0.0.1:6502".into()];
    spec
}

fn option(name: &str, value: Option<&str>) -> GenericOption {
    GenericOption {
        name: name.into(),
        value: value.map(str::to_owned),
        vartype: "enum".into(),
    }
}

#[test]
fn legacy_policy_is_preserved() {
    let mut spec = fixture();
    spec.dynamic_shared_buffers = None;
    spec.cluster.roles = vec![role("postgres", Some("legacy-md5".into()))];
    validate_spec(&spec).unwrap();
    validate_parameters(&spec, "legacy custom group", true).unwrap();
}

#[test]
fn managed_parsing_precedes_pgdata_and_redacts_verifiers() {
    let mut spec = fixture();
    crate::compute::ParsedSpec::try_from(spec.clone()).unwrap();
    assert!(!format!("{spec:?}").contains(&verifier()));
    assert!(serde_json::to_string(&spec).unwrap().contains(&verifier()));
    assert!(
        !query(&spec, "neon_superuser")
            .unwrap()
            .definition
            .contains(&verifier())
    );
    spec.skip_pg_catalog_updates = true;
    assert!(validate_spec(&spec).is_err());
    spec.mode = ComputeMode::Replica;
    validate_spec(&spec).unwrap(); // RO可以禁止目录写，但不能跳过后面的真实快照核验。
    spec.skip_pg_catalog_updates = false;
    spec.cluster.roles[1].name = "postgres".into();
    let error = crate::compute::ParsedSpec::try_from(spec)
        .unwrap_err()
        .to_string();
    assert!(error.contains("reserved or invalid"));
}

#[test]
fn reserved_duplicate_and_unbounded_names_are_rejected() {
    for name in [
        "postgres",
        "/.*",
        "cloud_admin",
        "Cloud_Admin",
        "zenith_admin",
        "pg_read_all_data",
        "neon_superuser",
        "public",
        "none",
        "",
        "bad\0role",
        "bad\nrole",
        &"x".repeat(64),
        &"🦀".repeat(16),
    ] {
        let mut spec = fixture();
        spec.cluster.roles[1].name = name.into();
        assert!(validate_spec(&spec).is_err(), "{name:?}");
    }
    let mut spec = fixture();
    spec.cluster.roles.push(spec.cluster.roles[1].clone());
    assert!(validate_spec(&spec).is_err());
    spec = fixture();
    spec.cluster.roles = vec![role(ADMIN, Some(verifier())); MAX_ROLES + 1];
    assert!(validate_spec(&spec).is_err());
    let mut spec = fixture();
    spec.cluster.roles[1].name = "tenant'\\\"$x$;$xx$".into();
    validate_spec(&spec).unwrap();
}

#[test]
fn scram_shape_and_internal_administrator_separation_are_mandatory() {
    for value in [
        "md5abc".to_owned(),
        "abc".into(),
        verifier().replace("4096:", "4095:"),
        verifier().replace("4096:", "1000001:"),
        verifier().replace("4096:", "+4096:"),
        verifier().replace(&STANDARD.encode([2; 32]), &STANDARD.encode([2; 31])),
        verifier() + "'; SELECT 1; --",
    ] {
        let mut spec = fixture();
        spec.cluster.roles[1].encrypted_password = Some(value);
        assert!(validate_spec(&spec).is_err());
    }
    let mut spec = fixture();
    spec.cluster.roles.push(role(ADMIN, Some(verifier())));
    assert!(validate_spec(&spec).is_err());
    spec = fixture();
    spec.cluster.roles[1].encrypted_password = None;
    validate_spec(&spec).unwrap(); // JWT/NOLOGIN身份可以没有数据库口令。
    spec = fixture();
    spec.cluster.roles.push(role(ADMIN, None));
    assert!(validate_spec(&spec).is_err());
}

#[test]
fn options_are_bounded_semantic_values_and_conflicts_fail() {
    for invalid in [
        vec![option("SUPERUSER", None)],
        vec![option("IN ROLE", Some("cloud_admin"))],
        vec![option("PASSWORD", Some("secret"))],
        vec![option("LOGIN;SELECT 1", None)],
        vec![option("CREATEDB", Some("true"))],
        vec![option("CONNECTION LIMIT", Some("1; SELECT 1"))],
        vec![option("VALID UNTIL", Some("tomorrow"))],
        vec![option("LOGIN", None), option("NOLOGIN", None)],
    ] {
        let mut spec = fixture();
        spec.cluster.roles[1].options = Some(invalid);
        assert!(validate_spec(&spec).is_err());
    }
    let mut spec = fixture();
    spec.cluster.roles[1].options = Some(vec![
        option("NOSUPERUSER", None),
        option("NOREPLICATION", None),
        option("CONNECTION LIMIT", Some("3")),
        option("VALID UNTIL", Some("2099-01-01T00:00:00Z")),
    ]);
    validate_spec(&spec).unwrap();
}

#[test]
fn jwt_deltas_database_owners_and_internal_group_are_guarded() {
    for names in [vec![], vec![ADMIN.to_owned()], vec!["undeclared".into()]] {
        let mut spec = fixture();
        spec.local_proxy_config = Some(LocalProxySpec {
            tls: None,
            jwks: Some(vec![JwksSettings {
                id: "key".into(),
                role_names: names,
                jwks_url: "https://auth.example.test/jwks".into(),
                provider_name: "test".into(),
                jwt_audience: None,
            }]),
        });
        assert!(validate_spec(&spec).is_err());
    }
    for op in [
        DeltaOp {
            action: "delete_role".into(),
            name: ADMIN.into(),
            new_name: None,
        },
        DeltaOp {
            action: "rename_role".into(),
            name: "dsg_app".into(),
            new_name: Some(ADMIN.into()),
        },
    ] {
        let mut spec = fixture();
        spec.delta_operations = Some(vec![op]);
        assert!(validate_spec(&spec).is_err());
    }
    let mut spec = fixture();
    let db: compute_api::spec::Database = serde_json::from_value(
        serde_json::json!({"name":"app","owner":"cloud_admin","options":null}),
    )
    .unwrap();
    spec.cluster.databases.push(db);
    assert!(validate_spec(&spec).is_err());
    spec.cluster.databases[0].owner = "dsg_app".into();
    validate_spec(&spec).unwrap();
    for weak in ["cloud_admin", "pg_x", "x'; SELECT 1; --", "dsg_app", ""] {
        assert!(validate_parameters(&spec, weak, false).is_err());
    }
    validate_parameters(&spec, "neon_superuser", false).unwrap();
    assert!(validate_parameters(&spec, "neon_superuser", true).is_err());
}

pub(crate) fn parameters() -> crate::compute::ComputeNodeParams {
    crate::compute::ComputeNodeParams {
        compute_id: "role-native-test".into(),
        connstr: "postgres://cloud_admin@localhost/postgres".parse().unwrap(),
        privileged_role_name: "neon_superuser".into(),
        resize_swap_on_bind: false,
        set_disk_quota_for_fs: None,
        #[cfg(target_os = "linux")]
        filecache_connstr: String::new(),
        #[cfg(target_os = "linux")]
        cgroup: String::new(),
        #[cfg(target_os = "linux")]
        vm_monitor_addr: String::new(),
        pgdata: String::new(),
        pgbin: String::new(),
        pgversion: "17".into(),
        external_http_port: 0,
        internal_http_port: 0,
        remote_ext_base_url: None,
        installed_extensions_collection_interval: Arc::new(AtomicU64::new(3600)),
        instance_id: None,
        pg_init_timeout: None,
        pg_isready_bin: String::new(),
        lakebase_mode: false,
        build_tag: "native-role-test".into(),
        control_plane_uri: None,
        config_path_test_only: None,
    }
}

type ApplyContext = Arc<RwLock<crate::spec_apply::MutableApplyContext>>;
fn context() -> ApplyContext {
    Arc::new(RwLock::new(crate::spec_apply::MutableApplyContext {
        roles: Default::default(),
        dbs: Default::default(),
    }))
}

async fn production_phase(client: &Client, spec: &ComputeSpec, ctx: ApplyContext) -> Result<()> {
    crate::spec_apply::apply_operations(
        Arc::new(parameters()),
        Arc::new(spec.clone()),
        ctx,
        Arc::new(HashSet::new()),
        crate::spec_apply::ApplySpecPhase::CreateAndAlterRoles,
        || async { Ok(client) },
        false,
    )
    .await
}

async fn native_setup() -> Result<(cluster::Cluster, Client, Client, ComputeSpec)> {
    let cluster = cluster::Cluster::new()?;
    let admin = cluster.connect().await?;
    admin.batch_execute("SET password_encryption='scram-sha-256'; CREATE ROLE cloud_admin SUPERUSER LOGIN PASSWORD 'Private-native-fixture-only-2026'; CREATE ROLE app_seed LOGIN PASSWORD 'Tenant-native-fixture-only-2026';").await?;
    // 使用实际生产模板与监控继承，不能用一个空弱组替代Neon真实权限。
    admin
        .batch_execute(&format!(
            include_str!("../sql/create_privileged_role.sql"),
            privileged_role_name = "neon_superuser",
            privileges = "CREATEDB CREATEROLE NOLOGIN REPLICATION BYPASSRLS"
        ))
        .await?;
    admin
        .batch_execute("GRANT pg_monitor TO neon_superuser")
        .await?;
    let mut spec = fixture();
    for (index, name) in [(1, "app_seed")] {
        spec.cluster.roles[index].encrypted_password = admin
            .query_one(
                "SELECT rolpassword FROM pg_catalog.pg_authid WHERE rolname=$1",
                &[&name],
            )
            .await?
            .get(0);
    }
    // 应用通过真实SCRAM；内部cloud_admin仅在夹具0700私有socket上沿用官方
    // 无口令管理方式。额外credential_probe规则只用于证明旧管理口令已不可用，
    // 不是生产HBA部署模板；后续仍须验证网络、代理及管理通道的完整隔离。
    let hba = cluster.data.join("pg_hba.conf");
    let previous = std::fs::read_to_string(&hba)?;
    std::fs::write(
        &hba,
        format!(
            "local credential_probe cloud_admin scram-sha-256\nlocal all app_seed,dsg_app scram-sha-256\n{previous}"
        ),
    )?;
    admin.simple_query("SELECT pg_reload_conf()").await?;
    let mut applied = false;
    for _ in 0..40 {
        match cluster
            .connect_as("app_seed", Some("deliberately-wrong-password"))
            .await
        {
            Err(error) => {
                ensure!(
                    error
                        .downcast_ref::<tokio_postgres::Error>()
                        .and_then(|e| e.code())
                        .is_some_and(|code| code.code() == "28P01"),
                    "wrong-password login did not fail as SCRAM authentication"
                );
                applied = true;
                break;
            }
            Ok(client) => drop(client),
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    ensure!(applied, "SCRAM HBA reload not observed");
    let private = cluster.connect_as(ADMIN, None).await?;
    Ok((cluster, admin, private, spec))
}

#[tokio::test]
#[ignore = "requires explicit accurate PG17 bin/SHA and installed test libraries"]
async fn native_roles_use_actual_dispatch_and_preserve_admission_and_replay() -> Result<()> {
    let (mut cluster, admin, private, mut spec) = native_setup().await?;
    admin
        .batch_execute("CREATE ROLE existing_app NOLOGIN NOCREATEDB; CREATE ROLE bridge_role;")
        .await?;
    spec.cluster.roles.push(role("existing_app", None));
    let quoted = "tenant'\\\"$x$;$xx$";
    spec.cluster.roles.push(role(quoted, None));
    spec.cluster.roles.push(role("jwt_app", None));
    spec.local_proxy_config = Some(LocalProxySpec {
        tls: None,
        jwks: Some(vec![JwksSettings {
            id: "jwt-role-fixture".into(),
            role_names: vec!["jwt_app".into()],
            jwks_url: "https://auth.example.test/jwks".into(),
            provider_name: "fixture".into(),
            jwt_audience: None,
        }]),
    });
    let ctx = context();
    production_phase(&private, &spec, ctx.clone()).await?;
    assert!(
        admin
            .query_one(
                "SELECT rolpassword IS NULL FROM pg_catalog.pg_authid WHERE rolname='cloud_admin'",
                &[]
            )
            .await?
            .get::<_, bool>(0)
    );
    // 一个已知旧口令在专用SCRAM规则上也必须被拒绝；若仍有效会走到不存在库的
    // 3D000错误，只有28P01才证明认证本身失败。无口令内部通道仍可用于管理。
    let mut probe = tokio_postgres::Config::new();
    probe
        .host_path(&cluster.socket)
        .port(55432)
        .user(ADMIN)
        .dbname("credential_probe")
        .password("Private-native-fixture-only-2026");
    match probe.connect(NoTls).await {
        Err(error) => assert_eq!(error.code().map(|c| c.code()), Some("28P01")),
        Ok(_) => bail!("cleared administrator password unexpectedly authenticated"),
    }
    assert!(ctx.read().await.roles.contains_key(quoted));
    // production_phase故意仍传空的旧jwks缓存；受管实现必须从spec本身识别JWT。
    // 验证真实目录/成员关系，防止“默认NOSUPERUSER”掩盖了隐式BYPASSRLS提权。
    let jwt=admin.query_one("SELECT rolsuper,rolbypassrls,rolcreaterole,rolcreatedb,rolreplication,pg_has_role('jwt_app','neon_superuser','MEMBER'),rolpassword IS NULL FROM pg_catalog.pg_authid WHERE rolname='jwt_app'",&[]).await?;
    for i in 0..6 {
        assert!(!jwt.get::<_, bool>(i));
    }
    assert!(jwt.get::<_, bool>(6));
    let row=admin.query_one("SELECT rolsuper,rolcanlogin,rolcreatedb,rolreplication,xmin::text FROM pg_catalog.pg_authid WHERE rolname='dsg_app'",&[]).await?;
    assert!(!row.get::<_, bool>(0));
    assert!(row.get::<_, bool>(1));
    assert!(row.get::<_, bool>(2));
    assert!(row.get::<_, bool>(3));
    let xmin: String = row.get(4);
    production_phase(&private, &spec, ctx.clone()).await?;
    assert_eq!(
        admin
            .query_one(
                "SELECT xmin::text FROM pg_catalog.pg_authid WHERE rolname='dsg_app'",
                &[]
            )
            .await?
            .get::<_, String>(0),
        xmin
    );
    let old=admin.query_one("SELECT rolcanlogin,rolcreatedb,pg_has_role('existing_app','neon_superuser','MEMBER') FROM pg_catalog.pg_authid WHERE rolname='existing_app'",&[]).await?;
    assert!(!old.get::<_, bool>(0) && !old.get::<_, bool>(1) && !old.get::<_, bool>(2));
    spec.cluster.roles[1].options = Some(vec![
        option("NOCREATEDB", None),
        option("NOREPLICATION", None),
    ]);
    production_phase(&private, &spec, ctx.clone()).await?;
    let row = admin
        .query_one(
            "SELECT rolcreatedb,rolreplication FROM pg_catalog.pg_authid WHERE rolname='dsg_app'",
            &[],
        )
        .await?;
    assert!(!row.get::<_, bool>(0) && !row.get::<_, bool>(1));
    // 测试桥只安排真实内核准入状态，不冒充生产预算/VM完成。打开后验证实际
    // SET ROLE/SET SESSION AUTHORIZATION权限，关闭后验证既有连接的新SQL被拦截。
    admin.batch_execute("CREATE FUNCTION public.role_boot() RETURNS bigint AS '$libdir/test_dsg_region','test_dsg_boot_id' LANGUAGE C; CREATE FUNCTION public.role_admit(bigint,bigint,bigint,bool) RETURNS bool AS '$libdir/test_dsg_region','test_dsg_admission' LANGUAGE C STRICT; SELECT role_admit(role_boot(),1,1,true);").await?;
    let app = cluster
        .connect_as("dsg_app", Some("Tenant-native-fixture-only-2026"))
        .await?;
    assert_eq!(app.query_one("SELECT 1", &[]).await?.get::<_, i32>(0), 1);
    for command in [
        "SELECT rolpassword FROM pg_catalog.pg_authid WHERE rolname='cloud_admin'",
        "SELECT a FROM pg_catalog.pg_authid a",
        "SELECT passwd FROM pg_catalog.pg_shadow WHERE usename='cloud_admin'",
    ] {
        match app.query(command, &[]).await {
            Err(error) => assert_eq!(error.code().map(|c| c.code()), Some("42501")),
            Ok(_) => panic!("ordinary production weak role can read authentication secrets"),
        }
    }
    assert_eq!(
        app.query_one(
            "SELECT rolpassword FROM pg_catalog.pg_roles WHERE rolname='cloud_admin'",
            &[]
        )
        .await?
        .get::<_, String>(0),
        "********"
    );
    admin.batch_execute("CREATE FUNCTION public.role_secret_definer() RETURNS text LANGUAGE sql SECURITY DEFINER AS $$ SELECT rolpassword FROM pg_catalog.pg_authid WHERE rolname='cloud_admin' $$").await?;
    assert_eq!(
        app.query_one("SELECT public.role_secret_definer()", &[])
            .await
            .unwrap_err()
            .code()
            .map(|c| c.code()),
        Some("42501")
    );
    admin.batch_execute("CREATE FUNCTION public.role_escalate_definer() RETURNS void LANGUAGE sql SECURITY DEFINER AS $$ UPDATE pg_catalog.pg_authid SET rolsuper=true WHERE rolname='dsg_app' $$").await?;
    assert_eq!(
        app.query_one("SELECT public.role_escalate_definer()", &[])
            .await
            .unwrap_err()
            .code()
            .map(|c| c.code()),
        Some("42501")
    );
    for command in [
        "SET ROLE cloud_admin",
        "SET SESSION AUTHORIZATION cloud_admin",
        "ALTER ROLE cloud_admin PASSWORD 'must-never-authenticate'",
        "ALTER ROLE dsg_app SUPERUSER",
        "CREATE ROLE forbidden_superuser SUPERUSER",
        "UPDATE pg_catalog.pg_authid SET rolsuper=true WHERE rolname='dsg_app'",
        "GRANT cloud_admin TO dsg_app",
        "GRANT pg_read_server_files TO dsg_app",
        "GRANT pg_execute_server_program TO dsg_app",
    ] {
        let error = app.batch_execute(command).await.unwrap_err();
        assert_eq!(error.code().map(|c| c.code()), Some("42501"));
    }
    app.batch_execute("SET ROLE neon_superuser").await?;
    admin
        .batch_execute("SELECT role_admit(role_boot(),2,1,false)")
        .await?;
    assert!(app.query_one("SELECT 1", &[]).await.is_err());
    assert_eq!(
        private.query_one("SELECT 1", &[]).await?.get::<_, i32>(0),
        1
    );
    admin
        .batch_execute("GRANT cloud_admin TO bridge_role; GRANT bridge_role TO dsg_app")
        .await?;
    let before = ctx.read().await.roles.clone();
    assert!(
        production_phase(&private, &spec, ctx.clone())
            .await
            .is_err()
    );
    assert_eq!(ctx.read().await.roles, before);
    admin
        .batch_execute("REVOKE bridge_role FROM dsg_app; ALTER ROLE dsg_app SUPERUSER")
        .await?;
    assert!(
        production_phase(&private, &spec, ctx.clone())
            .await
            .is_err()
    );
    assert!(
        admin
            .query_one(
                "SELECT rolsuper FROM pg_catalog.pg_authid WHERE rolname='dsg_app'",
                &[]
            )
            .await?
            .get::<_, bool>(0)
    );
    // 原来用超级用户认证的会话不能靠ALTER NOSUPERUSER修复，故保留越权事实并
    // 拒绝，交给后续受控重建；本测试关闭整个独占集群，不把降权冒充安全恢复。
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicit accurate PG17 bin/SHA and installed test libraries"]
async fn native_fault_rollback_lock_deadline_and_session_identity() -> Result<()> {
    let (mut cluster, admin, private, mut spec) = native_setup().await?;
    let original: Option<String> = admin
        .query_one(
            "SELECT rolpassword FROM pg_catalog.pg_authid WHERE rolname='cloud_admin'",
            &[],
        )
        .await?
        .get(0);
    assert!(original.is_some()); // 合成旧口令只用于验证事务失败不会部分清除。
    spec.cluster.roles[0].encrypted_password = Some(verifier());
    spec.cluster.roles[1].name = "rollback_witness".into();
    // 仅测试在生成函数的最终END前注入服务端除零故障；实际函数内角色DDL已
    // 执行但尚未提交，公共执行器必须失败，密码与先前DDL都回滚。
    let mut sql = query(&spec, "neon_superuser")?;
    let end = sql.definition.rfind("END;").unwrap();
    sql.definition.insert_str(end, "PERFORM 1/0;\n");
    assert!(execute_and_snapshot(&private, &sql).await.is_err());
    let row=admin.query_one("SELECT (SELECT rolpassword FROM pg_catalog.pg_authid WHERE rolname='cloud_admin'),EXISTS(SELECT FROM pg_catalog.pg_authid WHERE rolname='rollback_witness')",&[]).await?;
    assert_eq!(row.get::<_, Option<String>>(0), original);
    assert!(!row.get::<_, bool>(1));
    spec.cluster.roles[0].encrypted_password = None;
    let ctx = context();
    admin.batch_execute("SET ROLE cloud_admin").await?;
    assert!(production_phase(&admin, &spec, ctx.clone()).await.is_err());
    admin.batch_execute("RESET ROLE; CREATE ROLE observer LOGIN IN ROLE neon_superuser; CREATE FUNCTION public.role_boot() RETURNS bigint AS '$libdir/test_dsg_region','test_dsg_boot_id' LANGUAGE C; CREATE FUNCTION public.role_admit(bigint,bigint,bigint,bool) RETURNS bool AS '$libdir/test_dsg_region','test_dsg_admission' LANGUAGE C STRICT; SELECT role_admit(role_boot(),1,1,true)").await?;
    let observer = cluster.connect_as("observer", None).await?;
    let backend: i32 = private
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    admin
        .batch_execute("BEGIN; LOCK TABLE pg_catalog.pg_auth_members IN ROW EXCLUSIVE MODE")
        .await?;
    let reading = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let guard = tokio::time::timeout(Duration::from_millis(100), ctx.read()).await?;
        assert!(guard.roles.is_empty());
        drop(guard);
        let visible: String = observer
            .query_one(
                "SELECT query FROM pg_catalog.pg_stat_activity WHERE pid=$1",
                &[&backend],
            )
            .await?
            .get(0);
        assert!(
            visible.contains("$1") && !visible.contains("SCRAM-SHA-256$"),
            "private role verifier escaped into SQL activity"
        );
        Result::<()>::Ok(())
    };
    let (result, read_result) =
        tokio::join!(production_phase(&private, &spec, ctx.clone()), reading);
    read_result?;
    let error = result.unwrap_err().to_string();
    assert!(error.contains("55P03"), "{error}");
    assert!(ctx.read().await.roles.is_empty());
    admin.batch_execute("ROLLBACK").await?;
    admin
        .batch_execute(
            "CREATE ROLE unknown_operator SUPERUSER PASSWORD 'synthetic-operator-password'",
        )
        .await?;
    assert!(
        production_phase(&private, &spec, ctx.clone())
            .await
            .is_err()
    );
    assert!(ctx.read().await.roles.is_empty());
    admin.batch_execute("DROP ROLE unknown_operator").await?;
    production_phase(&private, &spec, ctx.clone()).await?;
    assert!(
        admin
            .query_one(
                "SELECT rolpassword IS NULL FROM pg_catalog.pg_authid WHERE rolname='cloud_admin'",
                &[]
            )
            .await?
            .get::<_, bool>(0)
    );
    assert!(ctx.read().await.roles.contains_key("rollback_witness"));
    cluster.passed = true;
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicit accurate PG17 bin/SHA and installed test libraries"]
async fn native_replica_attests_replayed_roles_without_catalog_writes() -> Result<()> {
    let (mut primary, admin, private, mut spec) = native_setup().await?;
    production_phase(&private, &spec, context()).await?;
    let mut standby = primary.standby(|data| crate::dsg_hba::initialize_socket(data, 55432))?;
    spec.mode = ComputeMode::Replica;
    spec.skip_pg_catalog_updates = true;
    let replica = standby.connect_as(ADMIN, None).await?;
    assert!(
        replica
            .query_one("SELECT pg_is_in_recovery()", &[])
            .await?
            .get::<_, bool>(0)
    );
    attest_read_only(&replica, &spec, "neon_superuser").await?;
    // 真实调用生产Compute的RO入口，包括其专用连接，而不是只测一个未接线函数。
    let mut params = parameters();
    params.pgdata = standby.data.to_string_lossy().into_owned();
    params
        .connstr
        .query_pairs_mut()
        .append_pair("host", standby.socket.to_str().unwrap())
        .append_pair("port", "55432");
    let node = crate::compute::ComputeNode::new(
        params,
        compute_api::responses::ComputeConfig {
            spec: Some(spec.clone()),
            compute_ctl_config: compute_api::responses::ComputeCtlConfig {
                jwks: jsonwebtoken::jwk::JwkSet { keys: vec![] },
                tls: None,
            },
        },
    )?;
    let source = spec.clone();
    tokio::task::spawn_blocking(move || node.attest_dsg_read_only_roles(&source)).await??;
    let mut wrong = spec.clone();
    wrong.cluster.roles[1].encrypted_password = Some(verifier());
    assert!(
        attest_read_only(&replica, &wrong, "neon_superuser")
            .await
            .is_err()
    );
    assert!(
        replica
            .batch_execute("CREATE ROLE must_not_be_created")
            .await
            .is_err()
    );
    assert!(!admin.query_one("SELECT EXISTS(SELECT FROM pg_catalog.pg_authid WHERE rolname='must_not_be_created')",&[]).await?.get::<_,bool>(0));
    admin
        .batch_execute(
            "CREATE ROLE ro_bridge; GRANT cloud_admin TO ro_bridge; GRANT ro_bridge TO dsg_app",
        )
        .await?;
    let lsn: String = admin
        .query_one("SELECT pg_current_wal_flush_lsn()::text", &[])
        .await?
        .get(0);
    let mut caught_up = false;
    for _ in 0..100 {
        if replica
            .query_one(
                "SELECT pg_last_wal_replay_lsn() >= $1::text::pg_lsn",
                &[&lsn],
            )
            .await?
            .get::<_, bool>(0)
        {
            caught_up = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    ensure!(caught_up, "physical standby did not replay role mutation");
    assert!(
        attest_read_only(&replica, &spec, "neon_superuser")
            .await
            .is_err()
    );
    standby.passed = true;
    primary.passed = true;
    Ok(())
}
