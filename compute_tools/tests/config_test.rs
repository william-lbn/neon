#[cfg(test)]
mod config_tests {

    use std::fs::{File, remove_file};
    use std::io::{Read, Write};
    use std::path::Path;

    use compute_tools::config::*;

    fn write_test_file(path: &Path, content: &str) {
        let mut file = File::create(path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }

    fn check_file_content(path: &Path, expected_content: &str) {
        let mut file = File::open(path).unwrap();
        let mut content = String::new();

        file.read_to_string(&mut content).unwrap();
        assert_eq!(content, expected_content);
    }

    #[test]
    fn test_line_in_file() {
        let path = Path::new("./tests/tmp/config_test.txt");
        write_test_file(path, "line1\nline2.1\t line2.2\nline3");

        let line = "line2.1\t line2.2";
        let result = line_in_file(path, line).unwrap();
        assert!(!result);
        check_file_content(path, "line1\nline2.1\t line2.2\nline3");

        let line = "line4";
        let result = line_in_file(path, line).unwrap();
        assert!(result);
        check_file_content(path, "line1\nline2.1\t line2.2\nline3\nline4");

        remove_file(path).unwrap();

        let path = Path::new("./tests/tmp/new_config_test.txt");
        let line = "line4";
        let result = line_in_file(path, line).unwrap();
        assert!(result);
        check_file_content(path, "line4");

        remove_file(path).unwrap();
    }
}

/// 验证真实配置文件生成路径，特别是恢复来的旧配置和失败前不破坏文件。
/// 不启动存储服务；Postmaster命令行优先级另由真实PG生命周期测试验证。
#[cfg(test)]
mod managed_bootstrap_tests {
    use std::path::PathBuf;
    use std::sync::{Arc, atomic::AtomicU64};

    use compute_api::{dynamic_shared_buffers::DynamicSharedBuffersSpec, spec::ComputeSpec};
    use compute_tools::{compute::ComputeNodeParams, config::write_postgres_conf};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("neon-config-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn write(&self, spec: &ComputeSpec) -> anyhow::Result<()> {
            let params = ComputeNodeParams {
                compute_id: "dsg-test".into(),
                connstr: "postgres://localhost/postgres".parse().unwrap(),
                privileged_role_name: "neon_superuser".into(),
                resize_swap_on_bind: false,
                set_disk_quota_for_fs: None,
                #[cfg(target_os = "linux")]
                filecache_connstr: String::new(),
                #[cfg(target_os = "linux")]
                cgroup: String::new(),
                #[cfg(target_os = "linux")]
                vm_monitor_addr: String::new(),
                pgdata: self.0.to_string_lossy().into_owned(),
                pgbin: String::new(),
                pgversion: "17".into(),
                external_http_port: 3080,
                internal_http_port: 3081,
                remote_ext_base_url: None,
                installed_extensions_collection_interval: Arc::new(AtomicU64::new(3600)),
                instance_id: None,
                pg_init_timeout: None,
                pg_isready_bin: String::new(),
                lakebase_mode: false,
                build_tag: "test".into(),
                control_plane_uri: None,
                config_path_test_only: None,
            };
            write_postgres_conf(&self.0, &params, spec, None, 3081, &None, None, false)
        }

        fn read(&self) -> String {
            std::fs::read_to_string(self.0.join("postgresql.conf")).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn spec() -> ComputeSpec {
        let mut spec: ComputeSpec =
            serde_json::from_str(include_str!("cluster_spec.json")).unwrap();
        spec.dynamic_shared_buffers = Some(DynamicSharedBuffersSpec {
            protocol_version: 1,
            bootstrap_buffer_bytes: 2 * 1024 * 1024,
            maximum_buffer_bytes: 64 * 1024 * 1024,
            minimum_guest_memory_bytes: 128 * 1024 * 1024,
            non_buffer_reserve_bytes: 64 * 1024 * 1024,
        });
        spec
    }

    #[test]
    fn restored_large_pool_is_followed_by_safe_managed_bootstrap() {
        let fixture = Fixture::new();
        let mut spec = spec();
        spec.cluster.postgresql_conf = Some("shared_buffers = '32MB'".into());
        fixture.write(&spec).unwrap();
        let contents = fixture.read();
        let managed = contents.rfind("shared_buffers='2048kB'").unwrap();
        assert!(managed > contents.find("shared_buffers = '32MB'").unwrap());
        assert!(contents.contains("dynamic_shared_buffers_require_admission='on'"));
        assert!(contents.contains("dynamic_shared_buffers_startup_budget='65536kB'"));
        // 保留官方末尾include约定；命令行负责固定受管GUC，不改变通用override行为。
        assert!(contents.ends_with("include_if_exists = 'compute_ctl_temp_override.conf'\n"));
    }

    #[test]
    fn invalid_managed_budget_does_not_truncate_existing_configuration() {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.0.join("postgresql.conf"),
            "previous configuration\n",
        )
        .unwrap();
        let mut spec = spec();
        spec.dynamic_shared_buffers
            .as_mut()
            .unwrap()
            .non_buffer_reserve_bytes = u64::MAX;
        assert!(fixture.write(&spec).is_err());
        assert_eq!(fixture.read(), "previous configuration\n");
    }

    #[test]
    fn legacy_spec_does_not_enable_dynamic_buffers_or_admission() {
        let fixture = Fixture::new();
        let mut spec = spec();
        spec.dynamic_shared_buffers = None;
        fixture.write(&spec).unwrap();
        assert!(!fixture.read().contains("dynamic_shared_buffers"));
    }
}
