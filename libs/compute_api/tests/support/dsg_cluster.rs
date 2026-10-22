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
    log: PathBuf,
    started: bool,
    pub(crate) passed: bool,
}

impl Cluster {
    pub(crate) fn new() -> Result<Self> {
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
                "CREATE SCHEMA {NAMESPACE}; CREATE EXTENSION neon SCHEMA {NAMESPACE} VERSION '1.8';\
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
    pub(crate) fn standby(&self) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("neon-dsg-ro-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()?;
        let root = directory.path();
        let mut standby = Self {
            bin: self.bin.clone(),
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
        let previous = std::fs::read_to_string(&config)?;
        std::fs::write(
            config,
            format!("{previous}\nunix_socket_directories='{socket}'\n"),
        )?;
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
