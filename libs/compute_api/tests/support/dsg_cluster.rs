//! 严格原生门禁共享的私有临时PG夹具，仅供测试；不接受现有数据库连接串。
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, ensure};
use tempfile::TempDir;
use tokio_postgres::{Client, NoTls};

pub(crate) const VM: &str = "dsg-vm-1";
pub(crate) const COMPUTE: &str = "dsg-compute-1";
pub(crate) const NAMESPACE: &str = "\"dsg-\"\"admin\"";

pub(crate) struct Cluster {
    directory: Option<TempDir>,
    pub(crate) bin: PathBuf,
    pub(crate) data: PathBuf,
    pub(crate) socket: PathBuf,
    pub(crate) catalog_pg18: bool,
    log: PathBuf,
    started: bool,
    pub(crate) passed: bool,
}

/// 此函数只比较两个独立输入的来源锁和原 ELF 字节摘要；源码到二进制的构建
/// 归属必须由调用脚本先从已有归档核验，不能由一个自报环境变量补造。
fn verify_explicit_elf_source_lock(
    expected_source: &str,
    recorded_source: &str,
    recorded_digest: &str,
    actual_digest: &str,
) -> Result<()> {
    ensure!(
        expected_source.len() == 40
            && expected_source.bytes().all(|b| b.is_ascii_hexdigit())
            && expected_source == recorded_source
            && recorded_digest.len() == 64
            && recorded_digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && recorded_digest == actual_digest,
        "test PG explicit ELF/source lock differs"
    );
    Ok(())
}

#[test]
fn explicit_elf_source_lock_requires_both_original_source_and_bytes() {
    let source = "a".repeat(40);
    let digest = "b".repeat(64);
    verify_explicit_elf_source_lock(&source, &source, &digest, &digest).unwrap();
    for (expected, recorded, declared, actual) in [
        (source.as_str(), "", digest.as_str(), digest.as_str()),
        (
            source.as_str(),
            "cccccccccccccccccccccccccccccccccccccccc",
            digest.as_str(),
            digest.as_str(),
        ),
        ("abc", "abc", digest.as_str(), digest.as_str()),
        (source.as_str(), source.as_str(), "", ""),
        (source.as_str(), source.as_str(), digest.as_str(), "bad"),
    ] {
        assert!(verify_explicit_elf_source_lock(expected, recorded, declared, actual).is_err());
    }
}

impl Cluster {
    pub(crate) fn new() -> Result<Self> {
        Self::new_with_growth_injections(true)
    }

    /// HBA/角色专项使用正式构建的 PG，不要求它启用 Grow 故障注入。仍启动
    /// 同源测试 Provider 与真实 Neon 扩展、校验 source/ELF、使用独占 0700
    /// 目录；只有此明确入口不 attach Grow 注入点，不能替代 Grow/资源资格。
    #[allow(dead_code)] // 共享夹具由多个测试 crate 编译，只有目录专项使用此入口。
    pub(crate) fn new_catalog() -> Result<Self> {
        Self::new_with_growth_injections(false)
    }

    fn new_with_growth_injections(attach_growth: bool) -> Result<Self> {
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
            version.status.success(),
            "test PG version observation failed"
        );
        if !String::from_utf8_lossy(&version.stdout).contains(&format!("({expected})")) {
            // 发行构建可能未把 VCS revision 编入 --version。仅测试夹具允许显式
            // ELF 锁定替代：调用方必须从原源码归档及构建清单核对 source SHA 和
            // 原 ELF 摘要，不能把此处临时自算的摘要反过来当作源码授权。
            // 未提供完整锁时仍保持原拒绝；生产 Compute/Guest 没有这条分支。
            let source = std::env::var("NEON_TEST_DSG_EXPECTED_PG_SOURCE_SHA")
                .context("test PG binary does not match source lock")?;
            let digest = std::env::var("NEON_TEST_DSG_EXPECTED_ELF_SHA256")
                .context("test PG binary does not match source lock")?;
            let hash = Command::new("/usr/bin/sha256sum")
                .arg(bin.join("postgres"))
                .output()
                .context("explicit ELF source lock requires native sha256sum")?;
            ensure!(hash.status.success(), "test PG ELF digest unavailable");
            let output = String::from_utf8(hash.stdout)?;
            let observed = output.split_whitespace().next().unwrap_or("");
            verify_explicit_elf_source_lock(&expected, &source, &digest, observed)?;
        }
        let version_text = String::from_utf8_lossy(&version.stdout);
        let catalog_pg18 = !attach_growth && version_text.starts_with("postgres (PostgreSQL) 18.");
        ensure!(
            attach_growth || catalog_pg18 || version_text.starts_with("postgres (PostgreSQL) 17."),
            "catalog fixture requires exact PG17 or PG18"
        );
        let provider = if catalog_pg18 {
            "test_dsg_region18"
        } else {
            "test_dsg_region"
        };
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
            catalog_pg18,
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
        // 生产getter现在只走PGDATA内私有socket。外部socket仍供原生测试管理/
        // 故障安排；双socket仅为夹具，生产Postmaster固定为唯一私有目录。
        let private_socket = cluster.data.join("dsg_socket");
        std::fs::create_dir(&private_socket)?;
        std::fs::set_permissions(&private_socket, std::fs::Permissions::from_mode(0o700))?;
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
             shared_preload_libraries='{provider},neon'\nwal_consistency_checking='neon'\n\
             neon.last_written_lsn_cache_size=1024\nneon.dsg_vm_uid='{VM}'\n\
             neon.dsg_compute_instance_id='{COMPUTE}'\nenable_dynamic_shared_buffers=on\n\
             shared_buffers='2MB'\nmax_shared_buffers='64MB'\n"
            ) + &format!(
                "unix_socket_directories='{socket},{}'\n",
                private_socket
                    .to_str()
                    .unwrap()
                    .replace('\\', "\\\\")
                    .replace('\'', "''")
            ),
        )?;
        cluster.start()?;
        let mut initialize = format!(
            "CREATE SCHEMA {NAMESPACE}; CREATE EXTENSION neon SCHEMA {NAMESPACE} VERSION '1.8';"
        );
        if attach_growth {
            initialize.push_str("CREATE FUNCTION public.dsg_control(text) RETURNS text AS '$libdir/test_dsg_region',
                'test_dsg_grow_control' LANGUAGE C STRICT;SELECT public.dsg_control('attach');");
        }
        cluster.command(
            "psql",
            &[
                "-h",
                cluster.socket.to_str().unwrap(),
                "-p",
                "55432",
                "-U",
                "dsg_admin",
                "-d",
                "postgres",
                "-X",
                "-v",
                "ON_ERROR_STOP=1",
                "-c",
                &initialize,
            ],
        )?;
        Ok(cluster)
    }

    fn command(&self, executable: &str, arguments: &[&str]) -> Result<()> {
        let mut command = Command::new(self.bin.join(executable));
        command.args(arguments);
        if self.catalog_pg18 && executable == "pg_ctl" {
            // 原 PG18 TEST_ONLY 四数组入口，8192 个块精确对应本夹具 64MiB
            // 最大值。只传给独占 Postmaster 启动，initdb 不继承；这不构成
            // Root/Guest 出生资格，不能被普通业务配置或资源资格测试复用。
            command.env("NEON_DSGA_PG18_NATIVE_ARRAYS", "test-v1:8192");
        }
        let output = command.output()?;
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

    pub(crate) fn restart(&mut self) -> Result<()> {
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

    /// 真实物理备库专项：仅从当前夹具的独占socket做basebackup；创建第二个
    /// 0700目录及独立socket，不接触用户数据库，不以只读GUC模拟恢复状态。
    #[allow(dead_code)] // 该共享夹具在多个测试crate编译，只有角色RO门禁调用此方法。
    pub(crate) fn standby(
        &self,
        prepare: impl FnOnce(&std::path::Path) -> Result<()>,
    ) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("neon-dsg-ro-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()?;
        let root = directory.path();
        let mut standby = Self {
            bin: self.bin.clone(),
            catalog_pg18: self.catalog_pg18,
            data: root.join("data"),
            socket: root.join("socket"),
            log: root.join("postgres.log"),
            directory: Some(directory),
            started: false,
            passed: false,
        };
        std::fs::create_dir(&standby.socket)?;
        standby.command(
            "pg_basebackup",
            &[
                "-h",
                self.socket.to_str().unwrap(),
                "-p",
                "55432",
                "-U",
                "dsg_admin",
                "-D",
                standby.data.to_str().unwrap(),
                "-X",
                "stream",
                "--no-slot",
                "-R",
                "-c",
                "fast",
            ],
        )?;
        let socket = standby
            .socket
            .to_str()
            .unwrap()
            .replace('\\', "\\\\")
            .replace('\'', "''");
        let config = standby.data.join("postgresql.conf");
        let private_socket = standby.data.join("dsg_socket");
        std::fs::create_dir_all(&private_socket)?;
        std::fs::set_permissions(&private_socket, std::fs::Permissions::from_mode(0o700))?;
        let previous = std::fs::read_to_string(&config)?;
        std::fs::write(
            config,
            format!(
                "{previous}\nunix_socket_directories='{socket},{}'\n",
                private_socket
                    .to_str()
                    .unwrap()
                    .replace('\\', "\\\\")
                    .replace('\'', "''")
            ),
        )?;
        // 让调用者在真实物理拷贝上执行产品初始化；复制来的socket lock不能
        // 在夹具中悄悄删除，否则会掩盖生产RO启动失败。
        prepare(&standby.data)?;
        standby.start()?;
        Ok(standby)
    }

    pub(crate) async fn connect(&self) -> Result<Client> {
        self.connect_as("dsg_admin", None).await
    }

    /// 角色隔离门禁复用同一独占socket，允许验证真正的登录身份和SCRAM密码；
    /// 不通过SET ROLE冒充另一个session_user。既有connect仍使用原测试管理身份。
    pub(crate) async fn connect_as(&self, user: &str, password: Option<&str>) -> Result<Client> {
        let mut config = tokio_postgres::Config::new();
        config
            .host_path(&self.socket)
            .port(55432)
            .user(user)
            .dbname("postgres");
        if let Some(password) = password {
            config.password(password);
        }
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
