//! 受管PG的独立认证文件与私有管理通道。网络租户永不使用内部SU的本地trust。
//! 先hold再核验角色，之后才发布业务HBA；加载确认不能只看SIGHUP返回值。

use std::collections::BTreeSet;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use compute_api::spec::ComputeSpec;
use nix::unistd::{Uid, User};
use tokio_postgres::{Client, Config, NoTls};
use uuid::Uuid;

const SOCKET: &str = "dsg_socket";
const HBA: &str = "dsg_hba.conf";
const IDENT: &str = "dsg_ident.conf";
const OPTIONS: &str = "-c role=cloud_admin -c default_transaction_read_only=off -c search_path='' -c statement_timeout=0 -c pgaudit.log=none";

pub(crate) struct Receipt {
    nonce_database: String,
    hba: PathBuf,
    ident: PathBuf,
    hba_text: String,
    ident_text: String,
}

/// 字段用HBA自己的双引号语法，不用SQL字符串转义。引号也保护all/+/@/逗号等
/// 关键字符；不允许控制字符/换行注入。反斜线在引号内保留原义。
fn token(value: &str) -> Result<String> {
    ensure!(
        !value.is_empty() && !value.chars().any(char::is_control),
        "invalid DSGA HBA token"
    );
    Ok(format!("\"{}\"", value.replace('"', "\"\"")))
}

fn socket(pgdata: &Path) -> PathBuf {
    pgdata.join(SOCKET)
}

/// 上游URI解析器会追加host/port；旧连接可能先试默认5432再试query端口。
/// 单socket没有fallback，故接受一个单值query端口，拒绝重复/列表/冲突/0。
pub(crate) fn port(url: &url::Url) -> Result<u16> {
    let values = url
        .query_pairs()
        .filter(|(key, _)| key == "port")
        .map(|(_, v)| v.into_owned())
        .collect::<Vec<_>>();
    ensure!(values.len() <= 1, "DSGA PostgreSQL port cannot be repeated");
    let query = values
        .first()
        .map(|value| {
            ensure!(
                !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()),
                "DSGA PostgreSQL port must be one integer"
            );
            let port = value
                .parse::<u16>()
                .context("DSGA PostgreSQL port out of range")?;
            ensure!(port != 0, "DSGA PostgreSQL port cannot be zero");
            Ok::<_, anyhow::Error>(port)
        })
        .transpose()?;
    if let (Some(authority), Some(query)) = (url.port(), query) {
        ensure!(
            authority == query,
            "DSGA PostgreSQL port parameters conflict"
        );
    }
    let selected = query.or(url.port()).unwrap_or(5432);
    ensure!(selected != 0, "DSGA PostgreSQL port cannot be zero");
    Ok(selected)
}

pub(crate) fn startup_settings(pgdata: &Path, port: u16) -> Result<Vec<(&'static str, String)>> {
    ensure!(port != 0, "DSGA private PG port cannot be zero");
    ensure!(pgdata.is_absolute(), "DSGA PGDATA must be absolute");
    SocketAddr::from_pathname(socket(pgdata).join(format!(".s.PGSQL.{port}")))
        .context("DSGA private socket path exceeds platform limit")?;
    let text = |p: PathBuf| {
        p.into_os_string()
            .into_string()
            .map_err(|_| anyhow::anyhow!("DSGA paths require UTF-8"))
    };
    Ok(vec![
        ("hba_file", text(pgdata.join(HBA))?),
        ("ident_file", text(pgdata.join(IDENT))?),
        ("unix_socket_directories", token(&text(socket(pgdata))?)?),
        ("unix_socket_permissions", "0700".into()),
        ("unix_socket_group", String::new()),
    ])
}

/// 从零建立单一Unix目标，绝不在URL旧host/hostaddr/password后追加fallback。
/// getter只构造配置、不加state锁、不访问网络，适合已有配置锁下调用。
pub(crate) fn private_config(pgdata: &Path, port: u16, application: Option<&str>) -> Config {
    let mut config = Config::new();
    config
        .host_path(socket(pgdata))
        .port(port)
        .user("cloud_admin")
        .dbname("postgres")
        .options(OPTIONS);
    if let Some(application) = application {
        config.application_name(application);
    }
    config
}

/// vm-monitor/LFC也必须进入同一私有通道，不能在Linux继续用localhost SU trust。
/// URI不带host或密码，唯一host放在显式query中；用库编码路径/空格/引号。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn private_dsn(pgdata: &Path, port: u16) -> String {
    let mut url = url::Url::parse("postgresql:///postgres").expect("constant PostgreSQL URI");
    url.query_pairs_mut()
        .append_pair(
            "host",
            socket(pgdata).to_str().expect("validated UTF-8 path"),
        )
        .append_pair("port", &port.to_string())
        .append_pair("user", "cloud_admin")
        .append_pair("options", OPTIONS)
        .append_pair("application_name", "vm-monitor:DSGA");
    // query_pairs采用form编码，空格变为+；PostgreSQL URI参数只做百分号解码，
    // 不将+还原为空格。字面+已编码为%2B，故仅将生成的+改为%20。
    url.to_string().replace('+', "%20")
}

fn owned_directory(path: &Path) -> Result<()> {
    let info = fs::symlink_metadata(path)?;
    ensure!(
        info.is_dir()
            && !info.file_type().is_symlink()
            && info.uid() == Uid::effective().as_raw()
            && info.mode() & 0o777 == 0o700,
        "DSGA authentication directory must be owned, non-symlink and mode 0700"
    );
    Ok(())
}

/// 只用于新PGDATA、启动Postmaster之前。物理备份会复制socket目录中的普通
/// .lock文件；其PID在源机仍活着，不能让副本把它误判成自己的活动Postmaster。
/// 先检查完整目录和文件类型，再删除已知临时文件；未知文件/链接一律保留并
/// 拒绝启动。不按PID猜测远端进程，也不在在线hold/publish时删除活动socket。
pub(crate) fn initialize_socket(pgdata: &Path, port: u16) -> Result<()> {
    startup_settings(pgdata, port)?;
    owned_directory(pgdata)?;
    match fs::symlink_metadata(pgdata.join("postmaster.pid")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => bail!("DSGA socket initialization requires fresh PGDATA without postmaster.pid"),
    }
    match DirBuilder::new().mode(0o700).create(socket(pgdata)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    owned_directory(&socket(pgdata))?;
    let mut remove = Vec::new();
    for entry in fs::read_dir(socket(pgdata))? {
        ensure!(
            remove.len() < 64,
            "DSGA socket directory has unexpected contents"
        );
        let entry = entry?;
        let name = entry.file_name();
        let raw = name.to_str().context("invalid DSGA socket filename")?;
        let number = raw.strip_prefix(".s.PGSQL.").unwrap_or("");
        let number = number.strip_suffix(".lock").unwrap_or(number);
        ensure!(
            !number.is_empty()
                && number.bytes().all(|b| b.is_ascii_digit())
                && number.parse::<u16>().is_ok_and(|n| n != 0),
            "unknown DSGA socket file"
        );
        let kind = entry.file_type()?;
        ensure!(
            kind.is_file() || kind.is_socket(),
            "DSGA socket runtime entry cannot be a link or directory"
        );
        remove.push(entry.path());
    }
    for file in remove {
        fs::remove_file(file)?;
    }
    fs::File::open(socket(pgdata))?.sync_all()?;
    Ok(())
}

/// 同目录create_new临时文件+fsync+rename，旧symlink只被替换，绝不沿其目标写。
/// 任何失败不修改成功事实，不回退到备份HBA；父目录由PG所有者0700保护。
fn replace(pgdata: &Path, name: &str, content: &str) -> Result<()> {
    let pending = pgdata.join(format!(".dsg-auth-{}", Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&pending)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        fs::rename(&pending, pgdata.join(name))?;
        fs::File::open(pgdata)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&pending);
    }
    result
}

/// 调用方持受管配置专用锁；不能持state/ApplyContext锁跨后续网络等待。
/// 初始bootstrap/重配hold仅开放OS peer管理。publish只在实际角色已核验后调用。
/// JWT角色只给可信回环local_proxy免PG口令，普通业务角色仍强制SCRAM；内部及
/// 未声明身份不在白名单，local/TCP/physical replication均有明确终结拒绝规则。
pub(crate) fn prepare(
    pgdata: &Path,
    port: u16,
    spec: &ComputeSpec,
    publish: bool,
) -> Result<Receipt> {
    ensure!(
        spec.dynamic_shared_buffers.is_some() && spec.databricks_settings.is_none(),
        "unsupported DSGA authentication mode"
    );
    crate::dsg_roles::validate_spec(spec)?;
    startup_settings(pgdata, port)?;
    owned_directory(pgdata)?;
    match DirBuilder::new().mode(0o700).create(socket(pgdata)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    owned_directory(&socket(pgdata))?;
    let os_user = User::from_uid(Uid::effective())?
        .context("DSGA service OS user is unavailable")?
        .name;
    ensure!(
        !os_user.starts_with('/'),
        "DSGA peer user cannot be a regex"
    );
    let mut ident = String::from("# DSGA owned peer map; no includes or regex\n");
    for role in ["cloud_admin", "zenith_admin", "postgres"] {
        ident.push_str(&format!(
            "dsg_control {} {}\n",
            token(&os_user)?,
            token(role)?
        ));
    }
    // 每次刷新独立nonce：旧已加载HBA会允许管理身份到不存在库(3D000)，新HBA
    // 则在认证前明确拒绝(28000)。只用拒绝探针还不够，下面也验证新管理连接。
    let nonce_database = format!("dsg_hba_{}", Uuid::new_v4().simple());
    let mut hba = format!(
        "# DSGA owned; never include backup/client HBA\nlocal {} cloud_admin reject\nlocal all cloud_admin,zenith_admin,postgres peer map=dsg_control\nlocal replication cloud_admin,zenith_admin,postgres peer map=dsg_control\n",
        token(&nonce_database)?
    );
    if publish {
        let roles = spec
            .cluster
            .roles
            .iter()
            .map(|r| token(&r.name))
            .collect::<Result<Vec<_>>>()?
            .join(",");
        let jwt = spec
            .local_proxy_config
            .iter()
            .flat_map(|p| p.jwks.iter().flatten())
            .flat_map(|j| &j.role_names)
            .map(|s| token(s))
            .collect::<Result<BTreeSet<_>>>()?;
        if !jwt.is_empty() {
            let names = jwt.into_iter().collect::<Vec<_>>().join(",");
            hba.push_str(&format!(
                "host all {names} 127.0.0.1/32 trust\nhost all {names} ::1/128 trust\n"
            ));
        }
        hba.push_str(&format!(
            "local all {roles} scram-sha-256\nhost all {roles} all scram-sha-256\n"
        ));
    }
    hba.push_str("local all all reject\nlocal replication all reject\nhost all all all reject\nhost replication all all reject\n");
    replace(pgdata, IDENT, &ident)?;
    replace(pgdata, HBA, &hba)?;
    Ok(Receipt {
        nonce_database,
        hba: pgdata.join(HBA),
        ident: pgdata.join(IDENT),
        hba_text: hba,
        ident_text: ident,
    })
}

async fn connect(config: &Config) -> Result<Client> {
    let pair = tokio::time::timeout(Duration::from_secs(2), config.connect(NoTls)).await;
    let (client, driver) = match pair {
        Ok(Ok(pair)) => pair,
        _ => bail!("DSGA private peer connection unavailable"),
    };
    tokio::spawn(async move {
        let _ = driver.await;
    });
    Ok(client)
}

/// SIGHUP只证明信号发送。等新nonce规则实际在新连接上拒绝，同时重新证明正常
/// 管理连接可登录且实际hba/ident路径正确，才返回成功。上界内无证据则失败，
/// 不开放local_proxy/Running；已有连接、完整角色来源回执由外层继续负责。
pub(crate) async fn reload_and_verify(config: &Config, receipt: &Receipt) -> Result<()> {
    match tokio::time::timeout(Duration::from_secs(10), verify_active(config, receipt)).await {
        Ok(result) => result,
        Err(_) => bail!("DSGA authentication activation deadline exceeded"),
    }
}

async fn verify_active(config: &Config, receipt: &Receipt) -> Result<()> {
    let client = connect(config).await?;
    ensure!(
        client
            .query_one("SELECT pg_catalog.pg_reload_conf()", &[])
            .await?
            .get::<_, bool>(0),
        "DSGA authentication reload signal failed"
    );
    verify_loaded(config, receipt).await
}

/// 初始化GET只观察已成功发布的同一代认证文件，不发送SIGHUP、不改文件。
/// 精确文件内容 + 新nonce拒绝 + 新peer连接身份/路径共同证明已加载的原策略；
/// 外层还须在异步等待后比较同一Arc receipt，防止并发配置更换策略时拼接事实。
pub(crate) async fn verify_loaded(config: &Config, receipt: &Receipt) -> Result<()> {
    for (path, expected) in [
        (&receipt.hba, &receipt.hba_text),
        (&receipt.ident, &receipt.ident_text),
    ] {
        let metadata = fs::symlink_metadata(path)?;
        ensure!(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= 1024 * 1024,
            "DSGA authentication file is unsafe or oversized"
        );
        ensure!(
            fs::read_to_string(path)? == *expected,
            "DSGA authentication file differs from published policy"
        );
    }
    let mut probe = config.clone();
    probe.dbname(&receipt.nonce_database);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let response = tokio::time::timeout(Duration::from_millis(500), probe.connect(NoTls)).await;
        if let Ok(Err(error)) = response {
            if error.code().is_some_and(|code| code.code() == "28000") {
                break;
            }
        }
        ensure!(
            tokio::time::Instant::now() < deadline,
            "DSGA active authentication policy was not confirmed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let check = connect(config).await?;
    let row = check.query_one("SELECT session_user::text,current_user::text,current_setting('hba_file'),current_setting('ident_file')", &[]).await?;
    ensure!(
        row.get::<_, String>(0) == "cloud_admin"
            && row.get::<_, String>(1) == "cloud_admin"
            && Path::new(&row.get::<_, String>(2)) == receipt.hba
            && Path::new(&row.get::<_, String>(3)) == receipt.ident,
        "DSGA active authentication identity/path differs"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
