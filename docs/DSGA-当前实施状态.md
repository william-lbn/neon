# DSGA 当前实施状态

更新：2026-10-29（Asia/Shanghai）。V3为实施标准；**整体仍未完成，Goal active；继续PG17核心收尾后PG18→PG16**。当前逐项完成/候选/剩余代码及Linux边界统一见
[核心进度与验证清单](DSGA-核心进度与Linux验证清单-20261027.md)。下文保留各历史时点的验收记录，不能把旧段落的“尚缺”或“当前SHA”当成最新状态。
阶段性通过不能替代完整交付。默认关闭，不合
自有主分支，不构建镜像、不部署K8s、不发布。

## 最新：Agent私有协调入口已实际接线

AS公共f94d4e86、Agent准确12a993e7677817e9d936c79f072df98be9670afe、归档
b9f9ab7fbb4f79bc8ee5f607a51a5900a4d00a85已推原分支并核完整远程；Neon采用同一发行锁和
验收/收据。私有UDS默认关闭，已接真实Session/同一Run，完整Request/取消/分域
显式恢复、三方取消和原WAL保留。Mac四包去重16顶层/43含子记录0fail/skip，
vet/增量lint0；Agent双Linux架构准确revision ELF只编译、未执行，源码14摘要
一致，唯一AGENTS未提交导致modified=true。详见[阶段验收](DSGA-Agent本地管理入口-Mac验收-20261029.md)。
所有本阶段测试/质量/构建/AS推送会话均已终态，勿poll或重跑；所有者AGENTS
修改保留不stage，不apply旧stash。不是完整产品E2E；Monitor/自动策略、worker
建立前Root异常恢复、normal持续约束和其他核心仍继续，PG17完整后PG18→PG16。
Goal active，原heartbeat沿用；冻结/独立控制面/镜像/部署/main边界不变。

## 最新：QMP已生效丢回复后的真实失租接管

AS测试a6a2e761、归档909756fe已推原分支并核完整SHA，Neon准确发行锁采用同一
依赖。实际1GiB效果经真实失租接管只读回原Completed，未重发QMP或用旧Initial
冒充完整预算；原Prepare来源的实际预算/Host结清、后继PG Grow及下一Prepare
通过。最终Mac-race1顶层/1记录113.40秒、包116.312秒，0fail/skip；vet/增量
lint0。准确[验收/边界](DSGA-已发送VM失租接管-Mac验收-20261029.md)与收据已归档。
仅测试接线，无生产协议/PG/Rust修改，不重复旧未发送窗口；没有新Linux执行。
其他核心代码继续，不能据两个VM窗口关闭所有恢复，PG17完整后PG18→PG16。

## 前一阶段：真实失租接管后的原VM取消与后继业务链

AS测试代码2501160d、归档5c8a1bd1已推原分支并核完整SHA，Neon同一准确发行
锁和验收/收据对齐。旧holder真实续约失败、同Lease接管CAS丢回复后，保留原
WAL/初始预算；新Session只对账取消旧Key，再实际预算/Host结清、后继Grow及
下一Shrink Prepare通过。最终Mac-race4顶层/4记录0fail/skip，包179.587秒，
owner包vet/增量lint0。见[本阶段验收](DSGA-真实失租接管业务闭环-Mac验收-20261029.md)。
仅该接管窗口关闭，PG/Guest已派发/分区旧进程及其他核心恢复仍须代码/组合。
本批无生产协议/PG/Rust改变，不重跑未变矩阵或新增Linux执行。PG17整体核心
继续收尾，完整归档后PG18→PG16；长期Goal active，所有环境/冻结边界保持。

## 前一阶段：原QEMU确定未启动的停止屏障和Host结清

AS业务`0710d8cd`、归档`aebb209a`已推原分支并核完整SHA，Neon准确发行锁、
验收和收据同步采用。仅原持久launch后同一次调用明确未Start或Start失败且无
进程时产生v2停止来源，不伪造Wait；先本地fsync再不可变发布，Node结清仍须
整个原Pod容器终态。Mac三包-race12顶层/54含父子记录0fail/skip，三包vet/
增量lint0；双Linux测试ELF仅编译。见[阶段验收](DSGA-QEMU确定未启动关闭-Mac验收-20261029.md)。
PG17未变，不重跑原矩阵。无原launch的早期init、Runner被杀无Wait、来源异常、
安全降normal和持续预算、完整owner接管/跨boot迁移及Monitor自动入口仍须代码。
本阶段归档后继续PG17核心；其完整归档后PG18→PG16，整体未完成，Goal active。

## 前一阶段：原Root已持有约束的受限恢复与阶段归档

AS业务`81a4a2b3`、归档`59b66847`，Neon认证路由`6d71ad239`，准确发行锁已对齐。
原来源摘要/Root文件锁内floor/写前持久化/完整实际读回闭合；新owner不改老命令，
原GuestReady未知仍须独立Observe/落账。Mac新增及受影响Go -race去重22顶层/
56含父子记录、两包0fail/skip；Rust认证和UDS各1项通过；两包vet/增量lint0、
Clippy退出0。resourceguard双Linux架构仅编译、未运行。未改PG/Rust其余模块不
重跑。见[完整阶段验收](DSGA-原Root上界受限恢复-Mac验收-20261029.md)。

仅关闭可逐维恢复到原持有上界的Root漂移；缺源、异常生命周期、安全降低normal、
完整owner接管/无Wait/跨boot迁移及Monitor自动入口仍须代码。PG17核心未完整，
持续Goal active；原30分钟heartbeat沿用，归档后继续，不创建重复任务。

## 最新：首次取消实际预算与后继操作已通过并归档

准确AS业务`826e9c40`、资源错误修复`f5a0e4df`、中文验收/准确收据`65737287`。
首次VM原命令未发QMP即取消，实际512MiB、原目标1GiB；原Initial支撑完整实际
预算，Start/Record丢回复和SDK重建不改来源，Host按实际结清，随后正常Grow及
下一次Shrink Prepare组合135.04秒通过。GuestReady18场景13.82秒通过；最终
Mac -race2顶层/20含父记录0fail/skip，四包vet及增量lint0。旧领域/模型和未改
PG/Rust证据复用，不追加全矩阵或Linux执行。详见[本阶段验收](DSGA-首次取消实际预算闭环-Mac验收-20261028.md)。

该阶段不是完整核心封版。VM尚未派发且没有旧Native命令、来源/异常生命周期、
安全降normal、无Wait/接管/跨boot/迁移、持续控制/入口及其他写者/CPU仍有
明确代码工作，见上方核心清单。下文较早“尚缺”与SHA为历史时点，以清单为准。

## 本轮上游已冻结：优先核心整体收尾（2026-10-26 17:29:30）

按所有者最新指示，本轮只纳入截至上述北京时间已公开的完整PG17修复，此后
不再逐批fetch/merge新上游。社区固定15f57b68、Neon PG17固定56692dfb，Neon/
AS官方头分别固定fa504217/a4bce09e；准确值见[冻结清单与核心门槛](DSGA-冻结基线与封版清单-20261026.md)
及`ci/dsga-frozen-baseline-20261026.json`。后续只补核心功能、修自身缺陷及做
受影响Mac验证，不扩入独立控制面，不重复未变功能。2026-10-28新增PG18及PG16
后续版本范围，不改变当前PG17冻结点；PG14/15仍暂缓。

最后一项官方2PC检查点修复已完整合入并先推自有PG17原分支
`b078736a4cfdfccce69914feefb5b5e56560c1e8`，远程完整SHA核验。真实232/116字节
短写后，旧准确11d启动失败；新版本只留.tmp、恢复原事务并显式提交。新原生
3项、已有2PC崩溃恢复1项含2场景、官方prepared_xacts1项、旧反向1项、准确
Neon1.15/DSG组合加载1项通过，0最终失败/跳过；准确构建关键短写仅替换复核，
不累加，不重复Go/HTTP或core225。三处PG版本绑定同b078，准确4产物已重编。
见[维护、故障证据和边界](DSGA-冻结PG17两阶段检查点修复-Mac验收-20261026.md)。

**整体核心尚未完成，不能宣称已封完整在线伸缩版本。** 可信Prepare/系统剩余
上界、已派发取消/无退出回执恢复、其他写入者准入、迁移、安全降normal/首次
准入、完整生命周期与受控跨boot仍是核心代码缺口。后续集中关闭这些项；
控制面完成后另行移植/统一E2E，Linux内核/物理/性能后验，Goal继续active。

## 原VM未发送取消与Agent消费阶段完成（2026-10-27）

AS代码`1e0d432d8ac6cd39c07e10c94f9227ebaf707a0a`、中文验收/准确收据
`fe3967bc6c72dea8027395b50ae30ad5b4e3c742`已推送原分支并核远程，Neon
发行锁固定此依赖；PG17三处仍为b078，独立控制面d84c不变。

ResizeVM已在VM WAL派发、Runner尚未发送QMP时，在原flock内以两次原Kube
认证包围原QEMU before，持久封为cancelled_before_send并禁止迟到重放。
普通not_sent仍可重放，不能冒充取消；may_have_sent保留未知，真实目标完成
只落Completed。Agent核原Birth/State/不可变Receipt UID后原VM UID/RV只记录
这一条VM NoEffect，PG/Guest此前效果、旧预算及节点峰值全部保留。

首次新终态升**Runner record v2**，新普通动作不降级；VM v5/Node v5/Host
v2/PG ABI保持，外层Publication/Receipt对象类别v1。严格旧读者拒绝新原域
格式，独立控制面后续移植须同时支持VM v5与Runner v2，不混用旧服务。

Mac32顶层/153含子项最终通过0失败/skip，含受影响vmguard全包29/139、
实际扩缩取消/丢回复/丢回执/取消过晚及正常伸缩、显式重放和Host缺源；四包
vet/lint0。准确1e0d432d四服务双Linux目标8个ELF仅编译，未Linux执行。
组合发现并修复Host把其他域NoEffect当致命错误的分流，未授予跳过前驱的
许可。测试UID类型未编译及原分流失败完整保留，仅重跑失败、复用未变通过
结果，详见[原VM取消实施与边界](DSGA-原VM未发送取消-Mac验收-20261027.md)。

**完整取消仍须预算/准入AbortReconcile与容量/来源结清代码**；单条VM
NoEffect不能Finish整条操作。无Wait/Start/init恢复、跨boot/迁移、其他写者/
CPU、完整可信Prepare/首次/normal预算准入、持续约束与策略公开生命周期
继续。Linux内核/物理/K8s/性能后验；整体未封版，Goal active。

## 历史：未派发取消与预算关闭水位（2026-10-27）

AS代码`3b7d06c647acc2a2991070190221dbf3367309af`与
`68fd1addb28cde796fac6642310b03bbdb05b4c3`，中文验收/准确收据
`81aa3405ef7dbd9760bda3ed8e4960aed78268cd`已沿原分支推送并核远程。
该阶段已固定相应依赖；本页最新发行锁见上节。PG17三处仍为b078，控制面d84c不变。

实际生产worker在领域分流前只关闭完整全Planned、无Cleanup及SDK未决、
同原VM/PG的取消；原UID/RV CAS与派发/接管竞争，回复未知只读回。预算、
Host持有、高水位继续保留，不发原生Cancel或伪造NoEffect。

成功预算关闭新增永久水位，首次持久升**VM Model v5**；只允许零派发取消
推进，任何Start/Cleanup在写前撤销，新预算必须重闭合且不降级。Host结清
仍核原Runner/来源/Node。旧v4完整原成功与连续零派发历史可在显式Finish
升级；过期或夹杂派发不猜测。旧读者拒绝v5，后续独立控制面移植需升级。

本批去重Mac54顶层/148含子项最终通过0最终失败/skip，包含受格式影响的
journal全包43/108及实际Agent/Runner/Host在生产codec字节淘汰后取消并结清。
未派发两包、水位三包vet通过，两阶段lint0；准确68fd1add四服务双Linux
架构8个ELF只编译，未Linux执行。未变PG与其余矩阵复用。原编译/lint和
重复18轮组合25/60秒超时均保留；数量淘汰由Model/codec覆盖，实际字节
淘汰组合在原25秒期限内17.34秒通过，减少重复而未放宽生产期限。
详见[实施、测试映射和未完成清单](DSGA-未派发取消与预算关闭水位-Mac验收-20261027.md)。

已派发取消/失败及无Wait原Node/CRI恢复、受控跨boot/迁移、其他写者/CPU、
完整可信Prepare/预算及首次/normal准入、持续约束与Monitor/策略/公开生命周期
仍须代码；真实Linux内核/物理/K8s/性能等后验。整体仍未封版，Goal active。

## 历史：Scheduler有界轮询与历史/GC扫描收敛（2026-10-27）

AS代码`521537186a337cefaeb3ce298ee09ea3e6aca36c`仅改两个生产文件/两个
测试文件，完成原Node确认关闭历史过滤、有界分页/16项尝试、取消后保留队尾，
410只重置发现游标。首次引导仍完整核算；一分钟精确水位GC调度缓存不构成
执行权/容量TTL，旧持有与WAL不动。配套验收/准确收据已沿原分支提交推送。

本批Mac8顶层/16含子项最终通过0失败/skip，含512历史分三页、确定性取消
队尾、410恢复和实际Agent/Runner/Scheduler跨ticker不重复namespace LIST；
两包vet/lint0，准确52153718的Scheduler双Linux架构只编译、未Linux执行。
未变PG/其他服务及前批测试复用，不叠加重叠计数。见
[实现、真实用例与边界](DSGA-Scheduler有界轮询与GC收敛-Mac验收-20261027.md)。

有界工作量不承诺所有规模完成延迟；真实API分页/410/QPS、持续变动和高密度
节点进度须Linux验收。下一步集中取消/失败、无Wait原Node/CRI受控恢复，
随后跨boot/迁移/其他写者/CPU、可信Prepare/预算准入及策略公开生命周期。
核心仍未完成，默认关闭，Goal active。

## 历史：原QEMU退出、Pod结清和VM来源清理阶段（2026-10-27）

AS基础`e3cc8b66501a853a6c8d1d09b64a777b60045753`、实际接线
`08f3ebcb7e1b03930a840acb2d2aad45a677e731`及准确收据
`d57654e4531c2f8ae707334c110447e0a899fd9f`已沿原分支提交推送。
原Runner持有同一Start/Wait，先同步退出屏障再发布原Birth/容器绑定的退出CM；
退出不改原未知内存WAL，也不能继续消费旧QMP状态。Scheduler核全部原容器
终态及原Wait来源，按原Node UID/RV结清整Pod承诺；首结清v5拒绝旧节点写者，
先持久写原VM关闭锚点再移除Pod保护。Controller保留受管停止身份，删除时
核原锚点、全部Node无持有后，按原UID/RV清本协议来源；未知可续跑。

本批去重Mac20顶层/68含子项最终通过，0失败/skip；5包vet、6包Linux增量
lint0。准确08f3ebcb的Scheduler/Runner/Controller/Agent双Linux架构共8个ELF
仅交叉编译，均未Linux执行。真实Mac测试子进程不是QEMU，Kubelet/CRI/平台
开销是明确夹具；Controller清理为分段组合，不能称真实集群完整E2E。
见[实施顺序、源码/日志准确收据和剩余项](DSGA-原QEMU退出与Pod结清-Mac验收-20261027.md)。

**已实现的是关闭协议与分配账本，尚未认证整Pod的全部物理成本归还。**
Start/init失败、Runner被杀后没有Wait等未知路径仍持有，原Node/CRI受控恢复
生产者仍须代码；sandbox/cgroup/页缓存残留及平台开销须认证，不成立时须补
分阶段结清。取消、迁移/跨boot、其他写者/CPU、Scheduler有界公平/历史扫描、
可信Prepare/首次与normal准入、Monitor/策略公开闭环继续；整体未完成，Goal active。

## 历史：成功操作的Host峰值结清与旧来源收敛（2026-10-27）

AS账本`39839c1f93976b92aa9dc4d3d7685bec1b39f119`、实际链路
`efc385a77412379987ee14ba9054ffc22152a2de`及中文收据
`285fa2e626407ecb3f12de895d3a2a8f3776bbc7`已推自有原分支并核对远程。
实际Scheduler在原VM无Active且最新成功操作闭合后，核永久预算和原Runner
持久完成/无pending/实际值，再经Node UID/RV CAS结清旧峰值；目标、当前
来源与更晚在途承诺继续持有。节点首结清v4保留永久水位，旧执行读取拒绝，
只允许当前预算专用核验；确认后按原UID/RV清严格早于水位的同域旧Host CM。

新增/受影响Mac19顶层/68含子项最终通过0失败/skip，5包vet、最终lint0，
准确efc385a7的Scheduler/Runner/Agent六个双Linux架构ELF仅编译通过。
包括实际缩容到结清/等待Pod获得容量、未知/来源故障、实际Scheduler两阶段
结清与GC；原失败夹具单独替换，不重复不变功能、不叠加各批重叠计数。
详见[真实范围、准确收据和Linux边界](DSGA-Host峰值结清与来源收敛-Mac验收-20261027.md)。

**这里只结清运行中成功操作的峰值承诺，不是整份额度释放或物理证明。**
本段基线时VM/Pod停止销毁仍缺，正常原Wait/终态的协议结清现见上节；失败取消、迁移/跨boot及其他来源生命周期仍须代码；
全写入者隔离、CPU、可信完整Prepare、首次预算/安全normal/完整策略继续。
物理Host归还/平台资格及真实K8s/KVM/HugeTLB/性能留Linux后验；Goal active。

## 历史：实际Scheduler与首次Runner容量接入（76f8dc05，2026-10-27）

AS依次提交44bddd7f（v3首次容量/原Lease锁）、55cec6aa（实际Scheduler）和
`76f8dc05e6fb18da489ac285cc034e47b125c486`（最终Runner及组合）。实际入口已接
独立节点选主、固定原RV显式引导、Pod/原VM恢复、Reserve/旧批准与Publisher；
首次QEMU之前还核原Node/Pod/VM容量。不同副本可经原Node CAS单调加持有，
发布仍限节点owner，默认关闭；旧域不自动改义，未知/Unreserve不释放。
中文验收/准确源码收据由`ae4c937699568a1402433beadf97f3d8e3f4fe9e`随代码
推送自有原分支并核对远程，Neon发行依赖固定该SHA；PG17三处锁仍同b078736a。

新增/受影响Mac去重20顶层/98含子项通过0最终失败/skip，含实际Scheduler
循环到原Host发布及受影响Agent/Runner扩缩组合；6包vet、最终lint0问题。
准确76f8dc05的Scheduler/Runner/Agent双Linux架构共6个ELF仅交叉编译通过，
未在Linux执行；Kube/QEMU和资格/PG/Root仍有明确fixture边界。日志及24文件/
6产物摘要见[本批实施约束与验收](DSGA-Scheduler首次容量与实际接入-Mac验收-20261027.md)。

本段76f8dc05时仍须Host显式释放/历史收敛；成功操作部分现已由上节efc385a7完成。
VM退出/取消/迁移/跨boot结清、其他分配写入者准入与
独立CPU联动（当前受管VM关闭旧CPU批准旁路），以及可信完整Prepare、首次
预算/准入、安全normal、策略/Monitor/公开操作/取消/恢复。不是只待Linux。
Linux真实权限/内核/VM/物理/性能及控制面后续移植E2E按原边界完成；Goal active。

## 历史：Host来源发布与实际Agent消费（0d9fc2c9基线，2026-10-27）

AS代码`0d9fc2c9a16fcf1c7e052038e40c8eb642e8b850`、中文验收/收据
`a305c389618570ae527efcd8a9f7b1e8e55af758`已沿原分支推送并核对远程。
新Publisher从原VM WAL/Lease、Prepare/Outside、Runner实际观察及Pod核验，
先Node容量CAS，再原保护CM，最后Node CAS固定原CM UID；消费者在发布前拒绝。
实际生产Agent已接只读Consumer，原Host引用/摘要先写VM命令，复核后结算，
Start/Record未知只读原UID；Node/CM丢失或重建不自动补证/释放。正常未发布
窗口只等待；Host v2/Node发布v2须读者先升级，Model v4/Runner record v1不变。

Mac去重20顶层/142含子项最终通过0失败/跳过，7包vet、最终增量lint0问题，
准确Agent/Runner双Linux架构仅编译通过；真实旧29 Host读者拒绝v2。
包含实际Host生产/消费→Agent/Runner→预算/准入的扩缩组合，但Kube/QEMU及
Prepare/Host资格/PG/Root内核仍为显式fixture。原失败、中断和替换规则保留，
不重复未改PG/HTTP。见[模块交付与准确证据](DSGA-Host来源发布与Agent消费-Mac基本验收-20261027.md)。

**本段0d9fc2c9基线时Publisher尚未接实际Scheduler入口；现由上节76f8dc05接入。** 当时全授予/重启恢复、选主/引导、
其他工作负载完整计账、首次VM持有、显式释放/迁移互斥仍须代码；剩余可信
Prepare/首次预算/安全normal/公开生命周期继续，不能宣布整体封版或只待Linux。
控制面及PG/冻结基线未变；下节是本批之前的基础账本交付记录。

## 历史：Host节点持久账本原语（2026-10-27）

AS代码`6159109676a178c5294d0d78625651e812116714`、验收记录
`4e9656a3d6687a1fcfb98cfdc0003ad42be721c8`已推原分支并核对远程。完整原请求/
VM/Pod容量持有以Node UID/RV CAS保存，同Pod取峰值、旧Pod另计、切主保留；
丢回复只读恢复，丢源不自动初始化，达到有界条目上限不淘汰持有。

Mac新增7顶层/26含子项race通过0失败/跳过，readonly vet、最终增量lint0问题及
双Linux架构测试程序仅编译通过。该包尚未接Scheduler运行时，不是Scheduler
二进制/真实Host预留验收；Outside参数也未经本原语认证。真实全授予/重启恢复/
选主引导、Host源生产和显式释放仍须核心代码。见
[准确实现和边界](DSGA-Host节点持久账本-Mac基本验收-20261027.md)。
冻结基线/PG17/控制面不变，不重复此前未改功能，不把本批完成误报整体封版。

## 原VM显式重放传输完成（2026-10-27）

实际Session/Agent循环可在明确选择下复用原typed VM命令，将原not_sent三
对象UID/摘要与新nonce以同VM UID/RV保存；Runner自行拉取，锁内核本地原
回执UID，不因错误/普通轮询自动选择。重复选择只读确认，不改原命令/步骤/
Model序列；可能已发送拒绝，Host失联阻断，新not_sent需新明确选择。

Mac去重8顶层/33含子项最终通过0失败/跳过，原失败及替换规则保留；4包
vet、增量lint0问题及Agent/Runner双Linux架构仅编译通过。真实旧29 Runner
忽略提示、not_sent零写。Model仍v4、record v1、PG/控制面不变，见
[中文规范与准确证据](DSGA-原VM显式重放-Mac基本验收-20261027.md)。
通道默认nil，公开授权/策略/操作入口接线仍须生命周期实现；本模块只关闭
生产原VM重放传输缺口，不能声称整体核心或真实Linux物理已完成。

## Agent原VM派发与Runner来源长期结算完成（2026-10-27）

实际生产PGWorker已接typed ResizeVM/ObserveVM，原VM UID/RV先保存命令、
Runner自行拉取执行；Agent未知先查原Runner，不直发QMP。Shrink写前再核
当前PG/Root、原门禁/前台排空与Host双端持有；Observe复用原Resize事实。
最终预算和准入均核原Birth/State/Receipt UID及完整命令，来源丢失/替换不完成。
新来源当前预算与Finalize Completed同CAS原子升Model v4，淘汰历史/接管不丢失。

Mac去重53顶层/164含子项通过，0最终失败/跳过；8包vet、增量lint0问题及
Agent/Runner/Controller双Linux架构仅编译通过。真实旧c7读者接受v3并拒绝v4/
投影。PG17仍b078；控制面保留d84/SDK565，后续移植须先升级v4读者。
完整代码版本、原日志/包装器退出区别及夹具边界见[本模块](DSGA-Agent原VM派发与来源结算-Mac基本验收-20261027.md)。
随后实际Run复核发现并修复原Runner等待误退出owner：两个循环修复前失败、
修复后与6个读者边界/关闭通道共3顶层11含子项通过，增量lint0问题，两包vet
及更新后Agent双Linux架构仅编译通过；不重复累计原164项。原v4/PG/控制面不变。
可信Prepare、Host/Scheduler等剩余核心继续原分支，Linux认证另列，不追新上游。

## 已完成Runner消费/私有QMP与Controller状态接线（2026-10-26）

原Runner出生、固定Kube意图消费、每VM持久floor/完整pending、私有QMP唯一
set/实际读回及原UID回执已接实际Runner启动；受管Controller使用直接API来源，
移除TCP/manual入口，实际Pod核显式SA/sysfs/原Downward身份与状态卷。Mac去重
20顶层/110含子项通过，最终0失败/跳过，readonly vet和双Linux架构仅编译通过。
证据、质量补正及独立边界见[模块验收](DSGA-Runner持久执行与私有QMP-Mac基本验收-20261026.md)。
此为最终消费域模块；Agent typed派发/VM来源长期结算已由上节补齐，真实
Prepare/Host生产者等仍须完成。不能把新接线写成整个自动伸缩已经封版。

## 当前范围：内存伸缩核心先独立完成（2026-10-26所有者最新指示）

本任务仅推进Neon、PG17、autoscaling核心实现与Mac可执行验证；控制面在另一项
独立开发完成后再移植并重做统一端到端验收。本轮尚未提交的控制面main合并已
撤回，原DSGA分支/已发布`d84c0b7`保留且工作树干净；不继续追入main的新业务。
后续核心协议/依赖变化记录为移植清单，不自动修改控制面。分支和范围约定见
[维护规范](上游同步与功能分支规范.md)。内存伸缩自身缺失业务仍须完成，不能
归为控制面或Linux待验证；当前核心仍未完成，继续开发、验证并推送既有分支。

## 已完成模块：2026-10-26 当前预算长期持有（仍非核心整体完成）

AS `6d59154f69a9d669e8117ed3c1e59ca7ba62ede8` 已沿原分支提交、推送并核验完整远程SHA。
Model v3在同一次VM UID/RV CAS同时保存完整当前BudgetHeld与Finalize Completed；
原Guest/PG/命令/回执长期保留，取消、接管及历史淘汰不清除。新生产准入消费
同一完整持有预算，旧v2只在原Active全部来源重新核验后显式升级；未知准入先
查询，升级CAS未确认不新开放，不从历史或当前RSS补证。

Mac受影响journal完整及定向worker/合同/真实旧读者按全名去重38顶层/111含子项
通过，0失败/跳过；readonly vet与实际Agent双Linux架构仅交叉编译通过。PG17
仍11d、三处版本一致且源码/原4产物不变；不重复未变PG/core225/DSM/HTTP。
控制面保留d84/SDK565，未改业务/依赖，也未追入新main；后续移植须升级读者
再统一E2E，旧读者实测拒绝v3和其删字段投影。见
[本批实现、准确证据及边界](DSGA-当前预算长期持有-Mac基本验收-20261026.md)。

可信Prepare/剩余系统上界、Host/Scheduler及最终Controller/Runner/QMP生产者、
安全降normal与后台/私有约束、首次预算/开放、完整取消结清及受控跨boot仍须
核心代码，不能归到独立控制面或仅Linux待验；继续推进，Goal active/incomplete。

## 历史：2026-10-26 最终预算结算与持续持有来源（仍非完整预算生产闭环）

AS `565410ad37877d60ecbfb474be3d37c95af1c269`、控制面 `d84c0b73004a7d8cc3e89f3c3f0e3f06cfa8cba9` 已在原分支推送核验。
实际Agent接入Finalize和带原持有来源准入，完整结算/前驱摘要先CAS后复核；
Outside/Host真实UID、Pod/Node、原WAL/Root/PG/最终VM及删除保护均核验。
对象缺失/重建/释放/保护丢失不完成，不重发PG/资源/QMP、不代建或释放承诺。

Mac按完整测试名最终去重8顶层/49含子项通过0最终失败/跳过，两个夹具失败保留
并仅替换；readonly vet、实际Agent双Linux架构仅编译及准确旧读者验证通过。
控制面只对齐准确模块，Origin核验/最小2sum/vet；业务未改不重复测试。PG仍11d，
原生源码及4既有产物不变，不重复PG/core225/DSM/HTTP。
见[本批实现、实际证据与边界](DSGA-最终预算结算与原持有来源-Mac基本验收-20261026.md)。

可信Prepare/剩余系统上界、Host持有和最终Controller/Runner/QMP生产者仍须代码，
不把标签/immutable/资格ID当真实上界。首次预算/开放、安全降normal/私有后台、
公开完整生命周期及受控跨boot继续；Goal active/incomplete，Linux真实环境后验。

## 最新业务：2026-10-26 固定系统计费域与启动归组（仍非完整外部预算）

AS `e3641392ac2d3d2b4a5db76130c1de041f8c7c91`、控制面 `3ba51c5c66e8fdd264109ba16555b681d45bd0be` 已沿原分支推送核验。
原计划显式冻结系统额度，真实cgroup身份/WAL先于额度与PID1优先有界归组，
持久确认前不出生PG；运行worker/Quiesce保留原值，SystemCharged组件不重复计费。
迁移不迁旧charge，根组旧页/未计费项仍须其他可信来源，不能冒充完整SystemOutsidePG。

Mac AS去重42顶层/70含子项、控制面新增1项race通过0失败/跳过；readonly vet、
root helper/Agent双Linux架构仅交叉编译及准确旧读者拒绝新字段通过。PG17仍11d，
gitlink/两锁/实际检出及既有4产物hash核验，18/4/39来源校验通过，不重复未变PG测试。
见[本批代码、来源与Linux边界](DSGA-系统计费域与启动归组-Mac基本验收-20261026.md)。
完整Prepare/Finalize/残余系统上界、安全降normal/私有后台约束、首次预算/开放、
Host最终VM、公开worker完整生命周期和受控跨boot仍需代码，Goal active/incomplete。

## 最新维护：2026-10-26 PG17包装路径条件丢失修复（不代表业务完成）

PG17 `11dcee22d3e3b29e6c57af876159d62e32ca3178` 已完整合入社区3e914438并沿原分支推送核验；7导入文件
同官方，DSG开启core225通过。旧准确d4f在真实SQL中错返90行，新准确11d保留
Join Filter并正确返回0行，DSG布局/Neon1.15加载通过；新PG/配套库/扩展准确重编，
三处版本锁和18/4/39引用校验通过。AS849d/控制面53d无改动，不重复未变测试。
见[真实错行、修复与Mac证据](DSGA-同步PG17包装路径条件修复-Mac验收-20261026.md)。
本批没有新增预算业务。完整Prepare/Finalize/SystemOutsidePG、安全降normal、
首次预算/开放、Host最终VM/公开worker/受控跨boot仍需代码，Goal active/incomplete。

## 最新业务：2026-10-26 原创建有限任务数硬约束（仍非整体完成）

AS `849d84285e9099ae33c3d837a0563ff1f4974aa3`、控制面 `53d0c0d2f601fa5aeed20f74cf7394b8e05582ac` 已沿原分支推送并核验。
原Source/WAL/启动与运行命令冻结N，真实pids.max/current读回；迁入超限/漂移不
确认，下一次出生须计数确为0。实际Agent完整VM-CAS命令、Quiesce和来源摘要保留N，
不加成私有字节或VM许可。0/缺省保持旧未约束；严格读者先升级，平台不猜N默认。
Mac去重34顶层/57含子项及控制面新增1项race通过0skip，旧读者实测拒绝新字段；
root helper/Agent双架构仅交叉编译，实际内核待Linux。PG仍d4f三处同锁/扩展1.15，
不重复未变原生PG/HTTP测试，未构建镜像/部署。见[本批验收与边界](DSGA-有限任务数硬约束-Mac基本验收-20261026.md)。

本项只补数量边界。完整私有/后台/exec字节上界、可信Prepare/Finalize/SystemOutsidePG、
安全降normal、首次预算/开放、Host最终VM/公开worker/受控跨boot仍需代码。
Goal active/incomplete，只PG17，继续原分支，不合自有main，不包装成仅待Linux。

## 最新维护：2026-10-26 PG17与控制面完整跟进（不代表业务完成）

PG17 `d4f92ee0170c3566c12fbf2f563f0021c1e3d413` 与控制面 `4b151e4afc640c08c1b8a29227b631644212897a` 已在原服务分支远程核验，
分别完整包含社区08f0cec8与控制面main2283b1fa。PG5文件同官方，DSG开启core225与
PTY补全4断言通过，准确SHA/配套Neon1.15重编及关键替换复核通过；三处PG锁一致。
控制面34顶层/118含子项race、readonly vet、客户端9不同项与Web tsc通过0测试skip。
见[本批维护、原证据与边界](DSGA-同步PG17与控制面维护-Mac验收-20261026.md)。
未重跑未变业务；Goal仍active/incomplete，完整预算和最终VM等代码继续原分支演进。

## 历史：2026-10-26 四数组跨内部恢复持续保留地址占位（仍非整体完成）

PG17 ea07fdd1a0b5851de4b985d3b0d47c3b2f468bfe已先推自有原分支并核验。
Postmaster私有四slot表持续持有VA/span，旧共享backing在全部共享子进程退出
后换成不可访问私有占位；新boot在同址建立全新零共享backing。转换未知FATAL，
不先解除VA再选址；非共享子进程清自身表，DSBCtrl为空也不漏占位，不写共享目录。
Buffer6/Work2/Connection1/Prepared1/DSM1、Region结构、WAL/配置和扩展1.15不变。

Mac受影响原生5不同场景通过0skip，含连续3次真实内部恢复、同址保护/新零
backing与坏几何FATAL、真实数据；启用DSGA的官方core225通过。初组测试SQL
引号和单进程vmmap标签假设失败均保留，仅替换该项，产品不放宽。单映射
MAP_SHARED实测SM=PRV，不能据此推断私有成本；父/backend双映射共享仍严格
验证。准确SHA与配套库/Neon1.15重编通过，关键2项替换通过，不叠加计数。
Neon三处PG同锁；ASda2/CP21c保持，不造空提交，不重跑未变Go/HTTP/发行单测5。
见[本批代码、原始证据与边界](DSGA-跨内部恢复地址占位-Mac基本验收-20261026.md)。

本项只是可执行预算的地址空间寿命前提，不是私有硬额度或VM许可。完整可信
Prepare/Finalize/SystemOutsidePG、安全降normal/后台私有上界、exec/PID边界、
首次预算/开放、Host最终VM/公开worker/受控跨boot仍须代码。Linux内核/映射/
HugeTLB/物理/目标E2E/性能另验。Goal active/incomplete、PG17、默认关闭，不
构建镜像、不准备/执行部署、不合main；相同VA不代表相同boot或旧操作已结清。

## 历史：2026-10-26 首次PG身份先持久固定（仍非整体完成）

AS da2aa56a1efaf6241ebf85c7814542e41e757a66已先推并核验，沿原服务分支。
首次能力只读发现后，以原VM UID/RV CAS固定PGInstance才返回实际worker，
不等首个Resize/HWM；写后丢回复保留原身份。参数/显式boot均先校验，空Active/
接管/历史淘汰不改绑；旧v1沿原HWM恢复不补字段，新请求不能改HWM收养新boot。
通用Cleanup不是跨boot关闭证明，原未知Busy/旧历史可读及同boot推进保留。

首次绑定原子升账本Model v2，必带PGInstance，外层信封仍v1；与最终Resume
审计v2不同。旧读者实测原v1可读，新字段和v2投影均拒绝，所有读者先升级再
启用，不删字段/重造日志回退。Mac去重36顶层/71含子项race通过0测试skip，
原完整journal53过1旧合同失败后只替换该项；实际旧读者三条检查、最终vet和
Agent双Linux架构仅交叉编译通过。控制面准确自有模块Origin同da2、最小2sum/
readonly vet，以及新增专用TLS签发器读v2的1项race通过，不改账本/凭据。
Neon AS同锁，PGb812三处/扩展1.15不变，不重测PG/DSM/nativeHTTP/发行单测5。
见[本批实现与证据](DSGA-首次PG身份持久绑定-Mac基本验收-20261026.md)。

这只是首次身份基础，首次预算和开放仍须代码。完整可信Prepare/Finalize/
SystemOutsidePG、安全降normal/后台私有上界、首次预算/开放、Host最终VM/
公开worker/受控跨boot仍未完成，不包装成仅待Linux。Goal active/incomplete，
只PG17、默认关闭，不构建镜像、不准备/执行部署、不合main；目标内核/物理另验。

## 历史：2026-10-26 最终开放准入的原资源持续复核（仍非整体完成）

AS d6979c068866b310e0cfc1dd04a075081b1684a9已先推自有原分支并核验远程。
实际Agent/PGWorker的最终ResumeAdmission现在复用同一认证root，开放/显式
重放及写后完成前核原资源来源、floor、限额、Guest、真实Postmaster出生和
Outside一致性，重读VM/RV；同额度新Held也不收养。新成功审计v2保留来源，
历史v1不补写。未知原准入先查，资源/VM漂移仍保留WAL/预算，不重发或提前完成。

Mac受影响去重1顶层/18含子项（17不同场景）race通过0skip，readonly vet与
实际Agent Linux双架构仅交叉编译通过。旧准确源码overlay负向复现同额换源
仍发送1次；原日志保留。Kube/root/PG为协议夹具，不冒充目标内核或集群E2E。
控制面完整mainf7df93a经独立f4d3fc8维护先合入并推送，Catalog定向1PASS；
随后准确自有模块Origin同d697、最小2sum及readonly vet通过。Neon AS来源
同锁，PG三处b812及扩展1.15不变，不重复未变PG/DSM/nativeHTTP或发行单测5。
见[本批代码、验证与未完成项](DSGA-最终准入原资源复核-Mac基本验收-20261026.md)。

本项是最终恢复准入，首次开放仍须单独实现。完整可信Prepare/Finalize/
SystemOutsidePG、安全降normal与后台/私有可执行上界、首次准入、Host最终VM/
公开worker/受控跨boot仍是代码缺口。Goal active/incomplete；只PG17、原分支、
默认关闭，不构建镜像、不准备/执行部署、不合main。Linux内核/物理/性能另验。

## 历史：2026-10-26 非共享子进程的真实DSG映射修复（仍非整体完成）

旧准确PG5462实测logger仍持四个DSG共享映射；新增局部副本detach修复
b812536cd983595ba233a7fad418d9bae448b2f3已先推既有PG17分支并核验远程。
Neon三处版本绑定同SHA；Buffer6/Work2/Connection1/Prepared1/DSM1和扩展1.15
不变，避免子进程Destroy误清共享目录。Mac三个新增真实映射/数据生命周期
场景通过0skip，准确SHA重建后关键两个场景替换通过，不叠加计为五项；Linux
procfs分支、物理回收和内核效果尚未执行。见[代码与实际证据](DSGA-非共享子进程映射-Mac基本验收-20261026.md)。

本批控制面完整main1b0e8121经独立维护19a1e56b合入原分支并推送核验，41文件
全部纳入。新增/受影响22顶层/36含子项race、vet、Webtsc与新增本地模板2项
通过；受保护Linux账单E2E未执行。AS2c9不变，不制造空提交。

完整可信Prepare/Finalize/SystemOutsidePG、安全降normal与后台/私有可执行
上界、首次准入、Host最终VM/公开worker/受控跨boot仍须代码，不能写成仅待
Linux验证。Goal active/incomplete；只PG17、原分支、小批验证、默认关闭，不
构建镜像、不准备/执行部署、不合main。

## 历史：2026-10-26 持续聚合Quiesce与实际Shrink前复核（仍非整体完成）

AS合同568e5645/执行9740ec96及后续来源绑定修复沿原分支演进，精确SHA见收据。
实际Agent/PGWorker消费原Prepared，
原root出生/聚合及PG fence后，完整关闭命令CAS写前才发；同点DSM嵌套事实
核原关闭/连接ack与前台排空。2PC/后台/DSM/私有不清零，保留原约束。关闭
Pending恢复、丢回复原查询、显式分域重放；关闭写前必须持久完整原来源，重建
不能采用同数值的新Held。旧仅关闭格式只读保留，不补证/结案/重放；新Shrink
前核完整原命令与回执来源一致，再核当前门禁。
GuestReady也禁止无证明降低normal。逻辑PG完成仍不跳过物理ReleasePG/VM。

Mac新增/受影响Go race最终5顶层/33含子项PASS，0测试skip，1无test包skip
如实记录；readonly及最终tag vet通过，实际Agent双Linux架构仅编译通过。
新真实Go race/TLS/生产Router/JWT/SQL/PG5462+Neon1.15关闭与新消费者1项通过，
2PC槽1/DSM2MiB真实保留；root/外部为模型，WAL/CAS另由owner实际调用验证。
首轮err遮蔽产品修复、容量/漏fence fixture失败保留，仅替换失败及新增场景；
目标clippy通过、依赖27原告警保留，不重复Native8/core225/iso5。见
[本批代码、命令和实际证据](DSGA-持续聚合Quiesce-Mac基本验收-20261026.md)。

持续策略只保留/提高normal，安全降normal及后台私有可执行上界、完整可信
Prepare/Finalize/systemoutside、首次准入、Host最终VM/公开worker/受控跨boot
仍缺代码，不能以有限模式替代V3最终要求。Linux内核/物理/性能后验，Goal
active/incomplete、默认关闭、沿既有分支，不构建镜像/部署或合main。

## 历史：2026-10-25 Root聚合来源接入GuestReady审计v2（仍非整体完成）

AS a3d91b8a已先推送核验，原root认证View的已确认Bootstrap/Held、PG出生及
populated域生产不可变PG计费域+整个Guest HugeTLB池来源；实际Agent/PGWorker
GuestReady写新审计v2，旧v1不回写。Outside声明明确排除，摘要不认证来源，
后续动作须Match最新认证View，Unknown保留原承诺，不给VM许可。

Mac初组Go race5顶层/14含子项通过，新增root-empty恢复仅单测通过，去重5/15，
无测试跳过；无test文件resourceexec的1包级skip如实记录，执行器由owner覆盖。
readonly vet通过，实际Agent Linux双架构仅交叉编译通过；不冒充真实Linux资源。
控制面自有准确模块Origin同SHA、最小2sum及readonly vet通过，Neon AS来源同锁；
PG三处仍5462、扩展1.15不变，不重复DSM8/core225/iso5/未改HTTP。
见[本批实际代码和证据](DSGA-Root聚合来源-Mac基本验收-20261025.md)。

完整可信Prepare/Finalize与系统外部来源、后台/私有持续上界及降限活性、首次
准入、Host最终VM/公开worker/受控跨boot仍是缺代码；Linux目标内核/物理/性能
后验。Goal active/incomplete，沿既有分支，小批推进、默认关闭、不合main。

## 历史：2026-10-25 DSM正式SQL/HTTP与严格消费者（仍非整体完成）

NativePG5462不变；Neon1.15新增正式DSM只读成员和认证固定GET，原生同点嵌套
Prepared/Connection/Work，复用序列化helper不补读。C双SU/原session拒绝误授权，
Rust/Go严格全字段、实际页/成本/cap/范围/实例，默认0明确未约束；无VM许可，
不清Pending或跨boot旧预算。ASd8c1f2ea已先推核验，控制面准确自有伪版本Origin
同SHA、最小2sum和readonly vet通过，固定GET及独立观察已接。

Mac Rust协议1、Go协议/SDK race2、CP代理race去重2、准确PG+Neon1.15正式HTTP1
及真实Go race/TLS/Router/JWT/SQL1均通过；目标clippy通过、依赖27原告警保留。
初轮fixture JSON宏、路径计数15/16、权限在关门先55000均如实保留并只替换失败
检查；产品准入/权限未放宽。原Native8/core225/iso5未重复。见
[正式链与验收](DSGA-DSM正式管理链-Mac基本验收-20261025.md)。

控制面完整主线af5e70432文件经eb6829b独立维护推送，实际19顶层/26含子项
PG/race及vet通过0skip，不把附带Linux收据计本轮。下一项仍为完整后台/私有约束、
可信预算生产者/systemoutside/首次准入及Host最终VM/公开worker/受控跨boot代码。
Linux目标内核/物理/性能另验，Goal active，默认关闭，沿既有分支不合main。

## 历史：2026-10-25 外部DSM分配前预算原生模块（仍非整体完成）

PG17 `5462f1de54a97b9a9235c69d64d31cb0825dcea5` 已在原分支先推送核验，包含
独立官方锁维护bfd84a98。Native DSM1在真实OS创建前预留，pin/多mapping不重计，
实际DESTROY成功前/未知失败仍保留账；main-region去重、控制段独立、元数据计入
固定主段。postmaster-only可选payload额度，默认0明确未限制，不停必要后台工作。
旧Buffer6/Work2/Connection1/Prepared1及DSM控制格式保留，同点观察不反取DSM锁。

Mac新增8不同场景0skip：首轮5过/2 SQLSTATE断言误期望53200、真实53000，单项
替换及新增默认兼容1后通过；未重跑原5。新入口官方DSGA core225及相关隔离5
通过，收口关键2和准确SHA关键3属替换，不加计数。语法/静态分析通过；正式Neon
DSM消费者仍待编码，不把Native API/关门/idle当完整证书。详见
[原生实现与证据](DSGA-DSM分配前预算-Mac基本验收-20261025.md)。

本批控制面完整main577bb3b经b9f9be7维护合入原分支，新增/受影响真实PG race19、
vet及本地内存渲染3通过；缺yaml/旧Helm默认版本的失败保留，负向错误原因已固定。
两维护分支已推核验；没有构建镜像、准备或执行部署。其他PG版本暂缓。

正式DSM管理消费者、完整后台/私有预算与可信Prepare/Finalize/systemoutside/
首次准入、Host最终VM/公开worker/受控跨boot仍须代码。Linux内核/物理/性能另验。
Goal active/incomplete，默认关闭，所有服务继续沿原既有分支，不合main。

## 历史：2026-10-25 两阶段保留槽及正式管理消费者（仍非整体完成）

PG26e80ac6a3ad02fd149d272302d5bf9b4cbaa7ea、AS5b49d343c6223ffc18b40a150c0ea0a2c7f81c6b
已先推送核验。新独立Prepared1保留Work2/Connection1/Buffer6 ABI，原生计数包含
未valid准备、backend退出后的PREPARED及真实恢复槽，与关门和连接/Work同点。
新运行期PREPARE TRANSACTION包括SU/后台均受门禁，WAL/磁盘恢复保留旧意图；
不暂停检查点/回收，不替客户决定提交或回滚。Neon1.14正式SQL、认证固定HTTP、
Rust/Go及控制面消费者严格接线，读取不清Pending或给VM许可。

Mac新增6场景0skip：首轮5过，余1仅C ERROR默认SQLSTATE断言错，修正后单项
替换通过；真实持TwoPhaseStateLock backend SIGKILL、WAL/磁盘恢复均验证。
官方两阶段回归1项（实际303行输出）/专门隔离2项通过，未重复未变225/123。
准确SHA重编关键2项为替换；正式NeonHTTP1及真实Go race/TLS/Router/JWT/SQL1
通过，均读到前台idle与prepared槽1共存，原Pending保留、零写POST。
Rust协议1、Go协议/SDK race2、代理race2及目标clippy/vet通过，依赖原警告保留。
见[本批实际证据](DSGA-两阶段保留槽-Mac基本验收-20261025.md)。

本轮还完整合入控制面main76a7699客户端维护，cf8e387独立提交已推核验；仅
新增CLI/MCP本机HTTP/stdio15项0skip，不重复未改Go/数据库业务。后台/并行/DSM、
预备语句缓存/临时/会话私有内存、可信预算/首次准入、Host最终VM/公开worker/
受控恢复仍须代码；不能把两阶段计数或前台idle当全排空。Linux目标验收后置，
默认关闭、Goal active/incomplete。

## 历史：2026-10-25 关门期间已有事务的有界结束（仍非整体完成）

PG17 dfd989817b0afbce19caf4c8bec73857b3d7a5a8 已在原分支推送核验，准确
SHA和实际Neon1.13重编。已有显式/失败事务可在准入持续关闭时通过单条有界
COMMIT/ROLLBACK及官方别名结束，覆盖简单和扩展P/B/D/E；新named对象、多语句、
CHAIN/PREPARE/TO及不完整帧不得逃逸。真实READ/EXEC/retained及原预算持续到
提交回调、等待、取消和资源清理完成，不能把“接受结束”当排空或VM许可。

Mac新增原生9项0skip、受影响Work/帧6项、官方启用DSGA core225/isolation123
通过；16MB半帧实际backend分配量1182608前后相同。准确SHA关键2项为替换，
不重复计数；正式HTTP准入→公共应用事务→真实延迟触发器锁等待→正式同点
观察新1项通过，完成后仍关门、显式重开后同一连接可查询。目标clippy通过，
未改依赖原有警告和首轮测试桥缺字段/静态检查失败保留，未包装成全绿。
AS4460、控制面1a5的协议未变，不造空提交。见
[本批实现与验收](DSGA-关门事务结束-Mac基本验收-20261025.md)。

尚需实际后台/并行/DSM/预备事务与命名缓存/临时状态等保留内存的完整Quiesce
或持续上界、可信预算/首次准入、Host/最终VM/公开worker/受控恢复代码；不是
Linux待验项。Linux/K8s/HugeTLB物理/性能另验，默认关闭、Goal active/incomplete。

## 历史：2026-10-25 既有分支维护及原VM2数据库约定修复

四个既有分支继续演进，PG17 d42638e、AS 4460da5 已锁定，控制面
1a5ea539ada53e282728e82cc187fafd57c1db1d 已推送核验。新fetch的Neon/AS及
双PG官方线missing0；控制面main b1fbd5c全部维护经6898ec4独立纳入，未重建、
rebase或强推分支。新增策略/计划及OpenAPI/迁移注册9顶层/17含子项race通过，
Webtsc通过；未重复既有业务全套。

附带真实入口检查发现此前GoVM2与reservation SQL只认1的组合缺陷，原两项
失败保留。独立新增900000066前向迁移对齐真实VM2及显式资源对象，完整保留
旧不可变/锁序/凭据/原资源图约束；不UPDATE旧行或改已发布迁移。实际Go仍
核验完整额度/总量/来源，数据库格式接受不授予Guest预算或VM动作。

Mac准确PG17/Go race共13顶层/27含子项受影响创建/权限/配额/凭据/并发/迁移
通过0skip；其中旧回退夹具遗漏撤销依赖标记，修正后只替换该1项，其余未重跑。
旧v1整行/xmin保留、新v2接受、部分/未知版本直接SQL拒绝及readonly vet通过。
见自有控制面原分支的
[中文验证记录](https://github.com/william-lbn/control-plane/blob/codex/dsga-control-plane-v3-20261020/docs/DSGA-原VM资源计划v2守卫-Mac验证-20261025.md)。
这是实际缺陷修复，不是完整伸缩验收。下项仍是关门事务受控结束及完整Quiesce/
预算/最终VM等代码，Linux目标环境边界保留，Goal active/incomplete。

## 历史：2026-10-25 持续连接门禁和同点Postmaster确认（仍非整体完成）

沿用四个既有服务功能分支，PG17 d42638e、AS 4460da5 已先推送核验。
新增原生Connection API1及Neon1.13正式SQL/认证HTTP/Rust/Go观察，受管实际启动
显式要求门禁。公共Unix/TCP在fork前关闭，固定0700私有管理端点仍可用；不用
startup用户名猜管理身份。Postmaster主循环独立ack，不等待backend锁；ack与
Work2同点采样，启动/崩溃新boot关闭，序号饱和不能复用旧开放许可。

Mac新真实原生6项全部通过、受影响Work8项、core225/isolation123、Go协议/SDK
race2项、Rust协议1项、实际启动设置6项及控制面路由race2项通过。准确PG重编
关键2项替换通过；严格NeonHTTP1项和实际Go race/TLS/Router/JWT/SQL/PG1项
通过，均不是结构mock。先前夹具走旧公共管理socket被正确拒绝的失败保留，
只修正夹具私有路径后通过。相同功能不重复加计数。相关clippy/vet及版本绑定检查
按机器收据记录。见[实现与Mac事实](DSGA-持续连接门禁-Mac基本验收-20261025.md)。

这只是持续新连接屏障。完整后台/并行/DSM/预备事务及会话保留内存排空、关门时
已有事务受控结束、可信Prepare/Finalize/system outside/首次准入、Host/最终VM/
公开worker和受控恢复仍需编码；不包装成Linux待测。Linux/K8s/HugeTLB物理和
性能另验。默认关闭、所有VM许可false，整体Goal继续。

## 历史：2026-10-25 Postmaster启动包前窗口（仍非整体完成）

PG `9db3b91d3caff9f674786252f0104de75ed8e709` 已在既有功能分支推送核验。
Work2在Postmaster fork前登记，覆盖尚无startup包、SSL/GSS、DNS等PGPROC前
窗口；交接至PGPROC与原计数同status_lock，退出/fork失败由原父进程回收路径
原子清理。Postmaster不取得backend锁，早期child保留官方直接_exit行为。
受管容量/恢复拒绝在fork前关闭，避免未登记无界dead-end；非DSGA保持原路径。

Mac新增6项真实窗口全部通过，旧准确108版RAW漏计负向预期失败。实际将持
status_lock的测试backend停止并杀死，Postmaster仍能回收、重建关闭新boot。
准确SHA重编仅替换验证RAW及持锁崩溃2项；相同项不加计数。官方启用DSGA
core225/isolation123在登记/交接候选通过，最终回收接线由新6项覆盖，不重复
未变全套。首轮漏回收接线的7个失败与修正明载记录，未用普通套通过掩盖。

Neon正式SQL/HTTP及Rust/Go严格消费Work2，拒绝旧Work1；Buffer ABI6和Work
结构字段不改，扩展仍1.12。新实际Go race/TLS/Router/JWT/SQL/准确PG门禁保持
一条已GSS协商但未startup的连接，读到恰好一个startup；Pending保留、零写入。
正式HTTP受影响1项、Rust协议1项、Go协议/SDK race2项及相关clippy/vet通过。
AS `e539f2f336b5d63e58556a38447c32e5dc8ea0a9` 已先推送，消费者版本共同锁定。
控制面完整维护合并5ba50b8纳入main4d86357，Mac24顶层/40含子项race及Web
tsc通过，原AS依赖更新独立；没有运行随主线附带Linux部署/浏览器收据。

这仍是实际观察，不是禁止关门后新fork的确认或完整Quiesce：持续连接门禁、
后台/并行/DSM/预备事务及预算/Host/最终VM/公开worker/受控恢复仍须编码。
Linux原子/调度/cgroup2/HugeTLB/物理/性能后验。Goal active，仅PG17，沿原
服务分支演进，不构建镜像、不准备/执行部署、不合主分支。详见
[本批证据与边界](DSGA-Postmaster启动前登记-Mac基本验收-20261025.md)。

## 历史：2026-10-25 真实前台工作登记与正式管理链路（仍非整体完成）

实际Neon PG17 `1085064d44a47655149c8b7ab94c19d0cfa3a9c7` 已在原自有V3
分支推送并核对；包含独立社区维护合并36d58060（strict jsonpath根字符串GIN）。
固定PGPROC槽计费入启动主段，读取登记与关门同锁；跟踪STARTUP/read/execute/
有界拒绝/idle及独立保留事务，ERROR清理与ProcKill资源清理完成后才清状态。
COPY/WAL流保留执行；Describe补准入。背景/辅助只是实际生存期，不是假排空。

正式Neon扩展1.12/Native Work API1、Rust严格DTO/专用SQL和认证HTTP、Go只读
SDK及控制面固定GET已接通。HTTP暴露真实问题：扩展协议的Parse/Bind隐式事务
会保守阻止观察者排除；固定无参数观察改完整Simple Query，Native规则不放宽。
其他会话事务和未Sync扩展事务仍阻止idle；观察不清Pending、不写任何VM许可。

Mac原生去重9项通过（8新登记场景+1既有COPY兼容），启用DSGA官方core225/
isolation123通过；准确SHA PG与Neon C/Rust communicator重编。新增正式HTTP
1项、内层Go race→TLS→生产Router/JWT/SQL/准确PG门禁1项通过，Rust严格协议1项、
Go协议/SDK race2项、控制面路由race2项通过，相关vet/clippy通过。
AS `d1f5a9fccf335d565882edac3afbb7e9b21013c1` 已在原分支推送，Neon版本锁与
控制面准确模块共同绑定；控制面独立主线维护合并cf775bb保留。详细批次、首次
失败、最终替换证据与未验项目见[本批记录](DSGA-真实前台在途登记-Mac基本验收-20261025.md)。

PGPROC前连接/SSL/GSS窗口、后台/DSM/预备事务全排空及持续预算仍须编码；完整
可信预算生产者/system outside、首次准入、Host/Scheduler/最终Controller-Runner-
QMP、公开worker/生命周期及受控跨boot/owner关闭也仍是代码，不是仅待Linux。
Linux K8s/HugeTLB/物理/性能后验。Goal active，仅PG17，默认关闭；沿既有服务
分支演进，不构建镜像、不准备/执行部署、不合主分支。

## 历史：2026-10-25 关闭准入的大帧分配边界（仍非整体完成）

本批在Neon实际 `vendor/postgres-v17` 修复协议入口，不是独立普通PG项目：
原门禁在完整消息体读入后拒绝，关闭期间仍可能先分配大Query/Parse/Bind/
FastPath缓冲。现在在分配前冻结拒绝决定，以固定接收缓冲消费被拒帧，完整后
回原55000；主循环CopyData和ignore-till-Sync则有界静默忽略，非法/半帧仍断连。
本帧丢弃中重开也不执行被丢弃正文。Buffer ABI6、WAL和Rust存储层未改。

PG `b48b089f9004a6dd437b075f146fab5fa67711bf` 已在原自有功能分支先推送/核验。
候选Mac新原生5项、受影响准入5项、core225/isolation123通过；旧准确cbabd5
单Query负向按预期失败，真实backend已分配由1152912增至34707408字节。准确
SHA重编仅复核改动的2项通过；COPY真实失败/残留与在途写入已补验。Neon
子模块、两份PG锁和实际源码同步b48b089；AS08161712/控制面48d966b8未改，
没有制造空提交或改变已有服务分支。

这只是关闭/忽略消息的读取预算：**不是完整Quiesce**，仍须追踪已经读入/在途
命令、认证/连接创建、当前COPY、后台/并行/DSM等，不能以门禁关闭冒充排空。
完整预算生产/系统outside、首次准入、Host/最终VM和公开worker/受控恢复仍是
代码，Linux K8s/HugeTLB/物理/性能另验；Goal active、默认关闭，不构建镜像、
不准备/执行部署、不合main。详见
[真实协议与内存证据](DSGA-准入大帧读取上界-Mac基本验收-20261025.md)。

## 历史：2026-10-25 原生终态只读恢复（仍非整体完成）

AS `081617124ac265226c92e602d293b416e45118cd` 已在既有分支先推送并核验。
修复已落账原生终态在 worker 重建后仍恢复 Pending、从而挡住后续准入的问题：
完整原请求/审计/当前 fence/同一逻辑终态及本地栅栏逐项核对，只查询、不重发
resize、不从缺历史造 NoEffect、不改原 WAL；实际 Agent 循环接入。普通 Status
仍不清 Pending，Busy/Rejected 无原生终态者及跨 boot 仍需受控恢复。

Mac 去重 39 顶层/129 含子项 race 通过；更强严格跨仓门禁 1 项通过，内层 Go
实际经 TLS/Neon Router/JWT/管理 SQL/准确 PG17 验证重建及更高 owner 下历史
原终态，核对未增加 Resize POST。相关 vet、Agent 双架构目标编译及新 Rust
门禁 clippy 通过，目标编译不等于 Linux 运行。控制面完整合入 main028
`0aa0230c78a0f31a091e32fc49c8c4b0919a6ac7`，独立维护提交 `7c02b199`；
受影响 Go4/Node8 与 Auth/Web 类型检查通过。Neon/控制面准确版本锁共同对齐
已发布 AS，PG17 三处仍 cbabd5，本批未改 PG 或其他 PG 版本。

完整预算生产/系统 outside、首次准入/完整排空、Host/最终 Controller-Runner-
QMP、公开 worker/生命周期及受控跨 boot/owner 关闭仍是代码，不是平台待验。
真实 Linux K8s/HugeTLB/物理/性能后验。Goal active，默认关闭，不构建镜像、
不准备/执行部署、不合 main。见
[本批实现与证据](DSGA-原生终态恢复版本对齐-Mac基本验收-20261025.md)。

## 历史：2026-10-24 GuestReady实际资源执行（仍非整体完成）

AS `d3bd69d246cb40f6116fd1b7c874a9a319004a81` 已在既有功能分支先推送并核验，
含独立预算合同提交 `cb1fe5b8`。实际 Agent/PGWorker 现在接入 GuestReady 执行器：
消费原 Completed Prepared，核对 Grow/Shrink 顺序、完整 PG 成本/原释放回执、
原 Guest/Birth、全部 hstate 和实际 Runner/VM；资源 revision 取 root 原 WAL。
完整命令先 UID/RV CAS 后派发，普通轮询仅观察；独立显式原重放保留未知，写后
重读 PG/root/VM 再落审计。不是独立于实际服务的演示代码，也不将资源上限当 VM 完成。

Mac 新模块去重 4 顶层/21 含子项通过，race 无未修复失败/跳过，六包及实际 Agent
vet 通过，实际 Agent/root guard 双架构 4 目标编译通过；只编译，不是内核验收。
控制面与 Neon 锁须共同引用已发布 AS 准确 SHA，PG17 三处仍为 cbabd5，无 PG
源码修改。完整证据和最终远程状态见本批记录与恢复点。

**尚未全部开发完成**：Prepare/Finalize 可信生产者与系统外部上界、首次独立准入/
完整排空、Host/Scheduler/最终 Controller-Runner-QMP、公开 worker/生命周期和
受控跨 boot/owner 未知关闭仍需编码。Prepared 合同不替代真实生产者，缺少它时
执行器拒绝推进。真实 Linux K8s/HugeTLB/物理/性能后验，默认关闭，Goal active；
不构建镜像、不准备/执行部署、不合 main。见
[本批接线与验收](DSGA-GuestReady资源执行版本对齐-Mac基本验收-20261024.md)。

## 历史：2026-10-24 实际Agent原Guest绑定和资源代次（仍非整体完成）

AS b9e393f8fa846b19d14c0246c1da46cf0d82a70f已在既有功能分支先推送并核验：
实际Agent先UID/RV条件持久原Guest，再root fence；未知GuestReady完整命令恢复
Pending；新PG POST/显式重放核对持续约束，原未知PG查询可达。资源视图同锁保留
Original/Held/floor和当前限额，不能按PG revision或历史淘汰猜资源代次。
必要Mac race去重8项通过，相关vet及实际Agent/Controller/Runner/root双架构
8次目标编译通过，不是目标内核实测。Neon状态路由1项及clippy通过，控制面
新字段实际Store写入后的TLS路由1项race和准确依赖vet通过；两消费者绑定同一
已发布AS版本，PG17三处仍cbabd5。见
[当前接线与证据](DSGA-Agent原Guest接管与状态转发-Mac基本验收-20261024.md)。

仍缺GuestReady真实资源派发/原观察执行器、完整预算/系统上界、首次准入/全排空、
Host/最终VM、公开worker/生命周期和受控跨boot/owner关闭；这些是代码缺口，
继续实施，不能称为只待Linux。Goal active，仅PG17，默认关闭，不构建镜像、
不准备或执行K8s部署、不合main。最终提交/远程状态以实际Git及恢复点为准。

## 历史：2026-10-24 原Postmaster出生来源（仍非整体完成）

AS `a57bed0cd9b9b24f8447447681b9ffbfda2ec779`已在既有功能分支先推送并核对：
root原位exec前同锁持久Birth，实际UID/exe/cgroup/starttime核验，资源写不再收养
请求PID；固定只读进程来源及客户端接线。Mac新增和受影响4项race通过，最终
参数收紧仅复验受改1项，vet和Linux双架构编译通过。真实内核/exec仍未实测。
Neon/控制面新增GET process只读路由各1项检查通过，clippy/vet通过；实际PG17
产品构建源补PG程序及父目录root保护，未构建镜像，最终权限及扩展兼容待Linux。
两消费者版本绑定已发布AS来源，PG17三处仍cbabd5，其他PG功能暂缓。
见[本批源码/验证/边界](DSGA-原Postmaster身份转发-Mac基本验收-20261024.md)。

Agent资源领域持久协调、完整预算/首次准入/全排空、Host/最终VM、公开worker和
跨boot/owner受控关闭仍需编码，不是Linux待测；Goal active，默认关闭，不部署、
不构建镜像、不合main。各仓最终提交/远程状态以实际Git及工作区恢复点为准。

## 历史：2026-10-24 认证Guest资源通道（仍非整体完成）

AS `a6d191af172fc9fb109b648145cbe6ba484ac94d`已在既有功能分支提交、推送并核对
远程SHA：固定root认证UDS、原Ed25519短JWT二次验证、完整Guest身份同锁fence、
原查询/显式重放、独立于PG boot的TLS消费者、严格Evidence和产品服务源接线。
必要Mac组合6顶层/14含子项pass，认证补充仅复跑该项9事件、不重复计数；相关
vet、脚本语法及Linux双架构root目标编译通过。目标编译不等于内核运行验证。

Neon正式五路转发和控制面固定白名单源码已接入；新增Rust真实UDS/正式路由
各1项通过，clippy all-targets退出0；控制面实际TLS网关及受影响broker签发2项
race通过。自有主线7899ec0已由独立34aa7be完整合入，保留既有服务版本分支。
控制面AS模块与Neon发行锁绑定已推送a6d191af，PG17三处绑定仍cbabd5。
主仓/控制面本批最终提交与远程状态须以Git和恢复点核验，不能借依赖推送冒称
全部仓库已推送。见[本批详细证据](DSGA-认证Guest资源转发-Mac基本验收-20261024.md)。

仍缺Agent资源领域持久协调、完整运行期预算/系统上界生产、首次准入和完整排空、
Host/最终Runner-QMP、公开worker/生命周期、跨boot/owner受控恢复；这些仍须编码，
不能移入Linux待验清单。真实Linux K8s/NeonVM/HugeTLB/物理及性能后验；本轮仅
必要Mac基础验证，不重复已过功能，不构建镜像、不部署、不合main。

## 历史：2026-10-24 原资源来源到Guest启动的产品接线

AS `818ca7713f22a026868973a43897a7d1bdf941c0`已先推并核对：原immutable Secret准确
来源/公钥/额度→Controller直接重读→有界Runner→只读ISO→Guest封存/root cgroup/
实际Bootstrap→vmstart/PG重读，产品程序/模板安装源码接入，未构建镜像或安装
特权文件。旧未认证CPU写/任意路径关闭，保留固定管理TLS供给。最终Mac定向
7顶层/28含子项pass、vet及8个Linux目标编译/模板语法通过，不是目标环境实测。

控制面cedabd5生成同一immutable Secret的config/resource两文件，v2显式平台
额度、原计划深拷贝、缺额度拒绝执行；Mac6顶层/24含子项pass、vet通过。
Neon两套补充脚本不再将受管cgroup委派回postgres，受管模式停止旧无认证WS
Monitor/LFC调整，发行AS锁绑定818ca771。PG17 gitlink/两份PG锁仍cbabd5。
见[完整本批事实与边界](DSGA-原资源来源与Guest启动接线-Mac基本验收-20261024.md)。

独立认证root通道、完整运行期预算/首次准入/排空/Host/最终VM执行、公开worker/
生命周期及跨boot/owner受控恢复仍继续编码；这些不是仅Linux待测。Goal active，
仅PG17，默认关闭。真实Linux/K8s/SUID/整个HugeTLB池/物理与性能后验，不重复
未变内核/SDK/Web/实际PG功能，不部署或合main。

## 历史：2026-10-24 启动硬约束与原位PG出生

AS `c5f5f7eb24827a81b067c5bb964604bc1fcc8590`先推送并核对远程：新增PG不存在时的
真实启动限额/持久原意图、空组核验、原查询及显式重放；固定程序原位入组、全线程
降权后exec，普通postgres身份只能使用出生入口，不能调用资源写动作。重启核验
当前Held，未知资源写阻止新PG出生。Neon三个PG启动点接入Linux受管入口并跳过
越预算临时预热；Mac保留实际PG基础验证路径。见
[本批验收与接线缺口](DSGA-启动硬约束与原位PG出生-Mac基本验收-20261024.md)。

最终Mac Go10项race、Rust新增2项及受影响实际PG启动1项通过；vet/clippy与Linux
双架构编译通过。未执行SUID/迁移/内核实测，未安装特权文件或构建镜像。原VM
sysinit/可信配置与安装、原预算/认证root通道、完整预算/准入/排空/Host/最终VM、
公开worker及受控跨boot恢复仍继续编码。整体未完成，Goal active，仅PG17。
PG gitlink及两份PG17版本锁仍cbabd5；不重复未变内核/SDK/Router全套。

## 历史：2026-10-24 Guest资源硬约束执行器

AS `7ee052a3347132b538723cda98ae7d1ac02fd99e`新增实际Linux固定cgroup硬上限、
禁swap、全部HugeTLB池约束与准确读回；独立于PG boot的持久原意图/旧Held约束/
issuer floor和同步写锁，以及不监听网络的root程序。Mac新增7项race通过，
0fail/skip，vet及Linux amd64/arm64交叉编译通过；不是Linux内核实测。
依赖已先推送并核对远程，发行锁随后绑定；PG gitlink及两份PG17锁仍cbabd5。
见[基本验收与业务缺口](DSGA-Guest资源硬约束与控制链接线边界-20261024.md)。

本模块尚未接入完整受管启动/授权/预算控制链，默认不自动运行。旧官方cgroup
权限会被明确拒绝，不能把可被postgres自行放宽的限额当硬预算。生产权限/启动、
PrepareBudget/GuestReady/FinalizeBudget及独立启动准入、完整quiesce/Host/VM/
公开worker和受控跨boot恢复仍继续编码，不能列为仅Linux待测。Goal active，
仅PG17。Mac必要基本检查；真实Linux/K8s/NeonVM/HugeTLB/物理/性能后验。

控制面同轮独立合入自有主线5ee3bc4（b009b4a），验收记录7773b5b已推送核对。
受影响Go9项、Auth新增1项、vet/TypeScript/Web构建通过；未重复未变DSGA功能。

## 历史：2026-10-24 原生去重成本与Agent最终准入

PG17 cbabd5de541新增独立共享成本API1，固定主段与动态数组去重，BufferABI6不变；
Neon1.11/严格Rust与Go消费者/专用GET/固定网关接线。准确PG/Neon产物与新SQL、
生产Router、契约基本检查通过。Agent现在消费FinalizeBudget后的ResumeAdmission，
写前持久原意图、丢回复只读对账及当前owner显式原意图重放，分类/聚合互斥去重。
见[本批验收与范围](DSGA-PG17共享成本与预算接线-Mac基本验收-20261024.md)，
动作顺序补充入V3第8.1节。

实际预算/Host/Guest约束生产者、独立启动准入、公开worker、完整quiesce/最终VM及
跨boot/跨owner关闭恢复仍继续编码，不能列成仅Linux待验或全功能完成。Goal active，
仅PG17，Mac必要基本/受影响检查；Linux K8s/VM/HugeTLB/物理/性能后验。

## 历史：2026-10-24 原创建来源与合法配置通知

Compute冻结首次可信规格的原创建UUID/tenant/timeline；当前配置通知可更新UUID，
初始化仍核验当前角色/认证及实际PG并回报原创建来源。configure在改状态前拒绝
存储改绑，启动与内部重配也复核。新单元/Router及受影响真实PG初始化各一项通过，
clippy通过；保留首轮测试输入422及修复证据，未重复未变套件，见
[基本验收](DSGA-PG17原创建身份与配置通知-Mac基本验收-20261024.md)。

准入Rust/Go/HTTP/网关上一模块已推送自有分支；全功能仍未完成，真实Agent预算/
启动准入、公开worker、排空/最终VM及生命周期仍继续接线，不列作仅Linux待验。
Mac必要基本验证后按模块推送；Linux K8s/VM/HugeTLB/物理/性能后验，Goal active。

## 历史：2026-10-24 准入客户端、管理 HTTP 与 Go 消费者

原 PG17/Neon1.10 准入已接入严格 Rust/Go 请求回执、未知原意图恢复与显式更高关闭，
external 四路专用鉴权和中心固定名单接线。真实 PG SDK/Router、Go 消费者和网关
必要定向测试通过；保留两次新增测试设置失败及修复证据，未重复无关功能。见
[本批基本验收](DSGA-PG17准入客户端与HTTP-Mac基本验收-20261024.md)。

AS 新依赖66cff2dd先推；Neon发行锁及控制面Go模块随后绑定。控制面新主线0c322d5
独立完整同步95d2002，SSO/OIDC/OpenAPI17pass、vet/cmd/Web构建通过。
真实Agent持久全预算/启动准入、公开worker、完整quiesce和最终VM链等仍待接线；
它们是未完成业务，不是仅Linux待验。Goal active，仅PG17；Mac只必要基本与受影响
测试，K8s/VM/HugeTLB/物理/性能后验；不构建镜像、不部署、不合并主分支。

## 历史：2026-10-24 原准入只读对账与换owner观察

PG17 b144a23b2be增加无副作用状态入口，Neon1.10正式SQL接线；新owner能按旧
完整请求读最后回执，不改epoch重放。缺失/淘汰不是未执行证明，当前open不是
旧预算的成功证明。Mac新增2项真实SQL/resize在途通过，0fail/skip；准确PG/
Neon重编及产物冒烟、版本门禁通过，见
[基本验收](DSGA-PG17准入只读对账-Mac基本验收-20261024.md)。

同轮自有控制面主线新e381caa独立合入并推送506ae1f/记录4b7dfb1，受影响6pass、
vet/cmd0，未重复未变Web/DSGA。客户端/HTTP、真实Agent预算/启动准入、公开worker
与最终执行仍继续编码，整体未完成，Goal active，仅PG17，集群物理后验。

## 历史：2026-10-23 issuer栅栏准入内核与正式SQL

PG17依赖13e109b720d增加独立准入API1，BufferABI6布局不变；绑定已建立的floor、
完整意图摘要、boot/容量/连续准入revision，旧owner不能迟到开门。关门不等resize，
开门需串行锁且无未知PREPARING。Neon1.9正式SQL及升降级安装/新CI用例已接线。

Mac新增内核3项、正式SQL修后2项通过，0fail/skip，首轮漏打包升级脚本失败已修
并保留日志；准确13e PG/配套Neon重编与产物冒烟、版本/源码门禁通过，见
[基本验收](DSGA-PG17栅栏准入内核与管理SQL-Mac基本验收-20261023.md)。

原生HTTP、Rust/Go SDK、Agent预算/启动准入、公开worker及最终执行链仍继续
编码，不能标作整体完成或仅Linux待验。当前仅PG17，Goal active，默认关闭。

## 历史：2026-10-23 全分片timeline原持久意图与恢复

控制面173ae90移除单pageserver候选，使用正式鉴权读取完整分片，原请求在HTTP前
持久保存，仅首次确认可推进；503不作不存在证明，未确认按官方持久幂等显式
重放，确认后丢失拒绝重建。AS e1757bca新增只读8MiB JSON入口，旧控制写上限
不变，依赖先推送核验；发行AS锁同步到该SHA，PG仍fcada未变。

Mac初轮11pass/6fail的上限问题已修，修后17pass及独立迁移/大响应1项通过，AS
新增6pass，均0fail/skip；最终vet/版本绑定通过，详见
[基本验收](DSGA-PG17全分片Timeline-Mac基本验收-20261023.md)。保留首轮失败，
不累计重叠、不重复未变功能；真实存储与集群验证留Linux K8s。

公开worker/预算准入/安全SQL与最终执行链仍继续编码，分支/RO/catalog及生命周期
尚未完整接齐；不能标成整体完成或仅Linux待验。Goal active，当前只PG17。

## 历史：2026-10-23 正式存储JWT接线与最新主线维护

控制面481da30为原Tenant接入固定Service/Secret UID、专用HTTPS/EdDSA鉴权，
冻结原计划引用；旧计划缺引用拒绝执行，SA不代替存储JWT。Mac7顶层/29pass
事件、0fail/skip；456a4ec独立完整纳入自有主线新两项S3维护，受影响54pass
事件、0fail/skip，最终OpenAPI3/vet/cmd/Web构建通过，重叠不累加。见
[基本验收及Linux边界](DSGA-PG17正式存储鉴权-Mac基本验收-20261023.md)。

遵守最新边界：Mac只必要基本与受影响验证；真实RBAC/证书供给/Service/VM/
内存热插拔/故障留Linux K8s。全分片timeline、公开worker、预算准入、安全
SQL路由与最终执行链仍须编码，不能称整体完成。当前仅PG17，Goal active。

## 历史：2026-10-23 原根Tenant与创建恢复

控制面4c25b1f补齐原项目typed Tenant创建与独立不可变存在事实，确认后404拒绝
重建，校验官方完整shard身份。首轮409恢复失败已修，仅定向复验4pass和新增
权限1pass；初轮19pass/2fail及所有原日志保留，515源稳定、最终vet通过，见
[Tenant验收](DSGA-PG17原根Tenant-Mac验收-20261023.md)。

仍须接控制器全分片timeline/完整worker、预算准入/安全路由及最终执行链；
未知外部动作的失败清理必须另行协调，存在事实不等于最终远程fence。Goal
active，当前PG17；不重复未变功能，不把代码缺口记作Linux待验。

## 历史：2026-10-23 初始化观察持久历史

控制面d9950ba按原Operation/VM/Config/凭据/管理Service/PG boot保留首份初始化
观察，始终先真实读取，不回退历史缓存、不重写旧事实；uint64最大boot保留，
越界拒绝。Mac新增7+1范围不同的用例pass，0fail/skip，512源一致，最终vet
通过。见[观察历史验收](DSGA-PG17初始化观察历史-Mac验收-20261023.md)。

**公开worker/完整预算准入与安全路由仍须接齐**，完整跨组件伸缩尚未完成；
其余业务继续编码，不能归为Linux待验。Goal active，PG17为当前范围。

## 历史：2026-10-23 原VM内部Service与初始化实际消费

控制面139d4c0完成两个内部Service的实际typed生产者、原UID不可变绑定及初始化
消费者接线；未知写入原GET恢复，已绑定UID丢失不重建，拒绝路由逃逸/同名换UID。
Mac8/11/受影响初始化17/新增UID2分批pass，均0fail/skip，509源清单一致，vet
通过，重叠不累计。见[Service验收](DSGA-PG17原VM内部Service-Mac验收-20261023.md)。

仍未派发公开worker；初始化持久历史/预算准入/安全路由、分支RO/catalog、pending
清理及完整闭环继续实现。PG仍fcada、AS仍8e1e，未重复未变测试。Goal active。

## 历史：2026-10-23 原实例初始化观察与探针凭据

探针凭据模块控制面242eb6b已推送核验；新四键来源可恢复普通探针身份，RO继承
而管理签名独立，旧三键不补写/不轮转。25+4定向pass，0fail/skip，见
[探针验收](DSGA-PG17独立探针凭据-Mac验收-20261023.md)。

原生初始化观察模块Neon4567158、控制面d7e4229完成生产者和绑定消费函数；实际
Router/SDK/PG17、只读scope、目录/HBA/存储身份及后验journal/lease检查已通过
Mac定向验证。详细运行批次、失败修复、源码与未验证边界见
[初始化验收](DSGA-PG17原实例初始化观察-Mac验收-20261023.md)。远程是否完成以
该批实际推送核对为准；未把本地commit当远程。

**公开worker仍未派发新action**；Service/安全路由、初始化持久历史、预算准入、
分支RO/catalog、pending清理、完整quiesce/策略/Scheduler/最终Runner及存储E2E
继续编码。仅PG17，PG仍fcada、AS仍8e1e；Goal active，不暂停、不宣称整体完成。

## 历史：2026-10-23 公开项目接受与原幂等保留

控制面`1cd5a96d`将PG17项目创建的接受阶段接入实际Session/CSRF/APIKey路由：
先SQL保留原plan/Operation，再typed凭据/来源固定，最后资源图/八步骤/原Operation/
通用幂等键同事务接受。原凭据在网络阶段撤销、锁等待后到期均拒绝；pending计
配额，双向锁防新旧入口夺键，迁移保留旧key行/xmin/OID。最终11顶层/17pass，
0fail/skip，503源一致，vet及OpenAPI3项通过。开发中夹具并发与SQL解析失败已
修复，原日志保留；详见[公开项目接受验收](DSGA-PG17公开项目接受与原幂等保留-Mac验收-20261023.md)。

仅接受阶段已接入，**worker尚未派发新action**；分支/RO/catalog、初始化/准入、
pending清理、完整预算/quiesce/最终Runner及存储E2E继续开发，Goal active，不
把HTTP202当运行完成。不重复旧内核/SDK/VM模板等未变功能；PG仍fcada、AS8e1e。
默认关闭，不构建镜像/部署或合main；Linux实际物理/K8s认证后验，代码缺口继续补。

## 历史：2026-10-23 冻结VM模板与原创建入口，继续完整开发

用户已澄清：必须完整开发PG17功能，基本验证后交Linux，不允许缺业务即停止。
此前暂停结论撤回，Goal active。控制面先独立完整同步mainf582（3aabf76），受影响
AI17/Gateway28事件与Web新增3项/构建通过；再完成VM模块`3948b2be`：原计划冻结
VM profile、官方typed held模板/TLS供给、tls.key/tls.crt实际路径、完整启动规格
核对、原持久UID恢复入口、VM/Config429禁止透明POST重试。

Mac首轮10顶层/45pass事件，补返回终检后仅重跑入口1项通过；生产者其他函数
字节不变，最终498源清单一致，vet通过。详见[VM模块验收](DSGA-PG17冻结VM模板与原创建入口-Mac验收-20261023.md)。该入口尚未
接公开handler/worker；catalog/初始化回执、完整预算/quiesce/最终Runner与存储
E2E继续编码，不把ready当普通SQL或VM许可。PG仍fcada、AS仍8e1e，不重复旧内核
等未变测试，不构建镜像/部署或合main。Linux实际证书/Guest/物理回收必须后验。

## 历史：2026-10-23 基本验证模块记录（暂停结论已撤回）

按所有者最新要求，基本验证后本轮结束，不再扩展新模块或重复未变测试，其他PG
暂缓，每仓继续同一汇总分支。控制面代码`1a7b51f4`补齐接受前凭据生产/
冻结UID及摘要；真实Renderer在首次来源绑定前核验。新增及受影响8顶层/27唯一
pass事件，真实PG17四类口令各经pgx/libpq认证通过，vet通过。首轮夹具误分流的
两组失败如实保留，仅补跑失败两组；不称最终源码单次全套通过。

详见[本轮基本验证与Linux交接](DSGA-PG17本轮基本验证收口与Linux交接-20261023.md)及机器摘要。本批PG仍fcada、AS仍8e1e，
Neon生产Rust/内核未变；不重复旧核验。Linux/K8s/NeonVM/HugeTLB及真实物理回收、
并发故障与性能必须后验。完整V3仍有公开worker/catalog/TLS初始化回执、完整预算/
quiesce/最终Runner及存储E2E业务接线未完，不能改记为仅待Linux测试。按最新指示
本轮提交后停止继续实施，持续目标暂停；默认关闭，不构建镜像、部署、合main或发布。

## 历史：2026-10-23 主线024维护同步

控制面`2b89687`完整纳入新main024的组织隔离/路由/计费及063迁移，30文件逐字节
一致，DSGA模块未改。只跑受影响AI34/Gateway70事件及vet/Web构建，零fail/skip，
未重复旧PG/DSGA/SDK/Go或未变Web47。详见
[主线024定向验收](DSGA-控制面主线024同步与Mac定向验收-20261023.md)。

按所有者最新要求，仍只完成当前PG17、每仓原汇总分支供Linux验证；凭据生产者/
完整catalog/公开worker/初始化回执、预算quiesce/最终Runner/真实存储E2E继续编码。
此处AI预算不替代内存预算，默认关闭，Goal active，不构建镜像/部署或合main。

## 历史：2026-10-23 原创建凭据来源与真实Renderer回调

控制面`e83946f`新增原Operation/VM绑定→typed immutable凭据GET→安全Renderer→
来源UID/完整Data域摘要固定/核对的实际Render回调；高位900000060追加迁移保护
来源事实，恢复不重写、同UID内容漂移拒绝、锁等待后lease过期拒绝。新增race
4顶层/18pass事件及强化的2场景/3pass事件通过，实际摘要门禁反向确实触发断言，
恢复原代码后通过。其余函数未变，不重复旧功能测试。详见
[凭据来源Mac验收](DSGA-PG17创建凭据来源绑定与Mac验收-20261023.md)。

本回调尚未接公开worker；DSGA凭据创建生产者、完整catalog、VM/TLS模板、原实例
能力回执及预算/quiesce/最终Runner/真实存储E2E继续编码。仅PG17、每仓原汇总
分支，PG仍fcada、AS仍8e1e。Goal active，不能改记为只有Linux待测。

## 历史：2026-10-23 PG17安全启动Renderer核心与真实解析契约

控制面`7c5da7f`从原冻结计划/真实VM UID及业务verifier/管理公钥生成确定性完整
Compute配置；Neon`a9ce645`加入本次Go导出→真实ComputeConfig/ParsedSpec门禁。
新增Go race4顶层/15pass事件与Rust1实际通过，Go零skip、Rust零ignored/filtered；只验证新增
路径，未重复旧业务套件。详见
[Renderer契约验收](DSGA-PG17安全启动Renderer与Mac契约验收-20261023.md)。

这仍是bootstrap核心；完整catalog/凭据来源UID绑定、公开创建worker/VM模板及
原实例初始化回执未接齐，预算/quiesce/最终Runner与真实存储E2E继续编码。
本阶段仅PG17，每仓同一汇总分支；Goal active，不改记为只有Linux待测。

## 历史：2026-10-23 PG17私有认证与冻结启动契约

代码`1015c804`实现私有0700 socket/peer、先hold再发布受管业务HBA、随机nonce
实际加载确认、Postmaster参数覆盖备份认证配置、全部7字段及端口冻结；坏配置
保留原查询/取消身份并拒绝新resize。实际PG17主/物理只读副本、原生HTTP和新增
路径均完成定向验证；未变用例复用原证据，不称全lib24/HTTP13或新CI151全量通过。
准确代码产物重编、BUILD_TAG与哈希核对通过。详见
[私有认证与启动契约验收](DSGA-PG17私有认证与启动契约-Mac验收-20261023.md)。

本阶段仍只做PG17、每仓一个既有V3汇总分支。完整Renderer/公开worker/原实例
来源回执、预算/quiesce/最终Runner及真实存储E2E仍须编码；本地HBA加载确认不
等于中心初始化回执。Goal active，默认关闭，Linux后验范围不变。

## 历史：2026-10-23 PG17统一分支与复制定向验证

项目所有者要求本阶段只完成当前PG17，其他PG暂缓；每个仓库只使用既有V3
汇总分支作为Linux集成入口。分支矩阵、去重策略和本批真实结论见
[PG17统一分支与Mac定向验收](DSGA-PG17统一分支与Mac定向验收-20261023.md)。

PG提交fcada44b收紧业务物理数据/复制槽权限，保留业务逻辑流与内部真实备份；
新增6项原生专项通过、只补跑新增startup ROLE断言，准确SHA重编/产物检查通过。
未变更的上批测试不重复执行，也不将旧9cf证据改标为新PG全量通过。PG17完整
Renderer/HBA/来源回执、预算/quiesce/最终Runner及真实存储E2E仍未完成，Goal active。

## 历史：2026-10-23 原生角色模块与 Mac 最终复验

最新角色约定、源码范围与验证证据以[原生角色模块交接](DSGA-原生角色模块与Mac验收-20261023.md)
及其机器摘要为准：业务角色禁止包含内部管理身份，cloud_admin沿用官方本地管理
通道且PG口令为NULL；主库原子应用、RO真实回放核验、SQL认证目录保护和延后
代理配置已接线。本批不是完整V3，Renderer/HBA/初始化回执、完整预算/quiesce/
最终Runner、多PG与完整存储E2E仍须编码。Linux物理/K8s验证另验，不能混记。

代码模块`1b7b2bd`：Mac完整CI145通过0跳过（230.650秒），角色18/HTTP12/
SDK10/Go跨仓链/新鲜契约均实际通过；默认19 ignored全部经严格门禁显式执行。
最新控制面dd6e040已推送核对，完整API1426通过事件、Web47及静态构建通过。
逐项范围/源码与日志SHA256见模块摘要，不将组合模块通过宣称为完整业务E2E。

下面同日和此前段落都是对应批次历史快照，不代表当前最新控制面SHA和测试数量。

## 历史：2026-10-23 Mac 本批收尾与 Linux 交接

本批权威入口是[Mac收尾与Linux交接](DSGA-Mac收尾与Linux交接-20261023.md)及
其中机器摘要。下面保留的2026-10-22文字是历史快照，版本/推送状态不能覆盖本节。

本轮fresh fetch完整纳入社区PG17 `e05b7e2a` 和自有控制面main `c8767714`；
官方Neon/Autoscaling/Neon PG17头未变且已包含。独立PG维护为 `d70986fd`，
功能组合为 `9cf674a5`；主仓gitlink、ci/distribution与vendor/revisions三处一致。
控制面 `0f929586` 已推送并核验，Autoscaling仍为已发布的 `8e1e0ea9`，本批未改代码。

Mac完成本批核心/隔离/复制边界、原生内核与正式SQL、真实Go/Rust/PG链路验证；
逐项数量、准确SHA、跳过/未执行及本批两次失败的修复证据见交接文档。控制面
最终全API race1369事件、Auth服务13项、Web43项及类型/构建通过，0用例fail/skip；
控制面这轮实际PG仍为准确0edff，不冒称新9cf运行。新版本prepare145仍为
13通过/132环境skip，不能称145全部执行。

**完整V3仍未完成。** 安全Renderer/租户角色隔离/公开创建worker、完整预算/
quiesce/Monitor/Scheduler/最终Runner、跨owner/boot关闭、多PG与完整Neon存储
E2E仍须编码；不能归为只有Linux待测。Linux/K8s/KVM/HugeTLB物理回收、故障
迁移和性能长稳后验。默认关闭，不构建镜像、不准备/执行部署、不合main。

## 历史接续：2026-10-22 创建计划、058前向兼容及PG17维护

本节是当前事实；下面阶段文字保留当时快照，不能覆盖本节。**整体V3仍未完成，
Goal active，默认关闭。已完成代码分批推送，未完成业务不能归为只有Linux待测。**

| 组件 | 当前功能提交/状态 | 已完成范围 |
|---|---|---|
| Autoscaling | `8e1e0ea93d5f2775376622a38ed10f39d9d1fc8c`，已推送核验 | 实际Agent/凭据消费者、持久held/ready、完整journal、原Config UID/VM owner、Controller/Agent保护 |
| control-plane | `5be4f5678c40d718c2a9472bc2c2ecb6f5baa8cd`，已推送核验 | 含创建计划模块60e74bf、真实UID绑定/typed创建核心、中心签发/六路由、旧入口保护；完整安全Renderer/公开入口/worker仍未接齐 |
| PG17功能 | `0edff73d4779ee2f26d2394017370008e16ca132`，已推送核验 | 完整包含Neon56692与社区17f66，新增恢复期unlogged COPY拒绝及Windows时区维护，保留既有Neon/DSGA业务 |
| PG17维护 | `4fbbcddf3298c41f872ad78d3d7abdf82181b6ad`，已推送核验 | 独立完整官方维护层；3个官方文件逐字节与17f66一致、作者/注释保留 |
| Neon发行绑定 | 本批三处绑定0edff，准确重编/门禁已完成 | Autoscaling仍8e；不把上一个准确1edef或候选版本结果代替新SHA认证 |

本轮fresh fetch：Neon官方main `fa504217`、Autoscaling `a4bce09e`、Neon PG17
`56692dfb`未变且已包含。社区PG17从2b66前进至 `17f66a68179bfe91a57987ffafafa9063c3907c7`，
两项新维护完整纳入独立4f与功能0edff。自有control-plane main从69c611d前进至
`2bf6206321ebedb1ac53ffd8870b9ecfd5609bb9`，已由独立同步分支
`codex/sync-control-plane-main-20261022` /5be完整纳入，再快进功能分支，不改main。

### 创建计划与真实存储入口

控制面60e在原Operation中冻结可信资源/身份计划：PG17、namespace、独立随机
incarnation/Compute ID/UUID、UTC时间、sha256镜像引用、整数bytes的bootstrap/M/
最低未来Guest/reserve与槽边界。真实版本化数据库生产者与原资源/步骤/idempotency
同事务；恢复核对完整原JSONB摘要、严格读取原计划，不重读env或重分配身份。

实际typed Kube存储入口先GET原目标以恢复未知写入，首次创建前GET祖先核对
真实tenant/timeline/PG版本；POST成功/409均重读原版本/祖先/LSN。固定中心TLS/
SA、有限期限/响应/速率，禁止redirect及429透明POST重试。当前沿用既有Driver
unsharded路径；服务TLS夹具不冒充实际storage-controller/pageserver端到端。
原生v2启动预算由真实VM UID绑定后生成；仍不是完整安全Compute配置Renderer。

该模块真实SCRAM PG与TLS/typed API专项200事件通过；补强CPU独立上界及6项
KB/uint64边界后，准确最终源码全API race1335事件（359顶层/976子）通过。
4项实际生产反向抓到预算取整、原摘要、存储版本与透明POST重试错误并恢复；
全仓vet与Linux/amd64全部cmd交叉编译通过，非Linux运行认证。

新增可复验 `ci/test_dsg_creation_plan.py`：新目录实际Go导出→真实Rust生产DTO
validate/postgres_settings，11组数据（2有效/9拒绝）含完整uint64，1 Go/1 Rust
实际通过；源码/数据hash一致。Rust默认compute_api lib/tests为36通过/1默认ignored，
该外部契约由显式脚本另实际运行，不把默认ignored算成运行通过。
证据为 `build/dsga-plan-cross-language-final2/`，不是完整PG/VM创建认证。

### 058分支冲突与数据保留

新主线058_catalog与已发布DSGA058同号；原数字登记会静默漏掉一份，Git自动
合并没有冲突也不安全。5be保留两份058原字节，追加高位900000059前向join，
同事务补齐缺失的完整DDL；关键列/键/引用/启用的不可变守卫不完整则失败回滚。
登记器只允许这一已审查同号对与配套join，拒绝其他重复/非法integer编号；后续
DSGA使用独立高位段，不重命名或改写历史，也不伪造058曾分别执行的历史。

真实旧001—057加原主线058/原DSGA058及新库三种来源、双次升级全部验证；两种
旧来源种入完整FK审计/UID行，整行/xmin/OID/原登记时间保留；禁用守卫则DDL与
成功见证均回滚。组合专项225事件，2项实际生产反向恢复后，最终**全API Mac
race1356事件（367顶层/989子），11测试包通过、9无测试包单列，0用例fail/skip**。
真实SCRAM PG准确1edef/PostgREST16.4实际运行后关闭；最终源码hash一致，全仓vet
和全部Linux/amd64 cmd交叉编译通过。继承Web UI没有在本批重新做浏览器认证。

详见自有control-plane的《DSGA-冻结启动计划与存储版本贯通》《DSGA-058分支迁移
冲突与主线同步-20261022》。根工作区 `build/control-plane-plan-mac-final2/`、
`control-plane-main2bf-join-mac-final/` 及各自negative/vet/Linux日志保留。

### 新PG17维护与准确二进制

纯维护候选225核心通过；首次主备未预载Neon rmgr而失败，补编/预载后63项中
61通过、2项旧Neon序列不预分配与社区33预期差异失败；新增unlogged COPY拒绝
实际通过。功能组合保留已有864d32的Neon真实序列1预期，225核心及DSGA off/on
两轮主备001各63项全部通过。失败与功能组合结果分层保留，没有改写官方新增断言。

准确0edff的构建/安装二进制已都核对--version：**80项原生内核120.766秒、
52正式SQL80.246秒、真实Go8e→TLS→Rust Compute→SQL→PG门禁8.88秒均通过**。
后者1 Rust/2 Go race进程、0skip/ignored/filtered，覆盖初始化/释放、epoch2持久
循环2048→4096、丢回复对账/原意图重放和重启；它仍不等于整个中心网关/完整
Neon存储链已交付。同一准确二进制的225核心及off/on各63项主备也全部通过，
新COPY拒绝实际执行；两轮没有skip。原生日志与SHA256归档于
`build/upstream17f66-exact-summary.json`，源码与安装二进制均准确0edff。
候选字符串feature17f66-candidate不当作准确SHA证据；源码prepare145是13通过/
132环境skip，不称145都跑过。当前三处PG绑定一致，依赖先推送并核对远程SHA。

前一准确**1edef**的off/on check-world各48报告/231文件/18414报告断言（含平台/
可选skip）及其XID/libpq补验，保留原日志，**不写成0edff新SHA的全check-world认证**。
XID on003首次超时无TAP plan失败；相同代码/期限的单文件在进程级caffeinate下
718秒补跑通过，休眠日志仅相关，未认定唯一根因。ICU/readline及完整SSL/GSS选项
组合未覆盖是构建选择，不是Mac硬件不支持；Windows专用项不适用于Mac。

### 还须编码与后续目标环境

1. 公开创建选择、完整安全Renderer、私有管理/普通租户凭据与真实角色/SET ROLE
   隔离、VM模板与worker真实调度；项目/Operation/Timeline/Guest必须贯通原版本。
   原Driver把租户密码赋给SUPERUSER cloud_admin，不能沿用后开放DSGA租户准入。
2. 完整预算/quiesce、Monitor/Scheduler/Controller/最终Runner fence、跨owner/boot
   受控关闭、其他PG大版本及完整Neon存储链端到端。以上是代码与Mac集成工作。

后续Linux集群验收：真实K8s CRD/admission/RBAC/投影令牌/TokenReview/etcd，
Linux/KVM/NeonVM/HugeTLB、Host/Guest物理回收、迁移/故障恢复、完整存储负载及
性能长稳。Mac SQL/TLS/CRD CEL解释器/交叉编译不替代目标认证。已完成模块可进入
目标阶段，整个V3仍须补齐上述业务；不构建镜像、不准备/执行部署、不合main、不发布。

## 阶段记录：Agent实际入口、中心六路由代理与官方升级测试修复

Autoscaling `f60ff588b4dba403885dcd371caa412079ae87a5`已推送自有功能分支并
核验远程。模块化中文提交4d21b97/f60ff58完成管理CA/投影SA短凭据消费者、
完整空journal才允许的首次PG boot发现，以及真实Agent受管入口/长期PGWorker。
固定UID读取失败、标记损坏或缺配置均不回落legacy；凭据刷新不重建SDK、不清
未知意图。新增协调心跳不等于PG/VM许可。受影响最终race303事件/13测试包，
9无测试包单列、0用例fail/skip；native标签lint0。真实Go→Rust Compute→SQL→
PG17 c848门禁9.24秒通过，1Rust/2Go race进程，0skip/ignored/filtered。

Control-plane `bbc2a70348793b55dc854f4385db67ea752e1e14`**已本地提交、尚未
推送**，固定引用自有f60ff58。在独立:8444实际生产mux接六个DSGA路由，严格
SNI/专用JWT/VMUID/CIID/完整journal/当前Lease及owned Service，内部原生3080
仍验签。执行和观察/取消双池、有限期限/大小、不透明重试POST。实际Session→
credential.Client→中心核验→SDK→生产代理的TLS/HTTP测试通过，最后native仍为
协议夹具；不可将其与上述另一条原生PG门禁拼称新中心到真实PG完整E2E。

最终完整API race956事件（255顶层/701子），9测试包/10无测试包，0用例fail/
skip，含68个DSGA事件；实际PostgREST16.4 RLS及一次性SCRAM PG已运行并关闭。
三项实际生产安全变异被对应业务断言捕获，逐字节恢复/源码hash核对后全量重验；
全仓vet、全部Linux/amd64 cmd交叉编译和实际Agent交叉编译通过，非Linux运行。
详情见自有Control-plane `docs/DSGA-凭据签发与Mac验证.md`。

fresh fetch社区PG17前进到官方`80da124a4f338a612550180dac86eb33850e2254`，
独立维护本地提交bdf0b191、功能合并本地提交
`586262aef0ade229ca1067540cca1ecc587775fc`完整包含新维护及Neon56692dfb。
功能组合仅官方AdjustUpgrade.pm一文件变化，生产C/ABI不变，无冲突并保留原作者。
维护核心225及实际调整模块13项通过；纯维护组合缺既有694d751的Neon slot修复，
pg_upgrade003失败如实保留。合入功能组合、正确rmgr/显式TAP后4文件86项全通过。
当前准确586提交已Mac重编安装成功，三处绑定一致；未把此前c848门禁误写成新
SHA实际执行。新的80内核/52 SQL/原生门禁尚待可用审批环境复验。

**远程状态必须区分：** f60ff58已推送；bdf0b191/586262a/bbc2a70尚未推送。
推送PG自有分支的工具调用在执行前被自动审批服务用量限制阻断（提示11:43后
重试，非不安全判定），不绕过审批；主仓新引用不先于依赖发布。最新官方Neon/
Autoscaling默认分支本次fetch未变。没有镜像/K8s/主分支/生产发布操作。

**整体仍未完成，Goal active。** Driver/API/Webhook/Controller创建初始化屏障、
完整预算/quiesce、实际Monitor/Scheduler/最终Runner、跨owner/boot受控关闭、
新中心到真实PG链、多PG版本及完整存储E2E继续编码。这些是代码/集成缺口，
不能归成只有Linux待测；目标Linux/K8s/NeonVM/HugeTLB/物理及性能另验。

## 阶段记录：独立TLS的中心凭据服务已推送

自有Control-plane `d30ab57f1e56ed9d3b4c2ecb1523e0c9c93402b0`已推送功能分支并核对
远程SHA；包含独立Mac编译修复ef446e2及实际生产网关凭据服务，固定复用自有
Autoscaling3eeafd9的完整journal/TokenReview。专用JWT不分发签名seed，读取私钥
后再验当前owner；默认关闭，TLS/SA半配置拒绝启动。

复核发现旧每Compute TLS私钥也挂载给Guest，因此已补**控制面独占证书的独立
TLS监听:8444**，强制当前管理域wildcard/ServerAuth身份，拒绝旧Guest证书及
错误SNI；原:8443 configure不受影响。未来Agent必须消费可信管理CA，并拒绝
Guest身份，不能把私钥分发给Agent或从普通VM请求接受URL/CA。

Mac最终完整API race908事件（248顶层/660子）、9测试包/10无测试包，0casefail/
skip；同CI版本官方Darwin PostgREST16.4的真实RLS、一次性SCRAM PG均实际运行。
四项生产反向真实失败后逐字节恢复，20个DSGA事件及全仓vet再次通过；Linux
全部cmd与Functions测试程序交叉编译通过（非Linux运行），没有镜像/K8s操作。
详见自有Control-plane docs/DSGA-凭据签发与Mac验证.md。GitHub功能分支未发现
CI run，不把未运行的Linux CI算通过；不创建/合并生产PR。

**整体仍未完成，Goal active。** Agent凭据客户端/真实入口、管理操作代理、
首次boot发现及跨owner/boot关闭、Driver初始化屏障、全部预算/Monitor/Runner、
多PG版本与完整存储E2E继续编码；真实TokenReview/RBAC/物理回收等目标环境后验。

## 阶段记录：远程凭据主体核验与PG17维护同步

Autoscaling `3eeafd997c8cc9079fcf90ffd0df27aa32c13ce5`已推送核验，新增生产PodVerifier：
专用TokenReview、真实SA/Pod UID、随机进程holder、同Node/生命周期、完整journal/
固定Lease二次读取及独立期限。既有Session.Check复用统一核验，不另写owner镜像。
受影响最终race278事件/12测试包、9无测试包，0case fail/skip；native标签lint0issues。
新增3项实际断言反向后逐字节恢复并全量重验。**这是主体核验核心，不是已接通的
凭据HTTP服务或Agent实际入口**，完整预算/Monitor/Runner等仍须实现，Goal active。

重新fetch发现社区PG17新增官方维护467bb7ea，已完整合并为自有
`c8482eeec4710aa3035a1850b41f77586e3731d4`并先推送核验。修复IS DOCUMENT反编译
遗漏括号，官方代码/回归保留，无冲突。候选完整核心225/225通过；三处PG绑定及
Autoscaling发行引用已在工作区更新。准确SHA重编、80内核/52正式SQL、真实Go链路、off/on全部world/TAP、SDK10/HTTP12
均通过；world含可选skip，prepare145为13通过/132环境skip。详见
[本批验证](DSGA-主体核验与20261021上游验证.md)。Control-plane实际凭据路由草稿
正在本地编译测试，尚未推送，不计完整交付；Agent/Driver/预算/Runner继续。

## 阶段记录：所有权选主与持久PG循环

Autoscaling生产所有权/循环1bf247d及阶段门禁补强
`75187718d7f4e99ed46924305e89ad83a943b0de`已推送核验。固定Lease/VM绑定、journal
CAS单调epoch、真正退出join、有界API/凭据等待、持久PG循环均实现；真实epoch2
循环实际扩到4096并验证丢回复恢复及显式重放。最终race254事件/12测试包，9无
测试包分列，0fail/用例skip；native lint0issues。8生产反向恢复后全量重验通过，
阶段缺失的PASS-only门禁反向另被拒绝。详见[新模块](DSGA-所有权与持久循环验证.md)。

尚未接Agent实际入口/凭据发放、完整预算/Monitor/Runner等，不关闭R03或Goal。
V3已补充选主与最终fencing、不可改写旧epoch及SDK受控关闭约定。PG6b7/三处锁不变。

## PG持久执行适配器（阶段记录）

Autoscaling `48f01e2aee8208ba1ab5392a518434e276086066`已推送核验，PG写前派发、
完整原意图恢复、显式安全重放和四数组Provider单独核验完成。受影响race219事件/
11测试包、9无测试包分列，0fail/用例skip；lint0issues。真实journal→Compute/JWT→
SQL→准确PG链路及实际重启已通过，4项实际反向失败后逐字节恢复并全量重验。
本批未接Agent可信owner循环，全部预算/Monitor/Runner等仍需继续。扩展envtest缺
etcd的0/2失败单列保留；详见[执行链路](DSGA-PG持久执行链路验证.md)。

## PG接管隔离批次（后续历史证据）

PG17 `6b7c3655ab74062e117eef7400953cd000679a49`已先推送核验，新增独立单调
issuer floor及原子登记检查；COMMITTED仍在同步Provider时接管只返回Pending。
gitlink/两份锁与准确重编二进制均绑定6b7。Neon扩展1.8/Buffer ABI6、六个正式
SQL/HTTP入口、Rust/Go独立观察通道接管均已接线；旧未知意图不会因Fenced清除。
Autoscaling依赖`831a453e3adaa86f163027a1f7319139f78b55c5`已推送核验。

准确SHA严格门禁：内核80项120.900s，正式SQL/存储52项78.346s，SDK10项14.42s、
HTTP12项6.08s均零skip/ignored/filtered；真实Go门禁1 Rust外层/2 Go race进程
通过。普通Rust36/31通过，其15ignored已另严格执行；Clippy退出0。Go完整race185
case pass事件、0fail/用例skip，10测试包/9无测试包分列；普通/native lint0issues。
prepare145项为13通过/132环境skip，实际80+52已另执行。候选world与已提交PG src
逐字节匹配，各off/on全world+TAP成功但含可选skip。两种实际反向均失败、源码已
恢复。详细契约、日志、升级限制及复现见[本批记录](DSGA-PG接管隔离与正式API.md)。

PG floor不是租约/VM许可或全预算；可信owner/持久循环/Monitor/受控重绑与下文
剩余业务继续实现。Neon本批提交与远程状态以Git/refs为准，不把工作树当已推送。

## 当前已经实现的范围

- PG17普通页Provider、固定地址四数组目录、L/H/M边界、Grow、尾部排水与逻辑
  Shrink、逐数组保守回收/regrow；pin/IO/CV、checkpoint工作区与异常恢复保护。
- 安全小池启动、共享布局预算核验、同Postmaster恢复换boot、受管新boot默认
  关闭普通准入，保留真正管理员的恢复通道。
- 内核R09：独立receipt/observed、当前操作外16条终态、epoch/hash绑定、过期
  拒绝、实际业务取消、FATAL遗留对账、完整逐数组释放身份及单调issuer floor。Buffer ABI6。
- **现有Neon扩展1.8的六个正式管理SQL函数**：capabilities、memory_status、
  request_resize、resize_status、cancel_resize、fence_issuer；真正superuser会话检查，严格
  原始JSON/唯一键/类型/范围校验，完整意图hash及固定VM/Compute身份绑定。
  这些是已加载验证的函数，不是以C测试桥替代管理端点。
- Compute启动契约v2：必填VM UID与Compute实例ID，配置/原生启动参数同时固定；
  备份auto.conf/include不覆盖当前绑定，重配不静默改变身份。普通静态spec兼容；
  旧实验v1须补齐身份后受控重启。
- 共享Rust管理客户端：严格响应类型、真实双SQL会话、可信schema/扩展成员核验、
  超时/future取消后保留完整原意图、独立状态/业务取消。已实际运行10项原生集群
  用例；已owned接入Compute HTTP，Monitor仍未接入。另拒绝NOINHERIT但可SET ROLE
  管理员的成员关系，保留旧校验漏拒绝的真实负向证据。见[客户端模块](DSGA-Rust双连接管理客户端.md)。
- Compute六个正式HTTP路由与OnceCell长期双连接：严格专用短期JWT认证，不沿用
  Hadron免认证；绑定Compute/VM/实例/epoch，限制原始JSON，禁止缓存；配置期间
  仍可查询/取消在途操作，后端断连不隐式换客户端。12项实际路由门禁通过，含
  4项原生/TCP链路。见[HTTP模块](DSGA-Compute管理HTTP与运行时接入.md)。
- Autoscaling自有分支已真实接入Agent固定UID/Runner身份、动作原资源快照、
  UID/resourceVersion条件写入及响应核验；保留同Revision分批批准。另完成Go
  请求/查询的无损完整身份契约，以及完整意图/命令的持久执行日志、写前条件提交、
  未知动作/接管/取消恢复与有界历史；受管标记禁止旧路径回退。Go正式消费者已
  编写，且到真实Compute/PG的整体门禁已执行；owner选择/PG循环已实现，Agent入口/凭据及完整领域循环仍待集成；
  账本不是租约认证或VM许可。
- 完整Neon扩展的共享LSN锁、独立SLRU接口、安全文件发布与Buffer提示保护；
  原生恢复/逻辑槽/配置样例/资源释放缺陷修复及保留的反向证据。

管理SQL只返回PG/Provider事实，**vm_shrink_permit始终false、production_ready
当前false**。预算引用不是预算证明，PG提交不是VM完成；外层完整资源承诺、issuer
及最终执行约束须继续实现。详见[SQL模块](DSGA-Neon管理SQL与实例身份.md)与
[R09内核接入](DSGA-R09-内核回执与取消接入.md)。

最新批次补齐Go正式六路由消费者/双连接池/未知意图栅栏及ABI6严格事实类型，
拒绝null标量补零，并与Rust一起重算完整PG context摘要；不能借用同Key下不同
VM目标/预算/期限的回执。见[跨语言摘要核验](DSGA-完整意图摘要跨语言核验.md)。
Go客户端尚未接Agent协调循环；Go→真实Compute→PG同一门禁已另完成，含实际
提交丢回复、原意图重放、数据校验及真实PG重启拒绝静默重绑。见
[真实链路门禁](DSGA-Go客户端真实链路验证.md)，它不等于完整Neon存储/VM端到端。

## 分支、源码与上游（历史快照，以本页最新接续为准）

| 仓库 | 分支/身份 | 状态 |
|---|---|---|
| 实际PG17子模块 | codex/dsga-pg17-v3-20261019 / 6b7c3655ab74062e117eef7400953cd000679a49 | 已先推送并核验GitHub refs，工作树干净 |
| Neon主仓 | codex/dsga-neon-v3-20261019，gitlink/两份锁均6b7c3655 | API/ABI6集成960551d已推送核验；本轮PG持久链路文档随当前提交，恢复以Git/refs为准 |
| Autoscaling | codex/dsga-autoscaling-20261019 / 75187718d7f4e99ed46924305e89ad83a943b0de | 日志/HTTP消费者及真实Go联调已推送核验；完整运行链仍未完成 |
| control-plane | codex/dsga-control-plane-v3-20261020 / 928eec95… | Driver等运行时尚未完成 |
| Helm | codex/dsga-neon-helm-v3-20261019 / 648488a… | 本业务配置/验收链尚未完成，不准备部署 |

本HTTP批次再次fresh fetch：Neon main仍fa504217c61bbcaf5c512d75830564541f917f8f，
Neon PG17仍56692dfb680281a963c7470fc7f0fec7f65ecfd4，社区PG17仍
91246c72695cdddf62003f6bf4c4ade511f134f4；均为对应功能头的祖先。维护合并
4a39412f…/873715d889保留；没有只挑选部分bug提交或覆盖自有功能。其他版本
移植前须分别重新核对，不能称PG14–16/18已支持DSGA。

repos/postgres仅为实际repos/neon/vendor/postgres-v17的兼容符号链接。
主/稳定分支未合入本业务，见[同步规范](上游同步与功能分支规范.md)。

## Mac验证证据（旧批次保留，最新以顶部两节及模块记录为准）

| 检查 | 真实结果 | 结论边界 |
|---|---|---|
| PG准确486338重编及内核专项 | 69项通过127.064s，零skip | 原48项加21项R09，不重复计数 |
| 486338原生off/on | 各基础81组/666项；各48份TAP报告、231报告文件/18410报告断言，46PASS/2NOTESTS/零FAIL | 报告总数含平台/可选skip |
| 本SQL批次完整Neon清理/构建/安装/加载 | 47项通过85.043s，零skip | 原22项存储专项加25项正式SQL测试，含Rust communicator |
| Rust compute_api/compute_tools普通全包 | 32项/30项通过，零failed；另SDK8/HTTP4项ignored | 12项原生用例已另行严格实测，不能将普通运行的ignored算通过 |
| Rust双连接SDK原生门禁 | 8项通过12.27s，零ignored/filtered | 含NOINHERIT/SET权限补强及owned生命周期 |
| Compute HTTP严格门禁 | 11项通过6.09s，零ignored/filtered | 7项路由/认证、4项原生，含实际TCP→SQL→PG及数据校验 |
| Rust Clippy | 两包已有门禁及客户端feature --no-deps --all-targets -D warnings退出0 | 依赖仍有remote_storage既有27条warnings |
| 最终prepare | 129项：13通过、116环境skip | 69+47已另行真实执行，不能把skip算通过 |
| C/脚本检查 | PG17及旧14/15/16头严格-Werror语法、pgindent、Ruff、Bash、diff、版本绑定通过 | 旧头仅证明不支持路径可编译，不代表功能移植 |

日志位于工作区根build/：neon-r09-exact-{build,lifecycle,extension,world,prepare}.log、
neon-r09-api-{strict-extension-final,compute-tests,clippy,prepare-final}.log。
客户端另见neon-dsg-client-{packages,default-final,strict-final,clippy}.log。
HTTP与最新权限补强见neon-dsg-http-{packages-final,strict-final,clippy-final}.log。
本HTTP prepare仍129项：13通过/116环境skip，日志neon-dsg-http-prepare-final.log。
Autoscaling追加Mac race组合132个pass事件（含父/子测试及fuzz种子）、0fail/用例
skip；8个有测试包通过，9个无测试文件包不计业务通过。Agent31/既有相关83/Go
协议18分记；API竞态两项真实反向失败，恢复后通过；请求15秒fuzz实际1275872次
通过，本地v2 lint 0issues。见该仓docs/DSGA-执行状态与恢复.md，日志工作区
build/autoscaling-dsga-cas-{full-final.jsonl,lint-final.log,negative.log}等；不是
apiserver/etcd、真实QMP或VM认证，没有远程LinuxCI执行证据。
持久日志c5db379追加最终Mac race组合161个pass事件，0fail/用例skip；9个有测试
包通过、9个无测试文件包单列；journal17顶层/2子测试及fuzz父/4种子共24pass。
恢复解码fuzz实际35835次输入、22.873s通过，v2 lint0issues。分别移除RV/UID条件
的定向HTTP/RFC6902反向均失败，逐字节恢复后最终race通过；UID项明确检查同名
新VM未被误写。见Autoscaling docs/DSGA-持久执行日志与恢复.md、工作区
build/autoscaling-dsga-journal-{full-final.jsonl,lint-final.log,fuzz-final.log,negative.log}。
新模块未自动打标/启用，不验证领导租约、不执行PG/VM动作。不是完整R03验收。
Go HTTP批次最终Mac race183pass事件、0fail/用例skip，10测试包通过/9无测试包
单列；本地v2lint包含plugin全范围0issues。既有随机评分测试误判已保留实际失败
并单独修复，定向race100轮通过。Rust摘要原检查漏检已做真实反向，恢复后原生
SDK8/HTTP11分别14.30s/7.46s通过，两包普通34/30通过、Clippy退出0；默认的
8+4 ignored已另实际执行。日志neon-dsg-context-*及autoscaling-dsga-http-*。
后续真实Go链路门禁1个Rust外层/2次独立Go race进程通过；9f4669f准确依赖复验
4.53s，零skip/ignored/filtered。共享夹具重构后SDK8/HTTP11、两包普通34/30及
Clippy也重验通过，普通新增跨语言1ignored已严格实际执行；日志neon-dsg-go-native-*。
Go带标签lint0issues，普通183事件沿用未改生产代码的上批证据，不重复计入新用例。
内核流水线70614、最终SQL流水线31469均终态退出0，不重复启动旧句柄。

原生门禁未覆盖未启用的ICU/readline、额外DNS/encryption/xid_wraparound及
全部TLS/GSS配置等；部分可在Mac另配，不能统称Linux限制。完整真实Pageserver/
Safekeeper/RW/RO/branch本机端到端仍未执行，SLRU协议fixture不构成该证据。
没有远程Linux CI执行证据，也没有VM物理回收/迁移/性能认证。

## 继续实施的完整范围

1. Monitor的实际运行时接入、外层持久
   协调循环与可信唯一执行者、跨owner/进程重连/
   不确定结果对账；不能把旧Ok或当前PG状态当新请求结果/缩容许可。
2. R02全部私有/查询/并行/DSM/维护/扩展/后台预算、硬约束、quiesce和资源承诺
   生命周期；RSS采样、填写预算ID或新boot门禁不能代替完整运行时预算。
3. Compute/Monitor/Agent/Scheduler/Controller/Runner实际fencing、恢复对账、最终
   QMP/VM串行执行；Provider/Guest/Host各自回执、平台探测/HugeTLB等缺失业务。
4. PG16/15/14逐版维护、语义移植和实际运行验证；PG18发行/AIO/SLRU适配按范围
   落实，不能用旧头语法检查当成功能完成。
5. control-plane Driver与Helm分支系列代码及本机验证；真实Neon存储本机端到端，
   Mac可配置的其余测试矩阵。环境/性能证据按实际执行更新。

以上是代码/集成剩余项，不能改称“只待Linux测试”。最终Linux/KVM/NeonVM/
HugeTLB/真实物理回收、迁移、K8s和目标性能/长稳由后续目标环境验收。
恢复入口为[执行恢复记录](DSGA-执行恢复记录.md)。旧stash 6f002a3…和f5c4dfc…均
已各apply一次，禁止再apply；反向源码已恢复，不遗留运行中的故障版本。
