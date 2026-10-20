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

    pub(crate) async fn connect(&self) -> Result<Client> {
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
