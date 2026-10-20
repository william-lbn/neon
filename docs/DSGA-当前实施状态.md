# DSGA 当前实施状态

更新：2026-10-20（Asia/Shanghai）。设计以 V3 为准；本文件只保留当前事实，历史测试见模块记录。

最新完整性审查见[代码完整性复核](DSGA-代码完整性复核-20261019.md)：整体未完成，
其中确认的 Grow/FATAL 缺陷已由36f1ed0修复并补5项永久回归；仍不能把模块测试通过当作生产可用。

## 当前目标与完成边界

按项目所有者确认的 V3 继续实施，先完整完成 PG17，再逐版移植。
M2f 引用/锁序、S2 checkpoint 工作区和安全bootstrap/新boot准入，以及 R05 IO/CV/恢复快速路径子片已完成 Mac
模块验证。S3 尾部排水、逻辑 Shrink、逐数组保守回收与 regrow 已完成 Mac 模块验证；
完整运行时私有/后台预算、外部 API 和
Monitor/Agent/NeonVM 控制链仍待实施。不能将这些子片通过解释为生产功能完整交付。

## 分支与维护基线

- PG：`codex/dsga-pg17-v3-20261019`，当前
  `864d32d9a8cfb23c71ea73caccc2c80360fb18bb`，包含S3 `7a1ff181…`、独立SLRU能力
  `76cfb197…`、关机restartpoint持久化修复 `e02a09ad…` 及TAP fixture适配，包含安全启动/准入 `f563e301…`、Grow清理修复
  `36f1ed0fcf003adeba1e7307ad036f3998bcb235`，依赖 S2 `9aebedf…`、M2f `b5785f8…`。
  PG 依赖已推送并核对远程完整 SHA，Neon 再提交/推送引用。
- Neon：`codex/dsga-neon-v3-20261019`；V3 `2422ce9`、构建身份修复 `9df2ba8`、
  M2f 集成 `4cb4e9f`、S2 集成 `a53c447`、安全启动接入 `de8f02a`；本批绑定上述 S3 PG 提交并加入16项真实场景。
- Autoscaling：`codex/dsga-autoscaling-20261019` / `ef6cd154aa3e60d72cab1e6b64abeb217ff9b245`，
  尚无 DSGA 业务改动；不能将文档分支当作 Agent 已接入发行。
- 独立维护已验证/中文提交 `4a39412f154796461f404785d369a3dc9424fcdc`，包含 Neon PG17
  `56692dfb680281a963c7470fc7f0fec7f65ecfd4` 及社区 PG17
  `91246c72695cdddf62003f6bf4c4ade511f134f4`，由 `873715d889` 合入 V3 功能分支。
  最新两项为反解析/分区聚合测试修复；6个官方文件与上游一致，无文本冲突。
- 本轮同步快照：Neon main `fa504217c61bbcaf5c512d75830564541f917f8f`、Autoscaling main
  `a4bce09e4d6bd7932ea634a1ed79afe26a9cfd1f`；PG14–18 官方维护头已 fetch。
  其他 PG 的发行升级/功能移植尚未完成，每版实施前必须重新核对，不承诺遗漏时点后的更新。
- 自有主分支/稳定分支未合并，无生产发布。功能仍默认关闭。

## 当前验证事实

| 检查 | 实际结果 | 边界 |
|---|---|---|
| 最新官方维护独立 check-world | Mac79组/660项报告检查通过 | TAP未启用，不是DSGA语义完成度 |
| 纯 M2e + 新维护组合 | 核心225、isolation123通过 | 不包含后续业务子片 |
| M2f 分配/锁序 | 实际CTAS1200行、Buffer ID边界、上下界拒绝；模块1regress+4isolation通过 | L栅栏不等于H缩小 |
| M2f 主仓集成 | b5785f8实际二进制及配套库9项通过，23.938s | 含最后unpin探针和原生pin waiter三路径 |
| S2 开关完整回归 | 开/关各81组/665项报告检查通过 | TAP未启用，未包含恢复预算 |
| S2 主仓集成 | 9aebedf实际二进制及配套库12项通过，32.167s | 含真实checkpoint并发Grow及两处ERROR |
| R05 开关完整回归 | 开/关各81组/665项报告检查通过，零not ok | 不含真实Shrinker/物理回收 |
| R05 最终候选真实集群 | 18项通过，48.098s | 5项IO/CV交错及1项实际物理备库WAL快速删除，连同原12项 |
| R05 主仓最终身份/集成 | c47f959实际二进制及重编配套库18项通过，41.645s | 已核对完整SHA，三处版本绑定一致 |
| S3 最新维护组合专项 | 45项真实集群通过，86.968s；原生1regress+5isolation通过 | 真实数据/尾部pin与IO/失败读/取消/FATAL/保守预算/regrow |
| S3 最新维护组合基础全量 | 开/关各81组/666项报告检查通过 | TAP未启用；不证明VM许可或物理能力 |
| S3 主仓最终身份/集成 | `7a1ff18108…`准确SHA重编、配套库重编后45项通过，84.343s | gitlink/两份锁一致；专项入口零skip |
| 最新存储接口/内核集成 | `864d32d9a8…`准确SHA重编后45项通过，86.415s | 含真实关机restartpoint修复 |
| 完整Neon扩展 | 清理旧ABI对象、构建/安装/实际加载后22项通过，41.137s，零skip | 含有效tail提示跨Shrink、SLRU目录/身份边界及网络协议fixture；不是实际Pageserver端到端 |
| 关机restartpoint反向/正向 | 旧版真实缺表；修复后开/关各4个TAP文件75断言通过 | 核心缺陷已修复，不再归为未定位 |
| 额外TAP维护基线 | 整体失败，8个文件失败 | 未含DSGA的维护组合；详细清单见下，不包装为通过 |
| Linux普通页/KVM/HugeTLB/Neon存储端到端/性能 | 未执行 | 后续按T15–T22单独验收 |

R05 的 ERROR 审核点仍持Guard和自身pin；FATAL 审核点已无Guard但仍有自身pin，
两者均已从CV退队。分别覆盖IO owner完成并放pin之后，以及owner尚未广播时的
取消/终止。快速路径由真实备库WAL重放触发，没有伪造InRecovery/size cache。
只关闭这两个引用缺口，不据此关闭整个R05或直接允许VM缩容。

日志在工作区 `build/check-world-v3-io-lifetime-{off,on}.log`、
`build/v3-io-lifetime-lifecycle-final.log`、
`build/neon-integrated-v3-io-lifetime-lifecycle.log`；其他历史证据见主仓M2集成记录。
pgindent/Ruff/diff通过；prepare为13项逻辑通过、18项环境skipped，后者已另行真跑。

## Grow/FATAL缺陷的修复状态

Grow 的 FATAL 清理在原生释放 resize_lock 后，没有核验本操作身份就修改当前共享
operation。本次真实临时集群复现：旧操作退出把新操作从PREPARING改为DENIED，
新操作随后COMMITTED却残留INTERRUPTED原因。该缺陷已通过操作身份绑定与条件重获串行锁修复，新增5项永久交错；
23项最终候选测试通过（49.377s），实际新SHA主仓二进制23项通过（45.714s）。
开关check-world各81组/665项通过；prepare13项通过/23项环境skip，已另行实跑。
原始复现日志仍保留，完整性复核是修复前快照，不能用历史“未修复”覆盖此状态。

## 恢复注意与下一阶段

实际构建 `build/neon-integrated/v17`，安装 `build/neon-integrated-install/v17`。
版本锁变更会使旧configure缓存失效；每次仍须实际核对postgres完整SHA并重编配套库。
PG内部 API 为4。具体设计与约束见子模块：

- `DSGA-M2f-引用寿命与锁序验证.md`
- `DSGA-S2-checkpoint工作区寿命与并发验证.md`
- `DSGA-R05-IO等待与恢复快速路径验证.md`
- `DSGA-S2-安全启动与新boot准入.md`
- `DSGA-S3-尾部排水与逻辑缩容验证.md`

Grow/FATAL已修复并完成本地验收；安全bootstrap、同Postmaster恢复共享布局预算和新boot准入默认状态
已完成本机候选验证，详见[Compute接入](DSGA-Compute安全启动与准入接入.md)：真实集群29项、
开关check-world各81组/665项、compute_api12项、compute_tools全包22项通过。
不能据此关闭R02全部运行时预算约束。接着推进完整
请求回执、有界历史、API、完整预算/准入和外部控制链。所有进程/扩展的剩余引用
与长Guard扫描活性继续按V3审查；Linux物理能力和完整端到端另验。

S3 保护页探针经反向验证可检出显式引入的不安全读取。仅回退 hash 冲突归还顺序时，
原 StrategyFreeBuffer 的 VA/L 防护仍使测试通过，文档已纠正，不将防御加固误报为
复现旧版缺陷。正式生产API尚未接入，测试函数不能充当对外管理接口。

额外启用的 TAP 检查发现 Mac 可复现的 rmgr fixture、序列语义及未定位恢复/逻辑槽
问题，见 [TAP基线验证](DSGA-TAP基线验证-20261020.md)的后续状态。缺表根因为
关机restartpoint遗漏写回，已经修复；最初8个失败文件中7个已有通过证据，
`pg_upgrade/003`及首次因失败而未执行的TAP仍须处理/复跑。不能宣称TAP整体全绿。

完整Neon扩展与最新PG的LSN锁、旧hook及SLRU接口兼容已实现并真实加载验证，见
[存储接口与Mac验证](DSGA-Neon存储接口对齐与Mac验证.md)。其中旧版头的语法兼容
不代表PG14–16功能已移植；正式API/预算/控制链等原有剩余范围保持不变。

M2f旧备份stash `6f002a31196f2e623e8386c38a649ff87be2ada4` 已apply一次，禁止重复apply；
旧未完成spec另存 `build/m2f-unpin-before-v3.spec`。不改isolationtester来伪造原生BufferPin等待。

## 本轮环境约定

Mac用于代码与可执行业务验证；暂不构建镜像、准备或执行K8s部署。后续到目标Linux
K8s验收，按代码/部署分支系列记录对应SHA和未验证项。镜像、NeonVM/KVM、HugeTLB、
真实物理回收与端到端/性能不能以Mac本地结果替代。
