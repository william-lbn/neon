//! DSGA内部控制请求的跨语言契约。
//!
//! 所有64位身份、计数和字节数在JSON中必须是规范十进制字符串，不能经过
//! JavaScript/Go map中的浮点数，也不能强转SQL bigint丢掉最高位。本模块只做
//! 语法/可表示性校验；预算承诺引用、管理认证、issuer所有权和VM最终执行权
//! 必须由实际服务端核验。构造或反序列化这些类型不会启用伸缩或产生VM许可。

use std::fmt;
use std::str::FromStr;

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const DSG_CONTROL_PROTOCOL_VERSION: u32 = 1;
/// 对应当前PG17协调器的一次等待上限；不表示可强行中断内核Provider调用。
pub const DSG_MAX_WAIT_MS: u32 = 60_000;
const PG17_MAX_BUFFERS: u32 = (i32::MAX / 2) as u32;
const MAX_ID_BYTES: usize = 128;

/// 唯一JSON表示为`"0"`或无前导零的ASCII十进制字符串。
///
/// 0在类型层面合法；各身份字段是否允许0由请求语义校验决定。拒绝JSON number、
/// 正负号、空白、指数、非ASCII数字和溢出，避免跨语言签名/重放键出现多种表示。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DsgUint64(u64);

impl DsgUint64 {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for DsgUint64 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for DsgUint64 {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        ensure!(
            !value.is_empty()
                && value.len() <= 20
                && value.bytes().all(|byte| byte.is_ascii_digit())
                && (value == "0" || !value.starts_with('0')),
            "DSGA uint64 requires a canonical decimal string"
        );
        Ok(Self(value.parse()?))
    }
}

impl Serialize for DsgUint64 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for DsgUint64 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// 对应同一次共享boot；实例身份不能由PG boot ID或名字单独替代。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgInstanceIdentity {
    pub vm_uid: String,
    pub compute_instance_id: String,
    pub pg_boot_id: DsgUint64,
}

/// 128位随机操作ID的两个无损半部，与PG原生请求保持同一位序。
/// 不采用平台相关usize或有符号SQL整数；两半均0表示未分配身份，必须拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgOperationId {
    pub high: DsgUint64,
    pub low: DsgUint64,
}

impl DsgOperationId {
    /// 跨语言固定按网络字节序拆分随机16字节：前8字节为high，后8字节为low。
    /// 随机数来源由受管客户端提供；此转换不生成随机值，不把全零变成合法操作。
    pub fn from_be_bytes(bytes: [u8; 16]) -> Self {
        let value = u128::from_be_bytes(bytes);
        Self {
            high: DsgUint64::new((value >> 64) as u64),
            low: DsgUint64::new(value as u64),
        }
    }

    pub fn to_be_bytes(self) -> [u8; 16] {
        ((u128::from(self.high.get()) << 64) | u128::from(self.low.get())).to_be_bytes()
    }
}

/// 请求、查询、取消共用的完整键。epoch/Revision是外层持久顺序；验证为非零
/// 只证明可表示，不能把调用者自报的较大数字当成合法issuer或资源执行权。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgOperationKey {
    pub instance: DsgInstanceIdentity,
    pub issuer_epoch: DsgUint64,
    pub revision: DsgUint64,
    pub operation_id: DsgOperationId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DsgResizeKind {
    Grow,
    Shrink,
}

/// 一经接受便不可修改的完整意图；同key改target、预算或期限必须作为冲突处理。
/// `target_buffers`和协议版本受32位小范围限制，可安全使用JSON整数；所有64位
/// 字段都用DsgUint64。预算ID只引用外层持久承诺，不接受自行填充的“可用字节”。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgResizeRequest {
    pub protocol_version: u32,
    pub key: DsgOperationKey,
    pub kind: DsgResizeKind,
    pub expected_capacity_generation: DsgUint64,
    pub target_buffers: u32,
    pub target_vm_memory_bytes: DsgUint64,
    pub budget_commitment_id: String,
    pub deadline_unix_ms: DsgUint64,
}

/// 查询/取消也必须关联完整实例、boot、issuer及原操作，不能只发送PID或最近Revision。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DsgOperationQuery {
    pub protocol_version: u32,
    pub key: DsgOperationKey,
}

fn validate_id(value: &str, name: &str) -> Result<()> {
    // 这里是UID/实例ID/承诺引用，不是显示名。跨C/Rust/Go统一字节长度，拒绝
    // NUL/控制字符/空白，避免C截断、日志伪造或不同规范化形成同一重放键。
    ensure!(
        !value.is_empty()
            && value.len() <= MAX_ID_BYTES
            && value.bytes().all(|byte| byte.is_ascii_graphic()),
        "invalid DSGA {name} identity"
    );
    Ok(())
}

impl DsgOperationKey {
    pub fn validate(&self) -> Result<()> {
        validate_id(&self.instance.vm_uid, "VM")?;
        validate_id(&self.instance.compute_instance_id, "Compute")?;
        ensure!(
            self.instance.pg_boot_id.get() != 0,
            "DSGA boot ID must be nonzero"
        );
        ensure!(
            self.issuer_epoch.get() != 0,
            "DSGA issuer epoch must be nonzero"
        );
        ensure!(self.revision.get() != 0, "DSGA revision must be nonzero");
        ensure!(
            self.operation_id.high.get() != 0 || self.operation_id.low.get() != 0,
            "DSGA operation ID must be nonzero"
        );
        Ok(())
    }
}

impl DsgResizeRequest {
    /// 只验证PG17 wire可表示性；实际B_boot/M、方向、generation、总预算、承诺
    /// 和执行者身份必须与当前运行事实核对。调用方在每个服务边界都须再次验证。
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.protocol_version == DSG_CONTROL_PROTOCOL_VERSION,
            "unsupported DSGA control protocol"
        );
        self.key.validate()?;
        ensure!(
            (16..=PG17_MAX_BUFFERS).contains(&self.target_buffers),
            "DSGA buffer target is not representable by PG17"
        );
        ensure!(
            self.expected_capacity_generation.get() != 0,
            "DSGA capacity generation must be nonzero"
        );
        ensure!(
            self.target_vm_memory_bytes.get() != 0 && self.deadline_unix_ms.get() != 0,
            "DSGA Guest target and deadline must be nonzero"
        );
        validate_id(&self.budget_commitment_id, "budget commitment")?;
        Ok(())
    }

    /// 仅在历史/幂等查找之后为新操作计算本次协调等待。过期请求不能重新执行，
    /// 但仍必须能够查询/重放已有回执，所以validate不根据当前时钟否定历史意图。
    /// 相对等待会截到内核60s上限；这不撤销已发出的Provider/QMP动作或预算承诺。
    pub fn remaining_wait_ms(&self, now_unix_ms: u64) -> Option<u32> {
        let remaining = self.deadline_unix_ms.get().checked_sub(now_unix_ms)?;
        if remaining == 0 {
            None
        } else {
            Some(remaining.min(u64::from(DSG_MAX_WAIT_MS)) as u32)
        }
    }
}

impl DsgOperationQuery {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.protocol_version == DSG_CONTROL_PROTOCOL_VERSION,
            "unsupported DSGA control protocol"
        );
        self.key.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> DsgResizeRequest {
        DsgResizeRequest {
            protocol_version: DSG_CONTROL_PROTOCOL_VERSION,
            key: DsgOperationKey {
                instance: DsgInstanceIdentity {
                    vm_uid: "vm-uid-1".to_owned(),
                    compute_instance_id: "compute-instance-1".to_owned(),
                    pg_boot_id: DsgUint64::new(u64::MAX),
                },
                issuer_epoch: DsgUint64::new(1),
                revision: DsgUint64::new(9_007_199_254_740_993),
                operation_id: DsgOperationId {
                    high: DsgUint64::new(1 << 63),
                    low: DsgUint64::new(u64::MAX),
                },
            },
            kind: DsgResizeKind::Shrink,
            expected_capacity_generation: DsgUint64::new(2),
            target_buffers: 2048,
            target_vm_memory_bytes: DsgUint64::new(1 << 30),
            budget_commitment_id: "reservation-1".to_owned(),
            deadline_unix_ms: DsgUint64::new(100_000),
        }
    }

    #[test]
    fn uint64_roundtrip_preserves_full_bits_as_strings() {
        for value in [
            0,
            1,
            (1 << 53) - 1,
            1 << 53,
            (1 << 53) + 1,
            i64::MAX as u64,
            1 << 63,
            u64::MAX,
        ] {
            let encoded = serde_json::to_string(&DsgUint64::new(value)).unwrap();
            assert_eq!(encoded, format!("\"{value}\""));
            assert_eq!(
                serde_json::from_str::<DsgUint64>(&encoded).unwrap().get(),
                value
            );
            assert!(serde_json::from_value::<DsgUint64>(json!(value)).is_err());
        }
    }

    #[test]
    fn rejects_ambiguous_decimal_and_non_string_json() {
        for text in [
            "",
            "00",
            "01",
            "-1",
            "+1",
            " 1",
            "1 ",
            "1.0",
            "1e2",
            "１",
            "١",
            "18446744073709551616",
            "999999999999999999999",
        ] {
            assert!(
                serde_json::from_value::<DsgUint64>(json!(text)).is_err(),
                "{text:?}"
            );
        }
        for value in [json!(null), json!(true), json!([]), json!({}), json!(1.0)] {
            assert!(serde_json::from_value::<DsgUint64>(value).is_err());
        }
    }

    #[test]
    fn operation_bytes_have_a_fixed_cross_language_word_order() {
        let bytes = [
            0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef,
        ];
        let id = DsgOperationId::from_be_bytes(bytes);
        assert_eq!(id.high.get(), 0xfedcba9876543210);
        assert_eq!(id.low.get(), 0x0123456789abcdef);
        assert_eq!(id.to_be_bytes(), bytes);
        let json = serde_json::to_value(id).unwrap();
        assert_eq!(json["high"], "18364758544493064720");
        assert_eq!(json["low"], "81985529216486895");
    }

    #[test]
    fn duplicate_fields_cannot_choose_a_different_identity_or_version() {
        let encoded = serde_json::to_string(&request()).unwrap();
        for (original, duplicate) in [
            (
                "\"protocol_version\":1",
                "\"protocol_version\":1,\"protocol_version\":2",
            ),
            (
                "\"vm_uid\":\"vm-uid-1\"",
                "\"vm_uid\":\"vm-uid-1\",\"vm_uid\":\"vm-uid-2\"",
            ),
        ] {
            let ambiguous = encoded.replacen(original, duplicate, 1);
            assert_ne!(ambiguous, encoded);
            assert!(serde_json::from_str::<DsgResizeRequest>(&ambiguous).is_err());
        }
    }

    #[test]
    fn request_roundtrip_binds_large_identity_and_all_parameters() {
        let original = request();
        original.validate().unwrap();
        let json = serde_json::to_value(&original).unwrap();
        assert_eq!(json["key"]["revision"], "9007199254740993");
        assert_eq!(
            json["key"]["instance"]["pg_boot_id"],
            "18446744073709551615"
        );
        assert_eq!(json["key"]["operation_id"]["high"], "9223372036854775808");
        assert_eq!(
            serde_json::from_value::<DsgResizeRequest>(json).unwrap(),
            original
        );
        let mut changed = original.clone();
        changed.deadline_unix_ms = DsgUint64::new(100_001);
        assert_eq!(changed.key, original.key);
        assert_ne!(changed, original, "同key改变期限必须仍可检测为不同请求");
    }

    #[test]
    fn unknown_or_missing_nested_fields_and_number_coercion_are_rejected() {
        for path in ["top", "key", "instance", "operation"] {
            let mut value = serde_json::to_value(request()).unwrap();
            let object = match path {
                "top" => &mut value,
                "key" => &mut value["key"],
                "instance" => &mut value["key"]["instance"],
                _ => &mut value["key"]["operation_id"],
            };
            object["unknown"] = json!(true);
            assert!(serde_json::from_value::<DsgResizeRequest>(value).is_err());
        }
        let mut value = serde_json::to_value(request()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("budget_commitment_id");
        assert!(serde_json::from_value::<DsgResizeRequest>(value).is_err());
        let mut value = serde_json::to_value(request()).unwrap();
        value["key"]["revision"] = json!(9_007_199_254_740_993u64);
        assert!(serde_json::from_value::<DsgResizeRequest>(value).is_err());
    }

    #[test]
    fn semantic_validation_rejects_empty_or_unrepresentable_intents() {
        for case in 0..13 {
            let mut value = request();
            match case {
                0 => value.protocol_version += 1,
                1 => value.key.instance.vm_uid.clear(),
                2 => value.key.instance.compute_instance_id = "x".repeat(MAX_ID_BYTES + 1),
                3 => value.key.instance.vm_uid = "vm\0other".to_owned(),
                4 => value.key.instance.pg_boot_id = DsgUint64::new(0),
                5 => value.key.issuer_epoch = DsgUint64::new(0),
                6 => value.key.revision = DsgUint64::new(0),
                7 => {
                    value.key.operation_id = DsgOperationId {
                        high: DsgUint64::new(0),
                        low: DsgUint64::new(0),
                    }
                }
                8 => value.target_buffers = PG17_MAX_BUFFERS + 1,
                9 => value.expected_capacity_generation = DsgUint64::new(0),
                10 => value.target_vm_memory_bytes = DsgUint64::new(0),
                11 => value.deadline_unix_ms = DsgUint64::new(0),
                _ => value.budget_commitment_id = "reservation with spaces".to_owned(),
            }
            assert!(value.validate().is_err(), "case {case}");
        }
        let mut value = request();
        value.target_buffers = 15;
        assert!(value.validate().is_err());
        value.target_buffers = 16;
        value.validate().unwrap();
        value.target_buffers = PG17_MAX_BUFFERS;
        value.validate().unwrap();
        value.key.operation_id.high = DsgUint64::new(0);
        value.key.operation_id.low = DsgUint64::new(1);
        value.validate().unwrap(); // 与PG仅拒绝两半都为0的原生语义一致。
    }

    #[test]
    fn expiry_does_not_rewrite_intent_and_wait_is_bounded_without_overflow() {
        let mut value = request();
        assert_eq!(value.remaining_wait_ms(0), Some(60_000));
        assert_eq!(value.remaining_wait_ms(99_999), Some(1));
        assert_eq!(value.remaining_wait_ms(100_000), None);
        assert_eq!(value.remaining_wait_ms(u64::MAX), None);
        value.validate().unwrap(); // 已到期仍可用于历史查找，不能重新执行。
        value.deadline_unix_ms = DsgUint64::new(u64::MAX);
        assert_eq!(value.remaining_wait_ms(u64::MAX - 1), Some(1));
        assert_eq!(value.remaining_wait_ms(1), Some(60_000));
    }

    #[test]
    fn queries_and_cancels_require_the_full_same_key() {
        let original = request();
        let query = DsgOperationQuery {
            protocol_version: 1,
            key: original.key.clone(),
        };
        query.validate().unwrap();
        let encoded = serde_json::to_value(&query).unwrap();
        assert_eq!(encoded["key"], serde_json::to_value(original.key).unwrap());
        let mut incomplete = encoded;
        incomplete["key"]
            .as_object_mut()
            .unwrap()
            .remove("instance");
        assert!(serde_json::from_value::<DsgOperationQuery>(incomplete).is_err());
        let mut unknown = query;
        unknown.protocol_version += 1;
        assert!(unknown.validate().is_err());
    }
}
