# DSGA Rust 双连接管理客户端

日期：2026-10-20。实现位于现有 `libs/compute_api`，`dsg_state` 提供严格响应
类型，`postgres-client` 可选 feature 提供 `dsg_client::DsgSqlClient`。这是已连接
真实 Neon 管理 SQL 的共享客户端；尚未接入 Compute HTTP、Monitor 或 Go Agent，
不代表完整控制链已交付。默认关闭与 V3 发布门槛继续有效。

## 连接、权限和能力边界

调用方提供两个专用、已认证的 `tokio_postgres::Client` 并负责连接 driver 的生命
周期。执行连接运行 resize；独立观察连接查询状态和发送业务取消。bind 比较实际
`pg_backend_pid()`，拒绝以同一会话兼任两个通道，避免执行排水阻塞时无法取消。
客户端不处理租户密码，不记录连接串，不连接用户已有数据库进行测试。

每个连接先固定 `search_path=pg_catalog,pg_temp`；目录、类型和系统函数显式限定
`pg_catalog`。从实际扩展目录发现并转义 schema，检查五个精确 C 签名确实属于
Neon 扩展，而非只按名称信任同名函数。要求扩展所有者为真正 superuser，并拒绝
任一非超级角色直接或继承获得管理 schema 的 CREATE 权限。伪角色
`pg_database_owner` 仅在实际数据库所有者为 superuser 时允许。

官方 Compute 已在 `spec_apply.rs` 的 `CreateSchemaNeon` / `HandleNeonExtension`
阶段创建专用 `neon` schema 并将扩展迁入其中。后续接入应使用系统维护数据库和
真正管理员连接；SDK 不擅自撤销业务权限或迁移整个扩展。租户可写 schema 必须
由管理员先处理，不能降级为未经核验的管理调用。

两连接能力须完全一致，Buffer ABI=5，动态模式启用且身份绑定；可信配置中的
VM UID、Compute 实例 ID 和实际 boot 必须一致。只绑定逻辑控制能力，即使
`production_ready=false` 仍可执行模块测试；`vm_shrink_permit=true` 一律拒绝。
不得把绑定成功转为 VM 执行许可。

## 不确定结果和本地执行栅栏

`DsgCallError::NotSent` 表示本地验证或执行栅栏拒绝，尚未调用 SQL；可能发送后
的 SQL ERROR、网络错误、响应超时或响应验证失败均为带完整原 key 的
`Unconfirmed`。超时和丢弃 future 不等于取消，更不等于 DENIED。

短持有的 Mutex 保护本客户端的原始完整请求和 in-flight 状态，不跨 await 持锁。
RAII 在正常返回、错误、超时及 future 被丢弃时释放调用占用；未确认的完整原意图
继续保留。后续只能原样重放，不能改变目标、预算引用、deadline 或换 operation ID。
只有验证过的未建立请求结果或本请求终态，才解除该进程内栅栏。

观察连接即使已看到 COMMITTED，也不解除执行栅栏：执行连接可能仍在 Provider
后处理。完整原意图经原执行连接重放，才能越过该连接的前序协议队列。STALE、
CONFLICT、boot 变化和历史缺失不能证明旧动作已结束。新连接/新客户端亦不能
证明其他连接或 VM 动作已完成；外层持久意图、唯一执行者、issuer 租约、结果
历史及最终 VM fencing 仍必须实现。SDK 不自动创建新 ID 或重试变更后的请求。

## 有类型的事实校验

响应拒绝未知/重复字段、缺失必填的 nullable 字段及有损数值表示；64 位字段统一
使用规范十进制字符串。检查完整实例/key、L/H/M、generation、四数组唯一性、
操作阶段/原因/取消窗口、释放身份、Provider 状态和偏移加法溢出。

receipt.result 与 observed 分别验证各自身份，历史请求结果可以与新的当前操作
并存，不能用 observed 的成功补成本请求成功。旧 release 的 VERIFIED 可以与
后来 Grow 的 allocation_unknown 并存，不能因旧释放事实扣减新分配预算。
NONE/PENDING/WAIT_CHECKPOINT 必须保留 Provider unknown；物理成功须有真实
结果、实际影响字节且无 partial。全体事实都只描述 PG/Provider，不生成 VM 许可。

## Mac 实际验证

使用 PG17 `48633803b05c1b7bd9a40d749b5278df9aaf01ac` 的原生 debug/assert/
injection 二进制和已安装完整 Neon 扩展1.7，不使用模拟 SQL 服务。每个集成用例
建立自己的临时集群，显式目录权限0700；异常/失败保留数据目录和服务端日志，
成功才删除。测试不读取现有数据库连接串。

| 检查 | 实际结果 |
|---|---|
| 默认 compute_api / compute_tools 全包 | 32 / 23 项通过；零失败/ignored |
| feature 开启的普通运行 | 32项通过，8项原生集群用例按设计 ignored，不能据此记为实际执行 |
| `ci/test-dsg-rust-client.sh` 严格原生门禁 | 8项实际通过14.19s，零ignored/filtered |
| feature 开启 `--no-deps --all-targets` Clippy `-D warnings` | 退出0；remote_storage 既有27条依赖 warnings 仍可见 |
| Rustfmt1.88、Bash语法、diff及三处PG版本绑定 | 通过 |

8项真实集群用例覆盖：Grow→Shrink→Grow 数据校验与历史、带引号 schema/恶意
search_path/临时目录表遮蔽、执行阻塞时独立取消、丢弃 future、提交后 ERROR、
实际响应超时及原意图不可变、backend FATAL 后原意图重连、新 boot 的 STALE，
以及错误身份、同会话、扩展成员被移除、租户直接/继承 schema CREATE 的拒绝。
工作集实际大于 bootstrap，按行数/总长/全 payload MD5 比较；不只验证空池状态。

严格脚本显式 `--include-ignored`，校验列出数与通过数完全相等且非零。PR宿主
原生门禁在完整安装后运行它；本批没有远程 Linux CI 执行证据。默认 DTO 使用者
不引入 SQL runtime，Cargo.lock 仅新增本包依赖关系，没有升级第三方版本。

日志相对工作区根：`build/neon-dsg-client-{packages,default-final,strict-final,clippy}.log`。
这批未改 PG/C SQL，不重复将既有69项内核、47项完整Neon、off/on world计为新增。
Linux/KVM/HugeTLB/物理回收及目标性能仍按 V3 后验；未完成的运行时接入和预算
属于代码剩余项，不能归入 Linux 环境限制。
