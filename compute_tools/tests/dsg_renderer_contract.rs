//! 新鲜Go安全Renderer→真实ComputeConfig/ParsedSpec消费者，不使用手写JSON替身。
//! 此门禁不启动PG/VM，不等于公开worker/存储/物理回收端到端；只认证本层接线。

use anyhow::{Result, ensure};
use compute_api::responses::ComputeConfig;
use compute_api::spec::ComputeMode;
use compute_tools::compute::ParsedSpec;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    name: String,
    config: ComputeConfig,
}

#[test]
#[ignore = "requires fresh NEON_DSGA_RENDERER_CONTRACT from own control-plane Go renderer"]
fn go_safe_renderer_is_accepted_by_actual_compute_parser() -> Result<()> {
    let path = std::env::var("NEON_DSGA_RENDERER_CONTRACT")?;
    let samples: Vec<Sample> = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure!(
        samples.len() == 2,
        "missing primary/replica renderer sample"
    );
    let mut seen = std::collections::HashSet::new();
    for sample in samples {
        ensure!(seen.insert(sample.name.clone()), "duplicate renderer mode");
        ensure!(matches!(sample.name.as_str(), "read_write" | "read_only"));
        let encoded = serde_json::to_vec(&sample.config)?;
        let decoded: ComputeConfig = serde_json::from_slice(&encoded)?;
        ensure!(decoded.compute_ctl_config.jwks.keys.len() == 1);
        let tls = decoded.compute_ctl_config.tls.as_ref().unwrap();
        ensure!(tls.key_path == "/run/neon/management/control.key");
        let spec = decoded.spec.expect("renderer must attach original spec");
        let parsed = ParsedSpec::try_from(spec.clone())?;
        parsed.validate().map_err(anyhow::Error::msg)?;
        ensure!((parsed.spec.mode == ComputeMode::Replica) == (sample.name == "read_only"));
        let native = spec.dynamic_shared_buffers.as_ref().unwrap();
        ensure!(native.vm_uid == "actual-vm-uid");
        ensure!(native.bootstrap_buffer_bytes == 64 << 20);
        ensure!(native.maximum_buffer_bytes == 2 << 30);
        ensure!(native.minimum_guest_memory_bytes == 1 << 30);
        ensure!(native.non_buffer_reserve_bytes == 512 << 20);
        ensure!(
            native
                .postgres_settings()?
                .contains(&("dynamic_shared_buffers_require_admission", "on".into()))
        );
        ensure!(!spec.skip_pg_catalog_updates);
        ensure!(spec.cluster.roles.len() == 2);
        ensure!(spec.cluster.roles[0].name == "app_owner");
        ensure!(spec.cluster.roles[1].name == "control_probe");
        ensure!(spec.cluster.databases[0].owner == "app_owner");
        ensure!(spec.local_proxy_config.is_none());
        for (name, expected) in [
            ("fsync", "on"),
            ("password_encryption", "scram-sha-256"),
            ("shared_preload_libraries", "neon,pg_stat_statements"),
            ("neon.max_file_cache_size", "0"),
        ] {
            ensure!(
                spec.cluster
                    .settings
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|option| {
                        option.name == name && option.value.as_deref() == Some(expected)
                    })
            );
        }
        // 同一真实入口必须拒绝把业务口令换到内部SU，不只是Go字符串断言。
        let mut unsafe_spec = spec;
        unsafe_spec.cluster.roles[0].name = "cloud_admin".into();
        ensure!(ParsedSpec::try_from(unsafe_spec).is_err());
    }
    Ok(())
}
