# Neon DSGA 完整实施范围、组件边界和验收纪律

版本：V2.1。日期：2026-10-19。状态：功能分支开发中，按模块验证与提交，尚未发布。

## 1. 本轮已确认的要求

- 在你们的 `william-lbn/neon`、`william-lbn/autoscaling`、`william-lbn/postgres` 中实施。
- 补全原方案全部功能和失败场景；用真实 Neon 路径验证，不能用孤立样例替代集成。
- 跟进 Neon 官方代码和 PostgreSQL 稳定分支修复，保留你们的构建与发行能力。
- 先在 Mac 开发、完成可执行的本地业务逻辑验证，再提交代码。
- 小批量修改，逐模块完成受影响组件全量检查；提交不能混入无关模块。
- 新增关键逻辑使用详细中文注释，说明职责、不变量、锁序、边界、失败和恢复原因。
- 已有 GitHub CI 可用；真实 Linux/KVM/NeonVM/HugeTLB 环境测试后续进行。

“全量验证”指本模块全部测试及受影响集成路径的验证。Mac 无法证明 Linux
`MADV_REMOVE`、KVM 热拔或真实 Host 巨页回收，必须保留对应待验收记录，不能以
模拟测试或编译通过替代。没有通过发布门槛的功能保持默认关闭。

## 2. 需求基准及完整性的定义

以原 V1.0 的 FR-01 至 FR-07、NFR-01 至 NFR-05、I1 至 I9 和明确的首期边界为
强制交付范围。DSGA 的地址、Provider、Granule、协调和 Buffer Adapter 均须有
真实实现。不能仅修改 GUC、LFC 限制或 VM 内存，再声称 Buffer Pool 已动态化。

| 原始要求 | 必须实现的行为 | 验证责任 |
|---|---|---|
| FR-01 | 保持连接的 Shared Buffers Grow/Shrink，尾部忙时拒绝 | 内核+Neon 集成 |
| FR-02 | 目标、可分配、已初始化、映射、物理提交与可释放状态分离 | 内核+Monitor |
| FR-03 | 幂等、取消、阶段失败、部分完成和重连对账 | 内核+Monitor+Agent |
| FR-04 | Backend/Worker 加入退出与 generation 安全 | 内核 |
| FR-05 | 普通页/HugeTLB Provider 能力与拒绝路径 | 内核+NeonVM |
| FR-06 | 配额、迁移互斥、LFC、Pageserver、主/只读回放兼容 | 三仓集成 |
| FR-07 | Linux amd64 首期支持；aarch64 单独验收 | 构建+环境矩阵 |
| NFR-01/02 | 不损坏数据、不错误释放、不因正常变配主动断连 | 全链路 |
| NFR-03/04 | 热路径开销可控，TPS/P99/CPU/GetPage/内存有对照 | 性能测试 |
| NFR-05 | 分级开关、停止缩容、受控重启回滚 | 生命周期+发布 |

原文明确首个生产版本只有 Buffer Pool Adapter，其他共享内存组件预留接口，
不在首期重写 WAL Buffer、ProcArray 或锁表。文章还介绍的自动 RO 横向增删、
跨节点连接保持等能力，要有独立需求与控制面设计；不能把内存改造冒充这些能力。
配套 RFC 已补充其接口和验收边界，便于后续完整产品化。

## 3. 锁定的组件修改范围

| 模块 | 仓库/位置 | 具体职责 | 不跨越的边界 |
|---|---|---|---|
| M0 官方补丁基线 | postgres 稳定支、三仓维护工具 | 合并维护修复、保留 Neon 差异、固定依赖 | 不混入动态 Buffer 功能提交 |
| M1 WAL 兼容适配 | postgres heap WAL；neon wal_decoder/neon_rmgr | 新 VM block reference 与旧 WAL 双向兼容、恢复测试 | 不改写 WAL 权威来源 |
| M2 DSGA/Buffer 内核 | postgres storage/ipc/buffer、GUC、目录函数 | 稳定 VA、L/H/M、Provider、Granule、Grow/Shrink、barrier | 不在每次缓存命中上增加全局大锁 |
| M3 Compute 生命周期 | neon compute_tools 与配置 | 能力发现、启动初值、重启预算、开关与版本 | 不用旧配置突破 VM 实际预算 |
| M4 vm-monitor | neon libs/vm_monitor | PG 客户端、资源预算、幂等/身份、异步状态、LFC 协同 | 无内核证明不许可 VM 缩容 |
| M5 Autoscaling | autoscaling agent/core/executor/bridge | 扩展现有 Revision、策略约束、重试和对账 | 不另造第二套并行控制状态机 |
| M6 NeonVM/巨页 | autoscaling runner/controller/plugin | virtio-mem 实效、资源预留、HugeTLB Provider 与迁移互斥 | spec/PATCH 成功不等于资源已回收 |
| M7 验证与发行 | 三仓 tests/CI/docs | 回归、故障注入、兼容矩阵、锁文件和镜像清单 | 未通过门槛不启用生产开关 |

Pageserver/Safekeeper 的持久化协议首期保持既有语义，但必须运行存储集成回归。
官方维护补丁如涉及 WAL，允许修改其读取/解码兼容路径，并单独评审与验证。

## 4. 版本和上游跟进

当前主仓只有 PG14–17。首期选择 PG17；不能把 `REL_18_STABLE_neon` 分支存在
当成发行已支持 PG18。原主仓 PG17 锁为 17.5，本轮已在独立功能分支更新到 17.11。

本地持续同步 PG 官方 `REL_17_STABLE`，当前固定提交
`94035972af3f10cdaf9ff2b5d232ad5f45463469`（维护版本标识 17.11）。
已合入你们的 PG17 维护分支，最新维护合并提交为
`1509896b1f83a087b97fa5c15288c0283660135d`。该配置的 Mac check-world 通过，
维护及业务分支均已推送到自有 fork，主分支没有合并；维护验证范围详见
`DSGA-M0-M1-本地验证记录.md`。当前业务实现与真实主仓构建验证见
`DSGA-M2-主仓集成验证记录.md`，最新源码以 gitlink 与两份版本锁为准。
同日新增优化器修复的独立维护与功能验证，另见本仓上游同步规范及 PG 子模块
`UPSTREAM-PG17-合并记录.md`，不将它混记为首次基线的测试。

2026-10-19 审计确认：自有 Neon、Autoscaling main 均包含当时全部官方 main 提交；
PG17 维护基线同时包含 Neon 官方对应稳定分支与社区维护头。PG14–16 的发行 gitlink
仍沿用官方主仓锁，不能据“自有 stable 分支已同步”宣称全部发行版本已经更新。
各分支 SHA、祖先检查与后续同步规则见 `上游同步与功能分支规范.md`。

每次更新同时检查：官方稳定修复、Neon 专有差异、Neon 主仓 gitlink、
`ci/distribution.json`、Autoscaling SHA、协议版本和镜像 digest。
PG14–16 的维护更新独立处理，不把 PG17 动态 Buffer 代码批量复制到其他大版本。

维护工具应只读比较、生成候选更新和审查报告；不能未经验证自动合并到生产分支。
社区实验性 Buffer 补丁是算法/测试参照，与官方稳定安全修复分开跟进。

## 5. 内核关键决定

- `L` 是新 victim 分配边界；`H` 是已初始化且允许受保护访问的边界；`M` 是本次
  Postmaster 生命周期的固定上限。稳态 `0 < L = H <= M`。
- 缩容先降 L，既有尾部 pin 仍有效。不得拿 L 替换所有 Buffer ID 合法性检查。
- hash 命中重新 pin、private refcount、ring、recent-buffer hint、数组遍历分别
  审查。最终 tag 删除必须与新 pin 的取得互斥，并重新检查 dirty/IO/refcount。
- 原生 ProcSignalBarrier 负责通知与进程生命周期协调；ACK 只在相应安全动作完成
  后产生。数组读侧保护与 pin 排水不能被 generation 字段替代。
- BufferBlocks、Descriptor、IOCV、checkpoint workspace 和 hash 各自记录容量。
  不能按粗略“减少了多少 Buffer”推断所有内存都已归还。
- grow 发布前完成初始化；shrink 提交 H 后才解除物理提交；页边界向安全方向取整。
- 物理释放失败可留下较大资源占用，不得恢复已经失去 backing 的 ACTIVE 状态。
- 协调角色先与现有内核机制结合，避免提前引入独立 MMAN 崩溃恢复模型。

## 6. 跨层协议与恢复

操作身份包含 VM UID、Compute 实例、PG boot 身份和单调 Revision。重启、重建、
领导者切换或新操作均使旧许可失效；最终 VM 写入也要检查身份，不能只在 PG 端检查。

扩容：Host 预留 → VM 实际增加 → Guest 可用 → PG 提交 → 上层确认。

缩容：PG 停止新尾部分配 → pin/IO/tag/遍历排水 → PG 物理释放 → Guest 池/预算
处理 → virtio-mem 实际完成 → Host/Scheduler 释放承诺。

必须区分逻辑提交、物理待释放、VM 待缩和 UNKNOWN。超时后读实际状态对账。
PG 缩小但 VM 没缩成是安全且有额外成本的状态；不能以成功标签掩盖它。

PG crash 后按当前 VM 实际容量计算启动初值。禁止外层已缩到小规格，PG 却按旧的
较大 `shared_buffers` 重新启动。缩容许可发出后至 VM 完成前仍要维护目标预算。

## 7. Mac 本地验证与提交顺序

每个模块建立一个记录，包含修改文件、目的、关键不变量、测试命令、实际结果和
待验收项。先做只读 diff/格式检查，再做模块测试、受影响组件全量测试与恢复测试。

| 批次 | 本地验证 | 提交条件 |
|---|---|---|
| 官方 PG 基线 | 编译、PG regress/isolation、Neon rmgr 兼容恢复 | 冲突语义已审查，测试通过 |
| WAL 适配 | 老/新 WAL、同/不同 VM 页、FPI、恢复与 Rust 解码测试 | 先读者后写者的发布约束明确 |
| DSGA/Buffer | 断言构建、确定性 pin/IO/扫描/barrier 测试、取消和重启 | 不能只有无并发的扩缩样例 |
| Monitor | Rust 单测、mock PG 故障、权限/超时/重连/预算 | 全模块测试与旧协议兼容 |
| Agent/VM | Go 单测/race、旧许可、重试、实际状态与资源对账 | 受影响包全量验证 |
| 端到端 | Neon 本地开发集成与跨版本组合 | 不能拿 mock 替代存储集成 |

Mac 不支持的系统调用明确拒绝或使用独立测试 Provider，绝不能伪造物理回收结果。
Linux、HugeTLB、NeonVM 的真实能力测试后续补齐后，再开启对应发布开关。

提交按模块拆分，说明具体行为、兼容影响与验证结果。新增代码的中文注释重点
解释为什么需要此锁/顺序、何处可以取消、失败后哪些事实仍有效。对同步来的官方
源码保留原作者和原注释；对我们解决的语义冲突增加中文说明，避免大范围翻译噪声。

## 8. 必须构造的故障场景

hash-hit 重 pin；tag 已登记而读未完成；扫描保存旧 H；fork/exit 与 barrier 交错；
旧 hint/ring 经 shrink/regrow；各数组部分 populate/remove 失败；协调者 SIGKILL；
Monitor/Agent 重连；VM 缩小后 PG crash；旧 Agent 延迟许可；私有查询内存突增；
HugeTLB 页只归还池而未减少 VM；FSM/VM、UNLOGGED 和建索引交错。

使用测试扩展主动持有指定尾部 pin 和 injection points 暂停关键阶段。空闲长事务
不能自动构成 pin 测试。数据结果、索引、checksum/amcheck 和重启后恢复一起验证。

## 9. 当前进度事实

- 已下载三个自有 fork；未修改其他已有本地仓库。
- 已重新核对官方主仓和 PG 维护分支，并建立独立实现分支。
- 已补全架构 RFC；本文件锁定中文范围、质量要求和验收纪律。
- 官方 PG 维护补丁已完成本地验证并形成独立合并提交；Neon 兼容改动另行提交。
- 尚未推送、创建 PR、发布镜像或修改生产配置。
- 后续进度以实际代码与测试记录更新，不把设计完成写成代码完成。
