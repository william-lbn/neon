//! 自有控制面Go生产者→原生Rust启动消费者的真实序列化契约。
//! 默认显式ignored：必须先运行Go导出器并提供本次文件，不能靠仓内旧快照冒充互通。
//! 不启动PG/VM；验证的是同一生产DTO的反序列化、预算规则与强制启动参数。

use compute_api::dynamic_shared_buffers::DynamicSharedBuffersSpec;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    spec: DynamicSharedBuffersSpec,
    valid: bool,
}

#[test]
#[ignore = "requires fresh NEON_DSGA_NATIVE_PLAN_CONTRACT exported by control-plane Go tests"]
fn go_frozen_creation_plan_matches_native_bootstrap_contract() {
    let path = std::env::var("NEON_DSGA_NATIVE_PLAN_CONTRACT")
        .expect("explicit fresh Go contract path is required");
    let bytes = std::fs::read(path).expect("read fresh Go contract");
    let cases: Vec<Case> = serde_json::from_slice(&bytes).expect("strict native DTO decode");
    // 两项有效/九项无效，缺少某组或重复名字不能静默缩减覆盖面。
    assert_eq!(cases.len(), 11);
    assert_eq!(cases.iter().filter(|case| case.valid).count(), 2);
    let names: std::collections::HashSet<_> = cases.iter().map(|case| case.name.as_str()).collect();
    assert_eq!(
        names,
        std::collections::HashSet::from([
            "production",
            "uint64",
            "version",
            "vm",
            "compute",
            "block",
            "maximum",
            "reserve",
            "floor",
            "rounding",
            "kb_overflow"
        ])
    );
    for case in cases {
        assert_eq!(case.spec.validate().is_ok(), case.valid, "{}", case.name);
        if !case.valid {
            assert!(case.spec.postgres_settings().is_err(), "{}", case.name);
            continue;
        }
        let settings = case.spec.postgres_settings().expect("native managed GUCs");
        assert!(settings.contains(&("dynamic_shared_buffers_require_admission", "on".to_owned())));
        assert!(settings.contains(&("neon.dsg_vm_uid", "actual-vm-uid".to_owned())));
        assert!(settings.contains(&(
            "shared_buffers",
            format!("{}kB", case.spec.bootstrap_buffer_bytes / 1024)
        )));
        // 不能用float64再编码，否则uint64边界会变成超出Rust范围的2^64。
        let encoded = serde_json::to_vec(&case.spec).unwrap();
        let restored: DynamicSharedBuffersSpec = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(case.spec, restored);
        if case.name == "uint64" {
            assert_eq!(restored.minimum_guest_memory_bytes, u64::MAX);
            assert_eq!(restored.non_buffer_reserve_bytes, u64::MAX - (128 << 20));
        }
    }
}
