//! 原生HTTP门禁共享夹具。只创建一次性密钥/私有临时PG，不读取用户数据库或凭据。
use anyhow::Result;
use axum::Router;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use compute_api::{
    dsg_auth::{DSG_HTTP_AUDIENCE, DSG_HTTP_SCOPE},
    dynamic_shared_buffers::DynamicSharedBuffersSpec,
    responses::{ComputeConfig, ComputeCtlConfig, ComputeStatus},
    spec::ComputeSpec,
};
use compute_tools::{
    compute::{ComputeNode, ComputeNodeParams},
    http::server::Server,
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, jwk::JwkSet};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::AtomicU64};
use std::time::{SystemTime, UNIX_EPOCH};
#[path = "../../../libs/compute_api/tests/support/dsg_cluster.rs"]
mod dsg_cluster;
pub(crate) use dsg_cluster::{COMPUTE, Cluster, VM};
pub(crate) const NODE: &str = "dsg-node/1";
/// 每个测试生成一次性签名密钥，不读取用户凭据/环境私钥。
pub(crate) struct Signer {
    pub(crate) encoding: EncodingKey,
    pair: Ed25519KeyPair,
    pub(crate) jwks: JwkSet,
}

impl Signer {
    pub(crate) fn new() -> Self {
        let private = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(private.as_ref()).unwrap();
        let jwks = serde_json::from_value(json!({"keys":[{
            "kty":"OKP", "crv":"Ed25519", "alg":"EdDSA", "use":"sig", "kid":"dsg-test",
            "x": URL_SAFE_NO_PAD.encode(pair.public_key().as_ref())
        }]}))
        .unwrap();
        Self {
            encoding: EncodingKey::from_ed_der(private.as_ref()),
            pair,
            jwks,
        }
    }

    pub(crate) fn claims(&self) -> Value {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        json!({"aud":DSG_HTTP_AUDIENCE,"scope":DSG_HTTP_SCOPE,"compute_id":NODE,
            "vm_uid":VM,"compute_instance_id":COMPUTE,"issuer_epoch":"1",
            "nbf":now-1,"exp":now+120})
    }

    pub(crate) fn token(&self, claims: &Value) -> String {
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some("dsg-test".into());
        encode(&header, claims, &self.encoding).unwrap()
    }

    pub(crate) fn raw_token(&self, body: &str) -> String {
        let message = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","kid":"dsg-test"}"#),
            URL_SAFE_NO_PAD.encode(body)
        );
        format!(
            "{message}.{}",
            URL_SAFE_NO_PAD.encode(self.pair.sign(message.as_bytes()).as_ref())
        )
    }
}

pub(crate) fn node(jwks: JwkSet, managed: bool, cluster: Option<&Cluster>) -> Arc<ComputeNode> {
    let mut connstr: url::Url = "postgres://cloud_admin@localhost/postgres".parse().unwrap();
    if let Some(cluster) = cluster {
        connstr
            .query_pairs_mut()
            .append_pair("host", cluster.socket.to_str().unwrap())
            .append_pair("port", "55432");
    }
    let params = ComputeNodeParams {
        compute_id: NODE.into(),
        connstr,
        privileged_role_name: "neon_superuser".into(),
        resize_swap_on_bind: false,
        set_disk_quota_for_fs: None,
        #[cfg(target_os = "linux")]
        filecache_connstr: String::new(),
        #[cfg(target_os = "linux")]
        cgroup: String::new(),
        #[cfg(target_os = "linux")]
        vm_monitor_addr: String::new(),
        // 无PG的协议测试不访问此目录；实际链路指向夹具真实私有socket。
        pgdata: cluster.map(|c| c.data.to_string_lossy().into_owned()).unwrap_or_else(|| "/tmp/dsg-http-unused".into()),
        pgbin: String::new(),
        pgversion: "17".into(),
        external_http_port: 0,
        internal_http_port: 0,
        remote_ext_base_url: None,
        installed_extensions_collection_interval: Arc::new(AtomicU64::new(3600)),
        // 显式走既有Hadron bypass条件，证明DSGA仍然必须验签。
        instance_id: Some("hadron-auth-bypass-must-not-apply".into()),
        pg_init_timeout: None,
        pg_isready_bin: String::new(),
        lakebase_mode: false,
        build_tag: "test".into(),
        control_plane_uri: None,
        config_path_test_only: None,
    };
    let mut spec: ComputeSpec = serde_json::from_str(include_str!("../cluster_spec.json")).unwrap();
    // 旧配置fixture故意含重复SK地址供另一个负向测试使用；本测试需要合法启动规格，
    // 但不会连接这些地址。不能为复用fixture而放宽生产ParsedSpec校验。
    spec.safekeeper_connstrings = vec!["127.0.0.1:6501".into(), "127.0.0.1:6502".into()];
    spec.dynamic_shared_buffers = managed.then(|| DynamicSharedBuffersSpec {
        protocol_version: 2,
        vm_uid: VM.into(),
        compute_instance_id: COMPUTE.into(),
        bootstrap_buffer_bytes: 2 * 1024 * 1024,
        maximum_buffer_bytes: 64 * 1024 * 1024,
        minimum_guest_memory_bytes: 128 * 1024 * 1024,
        non_buffer_reserve_bytes: 64 * 1024 * 1024,
    });
    if managed {
        // 旧示例含postgres超级用户/MD5，只服务旧路径；受管协议夹具显式提供
        // 普通应用角色，内部cloud_admin不能进入业务配置。这是合成测试值，不冒充实际
        // Renderer或数据库角色已应用；真实角色/SCRAM由独立原生门禁验证。
        spec.cluster.roles = serde_json::from_value(serde_json::json!([
            {"name":"dsg_app","encrypted_password":null,"options":null}
        ])).unwrap();
        for db in &mut spec.cluster.databases {
            db.owner = "dsg_app".into();
        }
        spec.delta_operations = None;
    }
    let compute = ComputeNode::new(
        params,
        ComputeConfig {
            spec: Some(spec),
            compute_ctl_config: ComputeCtlConfig { jwks, tls: None },
        },
    )
    .unwrap();
    compute.state.lock().unwrap().status = ComputeStatus::Running;
    Arc::new(compute)
}

pub(crate) fn router(node: &Arc<ComputeNode>) -> Router {
    Router::from(&Server::External {
        port: 0,
        config: node.compute_ctl_config.clone(),
        compute_id: NODE.into(),
        instance_id: node.params.instance_id.clone(),
    })
    .with_state(node.clone())
}

pub(crate) async fn native(signer: &Signer) -> Result<(Cluster, Arc<ComputeNode>, Router)> {
    let cluster = Cluster::new()?;
    cluster
        .connect()
        .await?
        .batch_execute(
            "CREATE ROLE cloud_admin SUPERUSER LOGIN; \
        CREATE FUNCTION public.dsg_hold_guard() RETURNS boolean AS '$libdir/test_dsg_region', \
            'test_dsg_control_hold_guard' LANGUAGE C",
        )
        .await?;
    let compute = node(signer.jwks.clone(), true, Some(&cluster));
    let app = router(&compute);
    Ok((cluster, compute, app))
}

/// 新模块专用：实际启动require-connection-gate与0700端点，创建新的Compute绑定。
/// 旧partial夹具不因字段形状相同就当作有持续门禁；不加载用户PGDATA。
pub(crate) async fn native_connections(signer: &Signer) -> Result<(Cluster, Arc<ComputeNode>, Router)> {
    use std::io::Write;
    let (mut cluster, _, _) = native(signer).await?;
    cluster.connect().await?.batch_execute("ALTER EXTENSION neon UPDATE TO '1.13'").await?;
    let mut conf = std::fs::OpenOptions::new().append(true).open(cluster.data.join("postgresql.conf"))?;
    writeln!(conf, "dynamic_shared_buffers_require_admission=on\ndynamic_shared_buffers_startup_budget='64MB'\ndynamic_shared_buffers_require_connection_gate=on\nunix_socket_permissions=0700")?;
    drop(conf);
    cluster.restart()?;
    // 管理夹具也切换私有端点；不能为了沿用旧公共管理socket而绕开真实门禁。
    cluster.socket = cluster.data.join("dsg_socket");
    let compute = node(signer.jwks.clone(), true, Some(&cluster));
    let app = router(&compute);
    Ok((cluster, compute, app))
}

/// Prepared1专用真实保留槽：旧原生API开门→真正PREPARE→关门，不伪造响应body。
/// 合成平台管理事务仅供隔离测试，不授权伸缩器替客户选择提交或回滚。
pub(crate) async fn native_prepared(signer: &Signer) -> Result<(Cluster, Arc<ComputeNode>, Router)> {
    use std::io::Write;
    let (mut cluster, _, _) = native_connections(signer).await?;
    cluster.connect().await?.batch_execute("ALTER EXTENSION neon UPDATE TO '1.14'").await?;
    let mut conf = std::fs::OpenOptions::new().append(true).open(cluster.data.join("postgresql.conf"))?;
    writeln!(conf, "max_prepared_transactions=4")?;
    drop(conf);
    cluster.restart()?;
    let sql = cluster.connect().await?;
    sql.batch_execute("CREATE ROLE dsg_app LOGIN; CREATE TABLE public.prepared_native(i int); \
        CREATE FUNCTION public.prepared_test_boot() RETURNS bigint AS '$libdir/test_dsg_region', 'test_dsg_boot_id' LANGUAGE C; \
        CREATE FUNCTION public.prepared_test_gate(bigint,bigint,bigint,boolean) RETURNS boolean AS '$libdir/test_dsg_region', 'test_dsg_admission' LANGUAGE C STRICT").await?;
    sql.simple_query("SELECT public.prepared_test_gate(public.prepared_test_boot(),1,1,true)").await?;
    sql.simple_query("BEGIN; INSERT INTO public.prepared_native VALUES(42); PREPARE TRANSACTION 'native_retained'").await?;
    sql.simple_query("SELECT public.prepared_test_gate(public.prepared_test_boot(),2,1,false)").await?;
    let compute = node(signer.jwks.clone(), true, Some(&cluster));
    let app = router(&compute);
    Ok((cluster, compute, app))
}
