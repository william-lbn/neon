# 内存伸缩核心进度、Mac证据与Linux待验清单

更新：2026-10-29（Asia/Shanghai）。本清单不按提交数量计算完成率。PG17核心
仍有明确代码工作，不能封完整版本或声称生产可用。所有者确认长期顺序：
**PG17核心实现、Mac可执行验证和阶段归档 → PG18 → PG16**；PG14/15与独立
控制面暂缓。2026-10-26上游冻结继续生效，不重新追新，不重复未变功能。

## 当前阶段归档与准确依赖

| 仓库 | 固定功能分支 | 本次采用准确提交 |
|---|---|---|
| Autoscaling | codex/dsga-autoscaling-20261019 | aebb209a327692c85d048362f48f64d18299e212 |
| PG17 | codex/dsga-pg17-v3-20261019 | b078736a4cfdfccce69914feefb5b5e56560c1e8 |

Neon沿`codex/dsga-neon-v3-20261019`归档本页、源码/测试收据及相同Autoscaling
发行锁；Neon自身准确提交由包含本页的Git记录和远程refs核验，避免文档嵌入自身
提交SHA形成循环。PG17实际源码在`vendor/postgres-v17`，gitlink、
`ci/distribution.json`和`vendor/revisions.json`保持同一b078提交。
独立控制面既有分支保留，未追其main、未修改业务/SDK。工作区Autoscaling的
所有者AGENTS.md环境说明修改继续保留，不与业务提交混合。

本次资格终结`c09798ea8880f40b925cc9d63fe66609670db76f`、旧Outside/Runner回收
`66e8eee20186f349be130ad3dadfe492fddb3aca`及阶段归档`17d7c744`已模块化提交、
先推自有Autoscaling原分支并核完整远程SHA。准确源码、逐用例及失败边界见
[原来源有界回收验收](DSGA-原来源有界回收-Mac验收-20261029.md)与
[机器收据](DSGA-原来源有界回收-Mac收据-20261029.json)。上一阶段从未派发取消
`33b76210`/`91b84273`/`c2bb86a9`已完成，不重新开发；其证据继续按未变范围复用。

## 最新阶段：原QEMU确定未启动的关闭和Host结清

Autoscaling业务`0710d8cdc1a7c956742c9706076732cecf190416`、归档
`aebb209a327692c85d048362f48f64d18299e212`已沿原分支推送并核对完整远程SHA；
本页、验收/收据和Neon发行锁采用相同依赖，PG17三处锁保持b078。
原启动意图已持久、同一RunQEMU明确未进入Start或Start失败且无Process/
ProcessState时，产生ProcessExit v2；PID/退出码/信号全零，不伪造Wait。
先原文件锁与内存WAL核验、fsync关闭，再发布原不可变来源；丢回复只恢复同一
原文，拒绝迟到Tick和重启。Scheduler继续要求整个原Pod全部容器终态及Node
UID/RV条件结清；NoStart不能代替sidecar停止或物理成本释放资格。

Mac -race三包12顶层/54含父子记录、0fail/skip；三包vet、增量lint0，vmguard
双Linux架构测试ELF只编译未执行。来源/源码/日志摘要和准确用例映射见
[本阶段验收](DSGA-QEMU确定未启动关闭-Mac验收-20261029.md)及
[机器收据](DSGA-QEMU确定未启动关闭-Mac收据-20261029.json)。不重复未变测试。
本批只关闭持久launch后的确定NoStart；缺原启动来源的早期init、Runner被杀
无Wait、跨boot/迁移仍须代码及组合，不能列为仅待Linux验收。下面Root阶段是
已完成历史证据，当前准确发行依赖以上表为准，整体核心未完成。

## 前一阶段：原Root已持有上界的受限恢复

Autoscaling业务`81a4a2b3255fa032dc6743a69a2e7e7fb9baf608`、阶段归档`59b66847`，
Neon正式认证路由`6d71ad239dfa038adbcd26e659a1ee6ae665f23b`已实现。本页采用
相同准确Autoscaling发行锁；未改PG17子模块和两份PG锁。
原Root的实际约束被调小，可显式选择原来源恢复；逐维拒绝实际降限/新目标。
覆盖新owner、原命令Pending、写前fsync失败不写内核、部分写及丢回复只读恢复。
Mac Go -race去重22顶层/56含父子记录、两包0fail/skip；Rust认证和UDS各1项
通过，Clippy退出0（保留既有依赖警告）；两种Linux架构测试程序仅交叉编译。
完整映射、日志/源码摘要和边界见[受限恢复验收](DSGA-原Root上界受限恢复-Mac验收-20261029.md)
及[准确收据](DSGA-原Root上界受限恢复-Mac收据-20261029.json)。不重复未变测试。

缺原来源/资格/Root日志、实际高于原约束及swap/任务/系统域异常仍需受控关闭/
恢复；本批不重建这些来源、不改变原命令。下方旧来源回收段落保留为前一阶段
证据；当前准确依赖以上表为准。PG17整体核心未封版，不报接近100%。

## 已完成并有Mac采用证据的范围

| 核心模块 | 当前已实现与验证 | 准确证据入口 |
|---|---|---|
| PG17动态Buffer/Provider/生命周期 | Grow/Shrink、保守成本、pin/IO/遍历/checkpoint、同Postmaster恢复、准入/连接/工作/DSM/2PC及冻结维护已有模块证据；本批未改PG，不重跑 | [冻结与PG17验收索引](DSGA-冻结基线与封版清单-20261026.md) |
| 持久意图与执行权 | 原实例/出生/owner、UID/RV WAL、独立领域fence、未知先查/显式原重放；尚非完整产品入口 | [原容量取消恢复](DSGA-原容量取消恢复-Mac验收-20261027.md) |
| 成功扩缩与Host结清 | 实际Go Agent/Runner/QMP协议、预算/准入组合，Scheduler容量/峰值/Through与防迟到重占、部分GC | [Host实际容量结清](DSGA-Host实际容量结清-Mac验收-20261028.md) |
| 部分效果取消 | 实际VM已扩/PG未扩、自动实际预算/Cleanup/Finish/Host；自身部分Shrink、旧大VM资格、历史淘汰及后继正常操作组合 | [自动恢复](DSGA-自动实际预算恢复-开发记录-20261028.md)、[部分Shrink组合](DSGA-部分缩容与后继操作-Mac验收-20261028.md) |
| 完整Prepare生产 | 原可信平台资格驱动Prepare/Outside v2生产及消费，Start/Create/Record丢回复与SDK重建 | [可信Prepare](DSGA-可信Prepare预算生产-Mac验收-20261028.md) |
| 首次预算与开放 | VM v9永久Initial/WAL、原setter与独立原重放、完整首次到后继Grow、原来源漂移拒绝/恢复 | [首次服务生产](DSGA-首次服务生产与原准入恢复-Mac验收-20261028.md) |
| 初始Host与零派发取消 | 实际Publisher/Scheduler核原Initial/Runner/Node；历史淘汰保留完整首次预算。该观察分支不退placement或峰值 | [首次Host验收](DSGA-首次Host持续核验与零派发取消-Mac验收-20261028.md) |
| 首次VM未发送取消与恢复 | 真实原命令不可重发取消后，以原Initial支撑永久实际预算；认证来源寿命、Cleanup丢回复/SDK重建、实际Host结清、后继完整Grow及下一次Shrink Prepare通过 | [本次首次取消闭环](DSGA-首次取消实际预算闭环-Mac验收-20261028.md) |
| 首次VM从未派发取消与恢复 | capacity v3独立原启动来源，VM仍Planned且无任何Runner命令；永久Initial/真实512MiB Host结清/Node v8、丢回复/SDK重建和后继完整Grow/下一Prepare组合通过 | [本次未派发闭环](DSGA-VM未派发取消闭环-Mac验收-20261029.md) |
| 旧操作来源回收与资格终结 | 原Host结清后逐payload核永久头/历史/Runner引用，有界回收同原域旧Outside/receipt；分页/丢回复/同名换UID及后继Prepare通过；活动资格保留，原VM完整关闭后才终结资格 | [本次有界来源回收](DSGA-原来源有界回收-Mac验收-20261029.md) |
| 原QEMU确定未启动的停止与Host结清 | 原持久launch后确定未Start/Start失败，以v2关闭；丢回复和整Pod/Node CAS恢复通过；不涵盖无launch或无Wait | [本次NoStart验收](DSGA-QEMU确定未启动关闭-Mac验收-20261029.md) |
| 资源发送未知错误 | 修复出生查询/序列化局部err遮蔽；三类setter准确ClientError及SDK只读恢复，18个受影响GuestReady场景通过 | 同上，错误修复准确提交f5a0e4df |

各模块收据的顶层/子场景计数只对其采用范围有效，不累加成完整产品E2E。
Go组合使用明确PG/root/资格/Kube/QMP夹具；本地真实PG与Rust模块另有证据。
夹具的物理动作不外推真实Linux证明。

## 本次实际通过与复用

- 最终Mac -race：首次服务→未派发取消→后继完整Grow→有界来源回收→下一
  Shrink Prepare组合117.08秒；受影响三条Shrink/Host/Scheduler子组合的父
  组合56.15秒。owner两顶层/五含父子记录，包耗时175.757秒，0fail/skip。
- 最终hostexec/hostscheduler/Controller领域9顶层/48含父子记录，三包通过，
  0fail/skip。本批总11顶层/53记录、4个不同包，不是53次独立产品E2E。
  五包vet通过，相对c2bb86a9的增量lint0；没有宣称全仓lint清零。
- 完整源码提交、14个Go文件SHA/7测试文件、逐用例及日志摘要、复现命令和
  初轮失败写入机器收据。Guest UUID夹具及旧Scheduler LIST计数失败保留，
  未放宽生产校验/Lease期限；中间通过结果不重复累计。每轮两页/16候选尝试，
  每候选可能PATCH+DELETE；进度只存小引用，不留32KiB正文。
- 20旧Outside/3旧receipt是明确批量领域夹具；Kube/Lease/PG/root/资格及QMP
  peer仍是夹具，不能外推真实Linux。PG17/Rust/GuestReady未改，不重跑；
  没有新Linux执行或8产物重编。VM v9、Node v8、Runner v2保持。

前一阶段未变的VM未派发取消/原启动来源证据：完整Mac -race101.20秒，合计
Node/owner 10顶层/20含父记录；budget/journal/admissionexec 13顶层/78记录；
均0fail/skip，七包vet/增量lint0。详见原VM未派发取消收据，不与本批重复累计。

上一阶段未变范围证据如下，计数不与本批相加成独立产品E2E：

- 新首次→取消→实际预算→Host结清→后继Grow→下一次Shrink Prepare组合：
  1项，Mac `-race`135.04秒通过；GuestReady受影响18子场景13.82秒通过。
  同一包最终2顶层/20条含父记录，0fail/skip；不是20条独立完整E2E。
- budget/budgetclaims/journal/admissionexec定向回归21顶层/107记录、旧Grow/Shrink
  两组合2顶层/4记录及永久来源模型26.24秒采用未变行为的准确既有结果，不重跑。
- 最终四包go vet通过；相对3f039e0a增量lint 0 issues。全量lint55项结果保留，
  未声称全仓lint清零，也不为历史规范问题重复整套业务测试。
- 最初离线缓存路径、旧lint工具版本错误及准确ClientError断言失败均保留；
  修复后最终通过。完整组合仅扩大测试总上限到3分钟，未修改生产请求/租约期限。
- 本批没有改PG/Rust内核，没有新Linux执行或8产物重编；此前准确产物/原生
  验证证据按原源码范围复用。源码SHA/用例映射/日志摘要已提交，不仅留在build。

## 仍须实现或补足组合验证的核心功能

| 顺序 | 工作项 | 当前不能签收的具体边界 |
|---|---|---|
| 1 | 来源与异常生命周期 | 原Root已持有约束的单调恢复已完成；缺原来源/资格/日志、实际高于原上界及swap/任务/系统异常、跨boot来源闭合仍须受控恢复。旧来源有界回收及关闭后资格终结不再重复开发 |
| 2 | 安全降低normal与活性 | 当前资源执行器明确拒绝降低memory.max；完整后台/私有内存持续上界及安全降限协议仍有代码工作 |
| 3 | 启动/接管/跨boot恢复 | 真正VM owner切换的完整组合；无Wait、Start/init失败；跨boot/迁移受控恢复，不能自动改绑原出生 |
| 4 | 持续控制与入口 | Monitor/自动策略、可信请求/取消/重放生产入口，全部写者与CPU隔离接线，仍须代码及组合 |
| 5 | PG18与PG16 | PG17核心Mac候选完整归档后分别移植与测试。PG18缺Neon发行接入且有AIO持有者差异，PG16需单独移植/集成；公共未变Go逻辑复用 |

这些是代码或Mac组合缺口，不是仅待Linux验证。独立控制面由另一任务完成后
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
