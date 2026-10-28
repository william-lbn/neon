# 内存伸缩核心进度、Mac证据与Linux待验清单

本清单是2026-10-28当前状态，不是按提交数计算的完成率。PG17核心仍有代码
工作，尚不能封版或声称生产可用。所有者新增长期顺序：PG17核心收尾及Mac
验证归档→PG18→PG16；PG14/15、独立控制面仍暂缓。2026-10-26上游冻结
继续生效，只修改自身缺陷和剩余功能，不重新追新。

## 已归档代码与当前工作树

| 仓库 | 固定功能分支 | 已归档提交 |
|---|---|---|
| Neon | codex/dsga-neon-v3-20261019 | d2b5911a3dfb6d522614ce4a20e74301dd412657 |
| Autoscaling | codex/dsga-autoscaling-20261019 | 316d5e698fed473d2a3d585d8af546654b036ea3 |
| PG17 | codex/dsga-pg17-v3-20261019 | b078736a4cfdfccce69914feefb5b5e56560c1e8 |

这些是本次开始时已交付且已有远程核验的版本；当前新增取消恢复尚未提交，
**不能按已归档HEAD将工作树候选算成已交付**。PG17实际源码在
vendor/postgres-v17，gitlink、ci/distribution.json、vendor/revisions.json一致。
独立控制面原分支保持d84c0b73004a7d8cc3e89f3c3f0e3f06cfa8cba9，不修改其业务。

## 已完成并有Mac采用证据的范围

| 核心模块 | 当前已实现与验证 | 准确证据入口 |
|---|---|---|
| PG17动态Buffer/Provider/生命周期 | Grow/Shrink、保守成本、pin/IO/遍历/checkpoint、同Postmaster恢复、准入/连接/工作/DSM/2PC及冻结维护已有模块证据；本批未改PG，不重跑 | [冻结与PG17验收索引](DSGA-冻结基线与封版清单-20261026.md) |
| 持久意图与执行权 | 原实例/出生/owner、UID/RV WAL、独立领域fence、未知先查/显式原重放；不是开放产品入口 | [原容量取消恢复](DSGA-原容量取消恢复-Mac验收-20261027.md) |
| 成功扩缩与Host结清 | 实际Go Agent/Runner/QMP协议、预算/准入组合，Scheduler容量/峰值/Through与防迟到重占、部分GC | [Host实际容量结清](DSGA-Host实际容量结清-Mac验收-20261028.md) |
| 部分效果取消 | 实际VM已扩/PG未扩、自动实际预算/Cleanup/Finish/Host；自身部分Shrink、旧大VM资格、历史淘汰及后继正常操作组合 | [自动恢复](DSGA-自动实际预算恢复-开发记录-20261028.md)、[部分Shrink组合](DSGA-部分缩容与后继操作-Mac验收-20261028.md) |
| 完整Prepare生产 | 原可信平台资格驱动Prepare/Outside v2生产及消费，Start/Create/Record丢回复与SDK重建 | [可信Prepare](DSGA-可信Prepare预算生产-Mac验收-20261028.md) |
| 首次预算与开放 | VM v9永久Initial/WAL、原setter与独立原重放、完整首次到后继Grow、原来源漂移拒绝/恢复 | [首次服务生产](DSGA-首次服务生产与原准入恢复-Mac验收-20261028.md) |
| 初始Host与零派发取消 | 实际Publisher/Scheduler核原Initial/Runner/Node；历史淘汰保留完整首次预算。该观察分支不退placement或峰值 | [首次Host验收](DSGA-首次Host持续核验与零派发取消-Mac验收-20261028.md) |

各模块收据内的顶层/子场景计数只对其采用范围有效，不累加成完整产品E2E。
Go协议组合使用明确PG/root/平台资格/Kube/QMP夹具；本地PG与Rust模块另有
真实执行证据。不能把夹具执行的物理动作外推为真实Linux证明。

## 当前正在收尾的代码与验证

首次真实VM命令未发送QMP即取消：实际VM512MiB、原目标1GiB，原Initial资格
必须能继续支撑实际预算。候选实现显式ReconciledCapacity v2/InitialOutside，
保存完整原初始预算，与旧OutsideRequest互斥；不改原目标、不伪造旧Resize或
VM命令。完整Cleanup与InitialHeld→CapacityHeld替换同CAS，Host按实际容量结清。

已采用、未受后续测试夹具修正影响的结果：

- budget/budgetclaims/journal/admissionexec定向回归21顶层/107含子记录，0fail/0skip。
- 旧Grow partial-auto与Shrink restore-prior组合2顶层/4含父记录，0fail/0skip。
- 受影响六包lint为0 issues，go vet通过；静态检查不算业务测试。

新增完整首次→取消→自动Cleanup/Finish→Host实际结清→后继Grow→下一次
Shrink Prepare尚无最终采用通过。最后诊断调用已走到后继正常扩容及末尾Prepare，
在120秒测试总期限到期，日志为build/dsga-initial-cancel-combined-diagnostic2.jsonl。
此前Guest响应不确定错误被局部err遮蔽的问题已补候选修复，并补准确ClientError
断言；完整组合仅扩大测试总上限至3分钟，不改生产请求/租约/单轮期限。

新网络组合和上述错误回归因工具自动审批额度限制尚未执行。失败日志不是通过，
候选代码尚未提交；恢复执行后只跑这些新增/受影响场景，不能跳过后推送冒充完成。
本地的build目录是执行日志，不是远程已归档证据。

## 仍须实现或补足组合验证的核心功能

| 顺序 | 工作项 | 当前不能签收的具体边界 |
|---|---|---|
| 1 | 首次部分效果取消与Host峰值 | 上述真实VM未发送取消候选待最终验证；Prepare/Host后VM尚未派发且无旧Native命令的恢复仍须独立真实来源，不能伪造命令 |
| 2 | 来源与异常生命周期 | 缺原来源/资格、Root不一致的受控恢复；Outside/资格/Runner等完整持有与回收；不能靠永久保守持有掩盖泄漏 |
| 3 | 安全降低normal与活性 | 当前资源执行器明确拒绝降低memory.max；完整后台/私有内存持续上界及安全降限协议仍有代码工作，不能只补成功断言 |
| 4 | 启动/接管/跨boot恢复 | 真正VM owner切换的完整组合；无Wait、Start/init失败；跨boot/迁移受控恢复，不能自动改绑原出生 |
| 5 | 持续控制与入口 | Monitor/自动策略、可信请求/取消/重放生产入口，全部写者与CPU隔离接线，仍须代码及组合 |
| 6 | PG18与PG16 | PG17核心归档后分别移植与测试。PG18尚缺Neon发行接入，不能仅改PG17版本宏；PG16尚未移植 |

这些是代码或Mac组合缺口，不能写成仅待Linux验证。独立控制面由另一任务完成后
移植统一产品E2E，本任务不追它的main或提前扩展其SDK。

## Mac通过后，Linux还必须验证什么

纯状态机、严格JSON、代次/身份、预算运算、WAL恢复、防重放和业务顺序可复用。
-race仅覆盖本次执行到的并发访问。已归档版本还曾编译准确源码的四个服务×
Linux amd64/arm64共8个ELF，核架构/VCS/摘要；未执行，不能冒充Linux测试。

| 范围 | Linux/K8s目标环境必须执行 | 通过前的不变量 |
|---|---|---|
| PG真实Provider | 普通页/各HugeTLB页型、对齐/边界、预触/失败尾部、HVO/池/保留成本、释放/重长并发 | 逻辑Buffer下降不等于物理页释放 |
| Guest资源和出生 | cgroup v2、swap、全部hstate、聚合去重、UID/SUID/能力/PID/归组及root文件保护 | 启动/准入须完整真实约束，不回退legacy |
| NeonVM/KVM/QEMU | virtio-mem requested/actual、Guest可用容量、不可迁移页、失败/超时、原QEMU退出和持久日志恢复 | PATCH或QMP请求成功不等于VM已缩小，不先退Host |
| K8s执行权 | RBAC/SA/TokenReview、Lease接管/过期、UID/RV冲突、分区、乱序、Runner重启 | 原未知先查、旧owner不能迟到派发 |
| 承诺与GC | 多VM并发、峰值/Through、跨Node迁移、终止/失联Pod、来源及finalizer回收 | 不按超时自动释放，也不永久泄漏 |
| 存储/负载/性能 | 真实pageserver/safekeeper、WAL/IO/热点/LFC、并行/维护/扩展、OOM/磁盘满/主机故障 | 逐项检验数据恢复和预先约定SLO |
| 长稳及版本 | 连续扩缩、接管/重启/取消/对账，PG18 AIO及PG16各自场景，控制面移植后的产品E2E | 真实目标环境通过后才申请合并/发布 |

Mac的mmap/释放、身份/文件锁/持久化/信号/调度与Linux不同。逻辑Buffer、PG
数组、Guest可用内存、VM实际容量和Host退还分别取真实回执；Mac结果不能
保证Linux无需测试。功能仍默认关闭，不构建镜像、不准备/执行部署、不合main。

## 固定收尾、归档和加速规则

1. 每阶段完成代码、受影响Mac验证、中文注释/说明后立即模块化提交：先推
   autoscaling，再更新Neon依赖锁并推送；核远程SHA，PG17三处绑定保持一致。
2. 同一次组合覆盖成功、丢回复、SDK重建、漂移、取消、Host结清及后继请求。
   仅新增/变化/受影响场景重跑；未改PG/Rust和通用Go逻辑复用准确已有证据。
3. 归档列出源码SHA、测试映射、日志摘要、实际fail/skip/未执行、Linux边界和
   剩余代码；提交未发生或测试未通过时明确写候选，不包装成已交付。
4. 当前阶段按上表固定缺口收尾；PG17完成后PG18→PG16。稳定公共协议复用，
   版本专属内核/扩展/构建与集成不能复用另一大版本的正确性证明。
5. 持续目标保持active；阶段归档不是整体完成。平台工具额度或用户中断仍可能
   阻止即时执行，恢复后读取具体进程终态和日志续跑，不盲目启动重复测试。
