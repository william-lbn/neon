# DSGA 生产控制请求契约

日期：2026-10-20。落实 V3 第9节/R09；共享类型位于现有
`libs/compute_api/src/dsg_control.rs`，不是另建独立项目。

最初wire提交只交付请求/查询/取消的共享数据类型、格式与结构校验；后续PG17已
增加内核回执/历史/业务取消，见[内核接入](DSGA-R09-内核回执与取消接入.md)。这
仍不代表生产SQL/HTTP端点、认证、预算或VM fencing已经完成。普通请求解析
成功不产生任何资源许可；动态模式仍须按V3完成全链路能力协商和发布门槛。

## 字段与跨语言表示

```json
{
  "protocol_version": 1,
  "key": {
    "instance": {
      "vm_uid": "vm-uid-1",
      "compute_instance_id": "compute-instance-1",
      "pg_boot_id": "18446744073709551615"
    },
    "issuer_epoch": "1",
    "revision": "9007199254740993",
    "operation_id": {
      "high": "18364758544493064720",
      "low": "81985529216486895"
    }
  },
  "kind": "shrink",
  "expected_capacity_generation": "2",
  "target_buffers": 2048,
  "target_vm_memory_bytes": "1073741824",
  "budget_commitment_id": "reservation-1",
  "deadline_unix_ms": "100000"
}
```

例子用于表示与人工时钟测试，不能直接作为当前可执行的预算/期限意图。

- 所有64位字段为ASCII十进制**字符串**，允许`"0"`或无前导零的正数，最大
  `"18446744073709551615"`。拒绝JSON number、符号、空白、小数、指数、Unicode
  数字及溢出；禁止经float或SQL bigint中转。大小/顺序比较使用解析后的uint64，
  不比较字符串字典序。
- 协议版本和Buffer数量为有界32位JSON整数。当前数量仅验证PG17可表示范围
  `[16, INT_MAX/2]`；实际B_boot、M、方向、generation和全内存预算仍须服务端核验。
- 操作ID按网络字节序将随机16字节分为前8字节high、后8字节low；两半都为0拒绝。
  测试向量`FEDCBA98765432100123456789ABCDEF`对应上例的两个十进制半部。随机
  来源由受管客户端提供，不使用浮点随机数，也不把转换本身当成随机数生成。
- VM UID、Compute实例ID与预算承诺引用为1–128字节ASCII可见字符，不是用户显示名；
  拒绝NUL、控制字符及空白，避免C截断/日志伪造。boot、issuer epoch、Revision与
  capacity generation必须非零；操作ID允许单半部为0，只拒绝两半同时为0。
- 所有结构拒绝未知、缺失与重复字段；Rust测试已验证。C/Go消费者必须落实相同
  规则并做交叉验证，不能先用会丢掉重复键的中间JSON map归一化后就当成已验证。

查询/业务取消使用`{ "protocol_version": 1, "key": 完整原操作键 }`。不能只查PID、
最近Revision或当前PG状态。同key改变kind、target、generation、预算引用或期限，
都必须由实际操作历史判为冲突；DTO的相等比较不等于已经实现幂等执行。

## 期限、回执与权限约定

请求期限是Unix毫秒。类型的结构validate不依据当前墙钟拒绝旧意图，因为查询/重放
已有回执必须仍能处理已过期请求。**先查历史与幂等身份，再决定新操作是否过期。**
新操作相对等待用checked_sub计算，截到当前PG协调器60s上限；已过期返回无等待
预算。未来期限很大时也不溢出。系统调用/QMP已经发出后仍须对账，超时或取消不
自动撤销资源承诺，更不能把它解释为“动作未发生”。

生产服务必须把本请求receipt和observed state分开，分别带自身身份。PG保留有界
本boot结果，外层持久保存历史；淘汰/过期返回STALE/RESULT_EXPIRED及当前事实，
不能再次执行相同旧key。PG17的内核回执/有界历史已独立实现，仍须接入生产端点
及外层持久历史；不能由本类型模块或内核测试桥代替。

预算ID只是外层承诺引用。服务端须验证管理认证、当前实例/boot、issuer所有权、
epoch/revision、承诺完整内容及有效约束；VM Controller/Runner仍须在实际写入前
再次fence并串行化已发出的动作。`validate()`返回Ok只证明结构与范围，不授权执行。

本模块协议1是新的DSGA子协议命名空间，与现有Monitor/Agent的旧v1.0握手不是同一
能力。尚未接入旧消息，也没有把旧Ok=true解释为新缩容许可；后续启用前必须完成
全链路显式协商。后续内核Buffer ABI5也不代表已经存在这些生产控制端点。

## Mac 实际验证

- `cargo test --locked --offline -p compute_api`：21项通过，零失败/ignored，包含
  9项新增契约测试；原有bootstrap/spec等12项同时通过。
- 覆盖2^53邻界、2^63及uint64最大值、拒绝数字强转/溢出/模糊表示、操作ID字节序、
  全身份回传、重复/未知/缺失嵌套字段、零值/范围/ID边界、期限到达和大值减法。
- `cargo clippy --locked --offline --no-deps -p compute_api --all-targets -- -D warnings`
  退出0；依赖仍报告既有warnings，不能称全工作区无警告。包含path依赖的原命令
  因remote_storage中41个已有lint退出101，没有修改或屏蔽这些无关源码。
- 使用仓库固定Rust1.88；本机PATH中独立rustfmt过旧，显式
  `rustup run 1.88.0 rustfmt --edition 2024`完成格式，不改全局工具链安装。

日志：工作区`build/neon-dsg-control-compute-api{,-clippy,-clippy-local}.log`。
尚未验证Go/C对同一JSON的交叉编码，也未完成实际生产控制链。这些是代码/集成剩余项，
不能归为Linux环境缺失。目标Linux/VM/HugeTLB及性能认证仍按V3另验。
