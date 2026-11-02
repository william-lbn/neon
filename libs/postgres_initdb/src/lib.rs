//! The canonical way we run `initdb` in Neon.
//!
//! initdb has implicit defaults that are dependent on the environment, e.g., locales & collations.
//!
//! This module's job is to eliminate the environment-dependence as much as possible.

use std::fmt;

use camino::Utf8Path;
use postgres_versioninfo::PgMajorVersion;

pub struct RunInitdbArgs<'a> {
    pub superuser: &'a str,
    pub locale: &'a str,
    pub initdb_bin: &'a Utf8Path,
    pub pg_version: PgMajorVersion,
    pub library_search_path: &'a Utf8Path,
    pub pgdata: &'a Utf8Path,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    Spawn(std::io::Error),
    Failed {
        status: std::process::ExitStatus,
        stderr: Vec<u8>,
    },
    WaitOutput(std::io::Error),
    Other(anyhow::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Spawn(e) => write!(f, "Error spawning command: {e:?}"),
            Error::Failed { status, stderr } => write!(
                f,
                "Command failed with status {:?}: {}",
                status,
                String::from_utf8_lossy(stderr)
            ),
            Error::WaitOutput(e) => write!(f, "Error waiting for command output: {e:?}"),
            Error::Other(e) => write!(f, "Error: {e:?}"),
        }
    }
}

// Neon现有远端WAL/redo页合同使用checksum profile 0。PG18把initdb默认改为1，
// 新目录必须显式选择已支持格式；不修改旧目录，也不放宽页头/WAL/控制文件校验。
fn configure_neon_storage_checksums(
    command: &mut tokio::process::Command,
    pg_version: PgMajorVersion,
) {
    match pg_version {
        PgMajorVersion::PG18 => {
            command.arg("--no-data-checksums");
        }
        PgMajorVersion::PG14
        | PgMajorVersion::PG15
        | PgMajorVersion::PG16
        | PgMajorVersion::PG17 => {}
    }
}

pub async fn do_run_initdb(args: RunInitdbArgs<'_>) -> Result<(), Error> {
    let RunInitdbArgs {
        superuser,
        locale,
        initdb_bin: initdb_bin_path,
        pg_version,
        library_search_path,
        pgdata,
    } = args;
    let mut initdb_command = tokio::process::Command::new(initdb_bin_path);
    initdb_command
        .args(["--pgdata", pgdata.as_ref()])
        .args(["--username", superuser])
        .args(["--encoding", "utf8"])
        .args(["--locale", locale])
        .arg("--no-instructions")
        .arg("--no-sync")
        .env_clear()
        .env("LD_LIBRARY_PATH", library_search_path)
        .env("DYLD_LIBRARY_PATH", library_search_path)
        .env(
            "ASAN_OPTIONS",
            std::env::var("ASAN_OPTIONS").unwrap_or_default(),
        )
        .env(
            "UBSAN_OPTIONS",
            std::env::var("UBSAN_OPTIONS").unwrap_or_default(),
        )
        .stdin(std::process::Stdio::null())
        // stdout invocation produces the same output every time, we don't need it
        .stdout(std::process::Stdio::null())
        // we would be interested in the stderr output, if there was any
        .stderr(std::process::Stdio::piped());

    // Before version 14, only the libc provide was available.
    if pg_version > PgMajorVersion::PG14 {
        // Version 17 brought with it a builtin locale provider which only provides
        // C and C.UTF-8. While being safer for collation purposes since it is
        // guaranteed to be consistent throughout a major release, it is also more
        // performant.
        let locale_provider = if pg_version >= PgMajorVersion::PG17 {
            "builtin"
        } else {
            "libc"
        };

        initdb_command.args(["--locale-provider", locale_provider]);
    }

    configure_neon_storage_checksums(&mut initdb_command, pg_version);

    let initdb_proc = initdb_command.spawn().map_err(Error::Spawn)?;

    // Ideally we'd select here with the cancellation token, but the problem is that
    // we can't safely terminate initdb: it launches processes of its own, and killing
    // initdb doesn't kill them. After we return from this function, we want the target
    // directory to be able to be cleaned up.
    // See https://github.com/neondatabase/neon/issues/6385
    let initdb_output = initdb_proc
        .wait_with_output()
        .await
        .map_err(Error::WaitOutput)?;
    if !initdb_output.status.success() {
        return Err(Error::Failed {
            status: initdb_output.status,
            stderr: initdb_output.stderr,
        });
    }

    Ok(())
}

#[cfg(test)]
mod checksum_profile_tests {
    use super::*;

    fn arguments(version: PgMajorVersion) -> Vec<String> {
        let mut command = tokio::process::Command::new("initdb");
        command.args(["--encoding", "utf8", "--locale", "C", "--no-sync"]);
        configure_neon_storage_checksums(&mut command, version);
        command
            .as_std()
            .get_args()
            .map(|arg| arg.to_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn pg18_neon_storage_explicit_checksum_profile_zero() {
        let args = arguments(PgMajorVersion::PG18);
        assert_eq!(
            args,
            [
                "--encoding",
                "utf8",
                "--locale",
                "C",
                "--no-sync",
                "--no-data-checksums",
            ]
        );
        assert!(!args.iter().any(|arg| arg == "--data-checksums"));
    }

    #[test]
    fn legacy_neon_storage_initdb_arguments_unchanged() {
        for version in [
            PgMajorVersion::PG14,
            PgMajorVersion::PG15,
            PgMajorVersion::PG16,
            PgMajorVersion::PG17,
        ] {
            assert_eq!(
                arguments(version),
                ["--encoding", "utf8", "--locale", "C", "--no-sync"]
            );
        }
    }
}
