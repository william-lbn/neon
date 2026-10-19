//! 动态 Buffer Pool 的受管启动契约。
//!
//! `shared_buffers` 必须始终是最低合法 Guest 规格能容纳的安全 bootstrap，
//! 在线目标交给运行时协议处理。不能把某次较大的 H 写回启动配置，否则同一
//! Postmaster 内 crash recovery 会在已缩小的 VM 中重建旧大池。

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const DSG_BOOTSTRAP_SPEC_VERSION: u32 = 1;
const PG_BLOCK_BYTES: u64 = 8192;
const PG_MAX_BUFFERS: u64 = (i32::MAX / 2) as u64;

/// 控制面明确给出的启动预算，单位统一为 bytes，不采用隐含的百分比默认值。
///
/// `non_buffer_reserve_bytes` 包含 OS、进程/查询/维护、页表、LFC、部署保留与余量。
/// 剩余预算用于 PG 主共享段（含按 M 固定的 hash/扩展共享内存）和 bootstrap
/// 数组。Rust 做可表示性检查，PG 在所有 preload 请求登记后核对真实启动布局。
/// 这里通过不代表运行时私有内存已受硬约束，也不直接生成 VM 缩容许可。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicSharedBuffersSpec {
    pub protocol_version: u32,
    pub bootstrap_buffer_bytes: u64,
    pub maximum_buffer_bytes: u64,
    pub minimum_guest_memory_bytes: u64,
    pub non_buffer_reserve_bytes: u64,
}

/// 这些字段定义一次Postmaster生命周期的资源下限，SIGHUP不能改变。
/// 目标H走resize协议；启动规格有变必须受控重启，不能只写文件后报重配成功。
pub fn validate_reconfigure(
    previous: Option<&DynamicSharedBuffersSpec>,
    requested: Option<&DynamicSharedBuffersSpec>,
) -> Result<()> {
    ensure!(
        previous == requested,
        "dynamic buffer bootstrap changes require a controlled restart"
    );
    Ok(())
}

impl DynamicSharedBuffersSpec {
    /// 只接受完整协商的规格；未知版本、单位损失或不可能的预算均在启动前拒绝。
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.protocol_version == DSG_BOOTSTRAP_SPEC_VERSION,
            "unsupported dynamic shared buffers bootstrap protocol"
        );
        for (name, bytes) in [
            ("bootstrap", self.bootstrap_buffer_bytes),
            ("maximum", self.maximum_buffer_bytes),
        ] {
            // 16 是 PG 参数的语法下界，并不是服务支持的最小生产规格。
            ensure!(
                bytes >= 16 * PG_BLOCK_BYTES
                    && bytes % PG_BLOCK_BYTES == 0
                    && bytes / PG_BLOCK_BYTES <= PG_MAX_BUFFERS,
                "{name} dynamic buffer size is not representable by PostgreSQL"
            );
        }
        ensure!(
            self.bootstrap_buffer_bytes <= self.maximum_buffer_bytes,
            "dynamic buffer bootstrap exceeds maximum"
        );
        ensure!(
            self.non_buffer_reserve_bytes > 0,
            "dynamic buffer startup requires an explicit non-buffer reserve"
        );
        let budget = self.shared_memory_budget_kb()?;
        ensure!(
            self.bootstrap_buffer_bytes <= budget * 1024,
            "dynamic buffer bootstrap exceeds the minimum Guest startup budget"
        );
        Ok(())
    }

    fn shared_memory_budget_kb(&self) -> Result<u64> {
        let bytes = self
            .minimum_guest_memory_bytes
            .checked_sub(self.non_buffer_reserve_bytes)
            .ok_or_else(|| anyhow::anyhow!("non-buffer reserve exceeds minimum Guest memory"))?;
        // 向下取整才能保留声明的 reserve；绝不能把预算向上放大到下一KB。
        let kb = bytes / 1024;
        ensure!(
            kb > 0 && kb <= i32::MAX as u64,
            "dynamic buffer startup shared-memory budget is not representable"
        );
        Ok(kb)
    }

    /// 相同受管项同时写入文件和 Postmaster 的 `-c` 参数；后者优先于
    /// postgresql.auto.conf/include，防止旧备份或临时配置恢复旧大池。
    /// 新 boot 默认关闭普通工作准入，由认证管理链完成预算对账后显式打开。
    pub fn postgres_settings(&self) -> Result<Vec<(&'static str, String)>> {
        self.validate()?;
        Ok(vec![
            ("enable_dynamic_shared_buffers", "on".to_owned()),
            (
                "shared_buffers",
                format!("{}kB", self.bootstrap_buffer_bytes / 1024),
            ),
            (
                "max_shared_buffers",
                format!("{}kB", self.maximum_buffer_bytes / 1024),
            ),
            ("dynamic_shared_buffers_require_admission", "on".to_owned()),
            (
                "dynamic_shared_buffers_startup_budget",
                format!("{}kB", self.shared_memory_budget_kb()?),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> DynamicSharedBuffersSpec {
        DynamicSharedBuffersSpec {
            protocol_version: 1,
            bootstrap_buffer_bytes: 8 * 1024 * 1024,
            maximum_buffer_bytes: 64 * 1024 * 1024,
            minimum_guest_memory_bytes: 256 * 1024 * 1024,
            non_buffer_reserve_bytes: 128 * 1024 * 1024,
        }
    }

    #[test]
    fn startup_uses_bootstrap_and_starts_with_admission_closed() {
        let settings = spec().postgres_settings().unwrap();
        assert!(settings.contains(&("shared_buffers", "8192kB".to_owned())));
        assert!(settings.contains(&("max_shared_buffers", "65536kB".to_owned())));
        assert!(settings.contains(&("dynamic_shared_buffers_require_admission", "on".to_owned())));
        assert!(settings.contains(&(
            "dynamic_shared_buffers_startup_budget",
            "131072kB".to_owned()
        )));
    }

    #[test]
    fn rejects_bad_version_ranges_alignment_and_budget() {
        for change in 0..7 {
            let mut value = spec();
            match change {
                0 => value.protocol_version = 2,
                1 => value.bootstrap_buffer_bytes += 1,
                2 => value.maximum_buffer_bytes = value.bootstrap_buffer_bytes - PG_BLOCK_BYTES,
                3 => value.non_buffer_reserve_bytes = 0,
                4 => value.non_buffer_reserve_bytes = value.minimum_guest_memory_bytes + 1,
                5 => value.minimum_guest_memory_bytes = value.non_buffer_reserve_bytes + 1023,
                _ => value.maximum_buffer_bytes = u64::MAX,
            }
            assert!(value.postgres_settings().is_err(), "case {change}");
        }
    }

    #[test]
    fn reserve_rounding_never_increases_available_budget() {
        let mut value = spec();
        value.minimum_guest_memory_bytes += 1023;
        assert_eq!(value.shared_memory_budget_kb().unwrap(), 131072);
        value.non_buffer_reserve_bytes = value.minimum_guest_memory_bytes - 1024;
        assert!(value.validate().is_err());
    }

    #[test]
    fn unknown_fields_and_missing_budget_are_rejected() {
        let mut json = serde_json::to_value(spec()).unwrap();
        json["non_buffer_resrve_bytes"] = serde_json::json!(123);
        assert!(serde_json::from_value::<DynamicSharedBuffersSpec>(json).is_err());
        let mut json = serde_json::to_value(spec()).unwrap();
        json.as_object_mut()
            .unwrap()
            .remove("non_buffer_reserve_bytes");
        assert!(serde_json::from_value::<DynamicSharedBuffersSpec>(json).is_err());
    }

    #[test]
    fn runtime_reconfiguration_cannot_change_bootstrap_contract() {
        let old = spec();
        assert!(validate_reconfigure(None, None).is_ok());
        assert!(validate_reconfigure(Some(&old), Some(&old)).is_ok());
        assert!(validate_reconfigure(None, Some(&old)).is_err());
        assert!(validate_reconfigure(Some(&old), None).is_err());
        let mut changed = old.clone();
        changed.bootstrap_buffer_bytes *= 2;
        assert!(validate_reconfigure(Some(&old), Some(&changed)).is_err());
    }
}
