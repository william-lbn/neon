//! Linux受管PG出生入口。root帮助程序先核验当前持久约束、原位入组及完整降权，
//! 再exec固定PG；Compute持有的Child PID因此仍是实际PG，不引入sudo/代理父进程。

use std::process::Command;

use anyhow::{Result, ensure};

pub(crate) const RESOURCE_GUARD: &str = "/neonvm/bin/dsga-resource-guard";
pub(crate) const RESOURCE_CONFIG: &str = "/etc/neon-dsga/resource.json";
const POSTGRES: &str = "/usr/local/bin/postgres";

/// 原VM已进入受管模式时，缺少/错误spec不能绕回非受管出生。标记仅能收紧启动，
/// 其存在本身不认证身份/预算；真正启动仍由root程序读私有事实与实际内核复核。
pub(crate) fn validate_guest_mode(managed_spec: bool, marker_exists: bool) -> Result<()> {
    ensure!(
        !marker_exists || managed_spec,
        "managed Guest cannot start PG without the frozen DSGA spec"
    );
    Ok(())
}

/// 只构造固定argv，不执行shell、不从环境选择特权程序或配置路径。缺少程序、
/// SUID权限/约束/身份不符时启动失败，由正常PG启动错误路径报告，绝不回退cgexec。
pub(crate) fn linux_postgres_command(pgbin: &str) -> Result<Command> {
    ensure!(
        pgbin == POSTGRES,
        "managed Linux PG requires the fixed production binary path"
    );
    let mut command = Command::new(RESOURCE_GUARD);
    command.arg("launch-postgres");
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute::ComputeNode;
    use compute_api::responses::{ComputeConfig, ComputeCtlConfig};

    #[test]
    fn fixed_birth_command_cannot_select_other_privileged_programs() -> Result<()> {
        assert!(validate_guest_mode(false, true).is_err());
        for (spec, marker) in [(false, false), (true, false), (true, true)] {
            validate_guest_mode(spec, marker)?;
        }
        let mut command = linux_postgres_command(POSTGRES)?;
        command.args(["--sync-safekeepers"]);
        assert_eq!(command.get_program(), RESOURCE_GUARD);
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["launch-postgres", "--sync-safekeepers"]
        );
        for path in [
            "postgres",
            "/tmp/postgres",
            "/usr/local/bin/postgres --single",
        ] {
            assert!(linux_postgres_command(path).is_err());
        }
        Ok(())
    }

    #[test]
    fn managed_prewarm_does_not_allocate_an_unbudgeted_temporary_pool() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut parameters = crate::dsg_roles::tests::parameters();
        parameters.pgdata = temp
            .path()
            .join("uncreated-pgdata")
            .to_string_lossy()
            .into_owned();
        parameters.pgbin = "/nonexistent/postgres".into();
        let node = ComputeNode::new(
            parameters,
            ComputeConfig {
                spec: Some(crate::dsg_roles::tests::fixture()),
                compute_ctl_config: ComputeCtlConfig {
                    jwks: jsonwebtoken::jwk::JwkSet { keys: vec![] },
                    tls: None,
                },
            },
        )?;
        node.prewarm_postgres_vm_memory()?;
        assert!(!temp.path().join("uncreated-pgdata.warmup").exists());
        Ok(())
    }
}
