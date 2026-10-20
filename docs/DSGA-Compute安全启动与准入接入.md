# Compute 安全启动与新 boot 准入接入

更新：2026-10-20（Asia/Shanghai）。按 V3 R01/R08 实施的 PG17 候选模块。
PG17 S3及正式Neon管理SQL已完成本机模块验证；完整S4预算/控制链仍未完成，
不应在业务部署中启用自动VM缩容。受管模式须经完整管理链核对预算后打开准入。

## 启动契约

ComputeSpec 可选 `dynamic_shared_buffers`，未提供时保持官方静态行为。示例只用于测试，
不是生产规格默认值：

```json
{
  "protocol_version": 2,
  "vm_uid": "dsg-vm-1",
  "compute_instance_id": "dsg-compute-1",
  "bootstrap_buffer_bytes": 2097152,
  "maximum_buffer_bytes": 67108864,
  "minimum_guest_memory_bytes": 134217728,
  "non_buffer_reserve_bytes": 67108864
}
```

v2要求显式VM UID/Compute实例ID，均为1–128字节可见ASCII，由认证控制面分配，
不猜测名字或从备份继承。旧实验v1须补身份并受控重启；普通未启用DSGA的spec
保持兼容。控制请求协议仍为1。详见[管理SQL与身份](DSGA-Neon管理SQL与实例身份.md)。

bootstrap/max 必须按 BLCKSZ=8192 对齐并可由 PG 表示。最低 Guest 减去显式 non-buffer
reserve 得到 PG 共享预算，转换 kB 时向下取整，不能吞掉 reserve。reserve 须由部署清单
约束 OS、连接/查询/并行/维护/扩展、LFC、页表等；填写数值本身不构成资源硬保证。
Rust 做参数检查，PG 在 preload 请求全部登记后根据真实结构尺寸、固定 M hash/目录、
主共享段及四数组 bootstrap 页界作最终布局检查；不能用固定百分比估算替代。

Compute 在解析时验证，配置生成在打开/截断原配置文件之前验证。受管项同时写入
postgresql.conf 和 Postmaster `-c` 参数；命令行优先于备份中的 auto.conf 及官方末尾
include。运行时 H 不写回启动契约。HTTP reconfigure 和 HCC refresh 均拒绝运行中
改变受管启动项或实例身份，避免将需要重启的配置误报为SIGHUP成功。

## PG 恢复与权限

启动准入初始依赖f563e301…/API3；当前PG依赖48633803…/Buffer ABI5保留并扩展
这些语义。新共享内存
boot（含同一 Postmaster 内部 crash recovery）回到 bootstrap、生成新身份，并默认关闭
普通 SQL 准入。内部 PG 超级用户保留恢复/管理通道；Neon 的普通/弱超级用户角色不能
自行放开门禁。普通请求拒绝为 55000 ERROR，保持连接；Sync/Close 等清理继续执行。

管理准入核对当前 boot/revision/capacity generation，过期与跨 boot 请求拒绝。
原生简单/扩展/FastPath 消息入口与简单 Query 的逐语句检查均覆盖，避免 SET ROLE
在同包内改变身份后绕过。正式管理SQL的身份/回执/取消/本boot历史已接入；外层持久意图、运行时排空和
后台预算仍由后续S4完整实现，SQL/内核接口均不直接授权VM缩容。

## 本机证据

最新身份v2批次：Rust compute_api24项、compute_tools23项通过；正式SQL/完整
Neon47项通过85.043s，包含CLI覆盖非法备份配置、身份固定/拒绝非法启动。
以下为最初安全启动批次历史记录，不覆盖后续已完成TAP或新增验证。

macOS arm64、Xcode clang、MacOSX14.4 SDK；PG debug/assert/depend/injection-points。

| 检查 | 实际结果 |
|---|---|
| 真实 PG 临时集群 | 29 项通过，59.600s；含新增六项启动/协议/内部恢复测试 |
| PG check-world，开/关 | 各 81 组/665 项报告检查通过，零 not ok；TAP 因缺 IPC::Run 未启用 |
| Rust compute_api | 12 项通过，含五项契约/预算/重配验证 |
| Rust compute_tools 全包 | 22 项通过；含三项真实配置文件生成/保留验证 |
| Python prepare | 13 项通过、29 项环境 skip；29 项已另行真实执行，不能相加或把 skip 算通过 |

日志在工作区 `build/v3-bootstrap-*`；PG 详细锁/恢复说明位于子模块
`DSGA-S2-安全启动与新boot准入.md`。主仓绑定依赖后还要重新构建完整 SHA，执行
`ci/test-dsg-postgres.sh` 核对锁定二进制与真实套件。本批已重建并核对
f563e3014c 完整 SHA，脚本实际运行全部 29 项通过（51.721s），无 skip。

PR 新增 dsga-pg17 专项 job：检出精确子模块、构建宿主 PG/配套库、真实执行全部用例。
脚本拒绝零测试、任何 skip、缺 injection 能力及错误二进制 SHA。此 job 的 GitHub
实际运行结果待远程执行，不以本地脚本验证冒充 Linux CI 通过。

完整预算/控制链、实际客户端和本机真实Neon存储端到端仍是代码/集成剩余项，
不能归为环境待测。Linux物理页/HugeTLB、目标VM恢复、K8s/NeonVM/QMP及目标
性能长稳后续按V3认证。本轮不构建镜像或执行K8s部署。
