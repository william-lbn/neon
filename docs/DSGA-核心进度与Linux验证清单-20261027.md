# 内存伸缩核心进度、Mac证据与Linux待验清单

当前剩余量与工期以[PG17剩余功能评估](PG17-剩余功能与工期评估-20261029.md)为准：按所有者最新范围收敛为P1～P4四组，粗估40～80有效小时；只验证自身修改及受影响链路。下文各阶段旧记录保留历史，旧缺口已被后续实现覆盖时不重复计数。PG17完成后暂停，不自动进入PG18/16。

更新：2026-10-30（Asia/Shanghai）。本清单不按提交数量计算完成率。PG17核心
仍有明确代码工作，不能封完整版本或声称生产可用。所有者确认长期顺序：
**先完成PG17全部核心实现、Mac可执行验证及远程归档，然后暂停**；PG18→PG16
留待所有者恢复，PG14/15与独立控制面边界不变。2026-10-26上游冻结继续生效，不重新追新，不重复未变功能。




## 最新归档：完整实际预算、服务重开与永久来源生命周期

AS `d7946ec6175d2905d081c76685fc51726432190e` 已按预算合同、SDK、服务恢复三实现模块及缩容验证模块提交并推自有既有功能分支，核完整远程SHA。原Prepare/Host完整的16/17以v4明确区分原PG未派发与已派发独立事实，按当前Root及实际VM规格/Outside/usable/Host峰值保存完整预算。新开放用实际容量代次，完成CAS同时安装v3实际永久头、原Unknown覆盖/独立Outcome和预算水位，后继Host/Scheduler/GC通过统一严格来源读取，不伪造Cleanup或目标成功。

采用Mac定向race52顶层/278含父子记录0fail/skip，七完整恢复窗口含一新增真实Lease接管、原生终态缓存淘汰/SDK重建、旧关闭后PG迟到下一代、InitialVM、Complete丢回复及Host/正常后继；额外新永久头后继读取及资格同名换UID拒绝。原单域受影响四根保留六次真实选主接管；原生SDK准确PG17/Router/JWT/SQL/SharedCost 1 Rust根/3 Go race进程9.88秒通过，8旧根过滤，非PG预算域为明确夹具。12包vet、普通/native tag增量lint0，33消费者仅编译；GC overlay反向失败/原源码复验通过。准确源码/用例/失败分别记录在[验收](DSGA-完整实际预算与服务恢复-Mac验收-20261030.md)与[机器收据](DSGA-完整实际预算与服务恢复-Mac收据-20261030.json)。

本批PG/SQL/Rust产品未改，PG17实际/gitlink/两锁仍79367196；仅目标Rust夹具为SharedCost升级临时Neon1.11，不改变正式成员认证。P1早期16缺完整Prepare/本请求Host、类别18及后继永久预算剩余接线/组合仍需代码；P2安全降normal/持续预算、P3直接接线、P4最终准确PG17组合继续，不能归为仅待Linux。PG17整体未完成，Goal active；全面完成远程归档后暂停，不进入18/16。以下为历史归档。

## 最新归档：VM在途独立对账与关闭后Runner最终写入隔离

AS `aea5507f433b236cea9bed2866a97f7b659054d9` 已推自有既有功能分支并核完整远程SHA，发行锁准确引用。关闭后来源恢复不得新Dispatch/消费旧not_sent重放；Runner仍可在原flock观察may_have_sent或封存明确未发送选择。独立选择/准确NativeCapacity/取消见证UID/RV保存，原Active Unknown、Abort、预算与关闭不改，接管须原Runner持久确认新floor。

Mac采用定向race按package/test去重22顶层/122含父子记录，0fail/skip；新增VM五模式、六个真实Elector接管窗口、Grow/Shrink模型、双处CAS四窗口及受影响Guest/PG/Runner取消路径通过。四包vet、最终增量lint0、33消费者仅编译；overlay禁用闭门gate确定失败，原源码未改且同项再通过。原夹具/断言和lint失败分别保留，采用子树与替换子根在[机器收据](DSGA-VM在途独立对账-Mac收据-20261030.json)精确固定。详见[阶段验收](DSGA-VM在途独立对账-Mac验收-20261030.md)。PG/SQL/Rust产品未改，准确79367196原生证据复用，不重跑未变矩阵。

Guest/PG/VM单领域证明已有，P1完整实际预算/Host结清、实际容量代次重开、类别18永久预算消费者及后继请求仍需代码；P2安全降normal/持续预算、P3直接接线、P4最终准确PG17组合继续。只验自身修改及直接受影响路径，独立审查/验证并行，源码变更及最终依赖验收保持顺序。PG17整体未完成，Goal active；全面完成并远程归档后暂停，不进18/16。以下为历史归档。

## 最新归档：PG在途与历史淘汰独立对账，P1继续完整资源恢复

AS `5767124c3aca9a04a5d39b9d8fece2eccdb4bbfc` 已推自有既有功能分支并核完整远程SHA，发行锁准确引用。完整原生终态与严格更高floor隔离分别认证：FENCED串行权/稳定实际容量/完整原Key，独立证明UID/RV保存后再Fence/查询才清同一SDK Pending。原Active Unknown/原回执、Abort、预算和关闭不改；空Pending仍核当前门禁。原关闭后晚到提交另存实际容量，不改原关闭expected代次，也不冒充旧预算覆盖了当前资源。

Mac定向race去重16顶层/97含父子记录0fail/skip；五正式生产组合150.09秒、三次真实失租接管、Grow/Shrink模型、CAS四窗口及受影响旧Guest/PG拒绝/关闭/接续通过。准确PG17严格Go TLS→Router/JWT→SQL→PG门禁1外层/3独立Go进程11.87秒通过，包含实际历史淘汰和迟到旧POST Stale。删除高floor检查确定性失败，源码逐字恢复后通过。五包vet、新缩容根vet、普通及native tag最终lint0；33消费者仅编译，Rust目标clippy退出0且27条既有依赖warning保留。详见[阶段验收](DSGA-PG在途与历史淘汰独立对账-Mac验收-20261030.md)与[机器收据](DSGA-PG在途与历史淘汰独立对账-Mac收据-20261030.json)。PG/SQL/Rust产品实现未改，只新增Neon原生测试，PG17实际/gitlink/两锁仍79367196。

Guest和PG单领域证明均已形成，但VM在途、完整实际预算/Host结清、关闭代次与重开、类别18永久预算消费者及后继业务仍需代码，不能归为仅待Linux。P2～P4继续，以[当前清单](PG17-剩余功能与工期评估-20261029.md)为准。PG17整体未完成，Goal active；全面完成并远程归档后才暂停，不进入18/16。冻结/独立控制面/无镜像部署合main边界不变，所有者AS AGENTS七行修改不stage。以下为历史归档。

## 最新归档：在途 Guest 独立对账，完整 P1 继续

AS `64282b38aa6442ec19974750a70793570c7c6e35` 已推自有既有功能分支并核完整远程 SHA，发行锁准确引用。类别16/17先以UID/RV CAS固定原Guest/关闭/操作摘要及取消选择；原命令已接收时只读核实际Applied，从未收到时由Root文件锁内封存取消屏障。原Active Unknown、Abort、旧预算和关闭不改；普通轮询只查已保存选择，不重发Apply或自动取消。完整Guest证明仍不开放、不结清PG/VM/Host。

Mac定向race去重14顶层/98含父子记录0fail/skip；六生产故障、两真实失租接管（87.91秒）、选择/Record两处CAS四窗口、深拷贝、类别17缩容后Model/codec及直接受影响旧关闭/取消通过。四包vet、最终增量lint0、33消费者仅编译；原监听/PATH/cache/lint及缩容夹具失败保留。详见[阶段验收](DSGA-在途Guest独立对账-Mac验收-20261030.md)与[机器收据](DSGA-在途Guest独立对账-Mac收据-20261030.json)。PG/SQL/Rust/原生HTTP未改，PG17实际/gitlink/两锁仍79367196，未重复未变原生矩阵。

本批是Guest单领域闭环；16/17的PG/VM在途、原PGUnknown/结果淘汰、独立完整实际预算/重开仍需业务代码，不能归入仅待Linux。P2～P4继续，以[当前清单](PG17-剩余功能与工期评估-20261029.md)为准。PG17整体未完成，Goal active；全面完成并远程归档后才暂停，不进入18/16。Linux物理/性能另验，冻结/独立控制面/无镜像部署合main边界不变；所有者AS AGENTS七行修改保持不stage。以下为历史归档。

## 最新归档：再关闭后的同容量业务重试及自动采样阻塞修复

AS `416c06264c5002d9ef1e0d77001e865de0c9967f` 已推自有既有功能分支并核完整远程SHA，发行锁准确引用。最近完整再关闭摘要经全来源审计、同容量/预算及有效期限核验后，UID/RV写前选择新开放；保留完整直接前驱和首次未知，不递归累积历史。逐字相同账本的完整解码单项复用消除串行采样阻塞；每次GET、VM/Lease/来源检查及原5秒新鲜度不变。

Mac race按package/test去重164顶层、785含父子记录0fail/skip（含Fuzz种子，非持续fuzz），journal/API完整回归与六根owner认证取消通过；六个恢复窗口完整根通过，恢复后自动Grow→Shrink 88.01秒正常Finish/Host结清，QMP peer最终1GiB且扩缩各一次。三包vet、增量lint0、33消费者仅编译通过，原失败保留。详见[阶段验收](DSGA-再关闭后业务重试-Mac验收-20261029.md)与[机器收据](DSGA-再关闭后业务重试-Mac收据-20261029.json)。PG17实际/gitlink/两锁保持79367196，PG SQL/Rust/原生HTTP不变。

R1仍缺过期后再次期限恢复；活动关闭13～18、持续预算、降normal、分区跨boot、Monitor/WSS/配额/RW-RO/可信CPU/全部写者仍须代码，Linux真实物理返还及性能另验。PG17整体未完成、Goal active；全面完成并远程归档后才暂停，不进入18/16。当前剩余量以[评估清单](PG17-剩余功能与工期评估-20261029.md)为准，旧阶段缺口不重复累加。所有者AS AGENTS七行修改保留不stage。

## 最新验证：原生过期受限管理与本地取消恢复完成 Mac 阶段

AS `293fbe57307195e887197ffbe9ba1de814d07b9a` 已推原自有功能分支并核完整远程 SHA，发行锁准确采用。PG17 实际/gitlink/两份锁仍为 `33c92fe33e741309f9e8ab23e3767fb6f2210216`。观察原生过期后保留原 Run/Session 串行管理，允许状态、原 Root 有界修复与关闭恢复，拒绝普通业务/自动策略/原开放重放，不续租或自动 rearm。管理排队跨过期在新 WAL 前核验；本地取消 Finish 丢回复后完整历史零 PG 读回，新 Begin 仍先强制协商，失败不写新意图。

Mac race 去重 13 顶层/64 含父子记录 0 fail/skip；完整自动 Grow→Shrink 167.24 秒正常 Finish/Host 结清，QMP peer 读回 1 GiB。最终 vet/增量 lint0、33 消费者仅编译通过；原启动退出、本地取消多取凭据和 lint 失败日志保留。详见[阶段验收](DSGA-原生过期受限管理-Mac验收-20261029.md)与[机器收据](DSGA-原生过期受限管理-Mac收据-20261029.json)。

这只是受控重开的必要前置，持久 rearm/重新开放及重复生命周期仍须代码，其余 normal/私有后台/分区/跨 boot/Monitor/CPU 等核心清单不变。PG17 整体未完成，Goal active；PG17 全面实现、Mac 验证及远程归档后才暂停，不进入 18/16。所有者 AGENTS 改动保留不 stage。



## 最新验证：Runner 无 Wait 的整 Pod 终态结清完成 Mac 阶段

AS `d78216bb92299e56a227c97701076ef1422f9a9b` 已推原自有功能分支并核完整远程 SHA，发行锁准确采用。PG17 实际/gitlink/两份锁仍为 `33c92fe33e741309f9e8ab23e3767fb6f2210216`。本批独立整 Pod 终态来源接入生产 Scheduler、活动 Reader/Controller 与完成后 GC；只在原 Wait 真实 404、原 Pod/VM/Node/Birth 和全容器终态固定时结清，不制造 Wait，不清原 resize 未知。来源选择保存后固定 UID/摘要，迟到 Wait 不替换，来源丢失/换 UID 保留未知。

Mac -race 去重 113 顶层/429 含父子记录，0 fail/skip（含 2 个隔离 helper）；真实 Launcher 在 Start 后 SIGKILL 且 Wait 不存在，完整终态仍要求 Kube 夹具；正式自动 Grow→Shrink 169.30 秒通过并 Finish/Host 结清、QMP peer 读回 1 GiB。三包 vet、最终增量 lint0、33 消费者仅编译通过。详见[阶段验收](DSGA-无Wait整Pod终态结清-Mac验收-20261029.md)及[机器收据](DSGA-无Wait整Pod终态结清-Mac收据-20261029.json)。

本批 PG/Rust 未改；真实 Linux CRI/cgroup/物理返还及性能另验。受控重新开放/重复生命周期、normal 安全降限、私有后台持续预算、分区/跨 boot、Monitor/WSS/配额/RW-RO/可信 CPU/全部写者仍须代码，PG17 整体未完成、Goal active。另一聊天已关闭；所有者 AGENTS 修改保留不 stage。PG17 全面实现、Mac 验证及远程归档后才暂停，不进入 18/16。本段为最新状态，下文既有缺口清单保留历史。



## 最新验证：原生准入失联关闭与生产续租已完成 Mac 阶段

AS `80bc0309a7c64771d520bab70da90519cc823e39` 已推原自有功能分支并核完整远程SHA；本仓PG17实际/gitlink/两份版本锁准确采用 `33c92fe33e741309f9e8ab23e3767fb6f2210216`，AS发行锁采用同一已验证依赖。


PG17 `33c92fe33e741309f9e8ab23e3767fb6f2210216` 已推自有功能分支；受管PG首次Fence启动单调期限，后台独立过期关闭。正式SQL/Rust/认证Router/Go和生产Run完成期限协商与原deadline CAS；每期限三分之一完整核验续租，坏缺WAL/失租不续、过期不自动rearm或开放。

Mac race按package/test去重60顶层203含父子记录0fail/skip；完整自动Grow→Shrink 174.10秒通过并正常Finish/Host结清，QMP peer读回1GiB，扩缩各一次。候选真实PG受影响37项通过，准确SHA重建后9项原生、1项正式Router→SQL→PG、1项认证范围与重复字段、1项Rust合同通过；vet/增量lint/目标Clippy0，32消费者仅编译，Neon默认CI228项13pass/215环境skip。原SIGQUIT、旧lint二进制/冻结依赖Clippy及早期候选失败均保留。

详见[阶段验收](DSGA-原生准入失联关闭-Mac验收-20261029.md)与[机器收据](DSGA-原生准入失联关闭-Mac收据-20261029.json)。续租不清预算/未知、不授予物理/VM许可；受控重新开放/重复生命周期、normal安全降限、私有后台持续预算、Runner已launch无Wait、分区/跨boot及Monitor/WSS/配额/RW-RO/可信CPU/全部写者仍须代码，Linux环境验收单列。整体未完成，Goal active；PG17全面完成/远程归档后才暂停，不进入18/16。另一并发聊天已停止归档，所有者AS AGENTS修改保留不stage。

## 最新验证：无失败屏障的Runner出生崩溃关闭已完成Mac阶段

AS `5cbb6c4d0db94c821a44dd66a074fa565d7ed5ed` 已推自有功能分支并核完整远程SHA，本仓发行锁准确引用。

完整原出生选择先原子fsync，State/Binding真实UID分步持久；五个真实子进程
直接退出越过defer后，只补原NoStart关闭，不返回可启动Runner。14种故障、
真实Start/恢复竞争及旧完整失败屏障兼容通过；未知launch/坏WAL/UID替换拒绝。
最终Runner全包及受影响Scheduler/首次服务→Grow组合-race53顶层217含父子
记录0fail/skip（含1个helper），不是217次独立E2E；vet/增量lint0，31消费者
仅编译，两架构ELF及LinuxRunner入口仅编译。原超时/基线缺陷/文案断言/lint/
Mac入口vet失败全部保留。详见[阶段验收](DSGA-无屏障出生崩溃关闭-Mac验收-20261029.md)
及[机器收据](DSGA-无屏障出生崩溃关闭-Mac收据-20261029.json)。

PG17实际/gitlink/两锁仍0d2be6c8，PG/Rust未改；已有launch无Wait、坏缺外层
WAL关闭/受控重开、normal安全降限及私有后台持续预算等仍需代码，Linux平台
验收另列，整体未完成、Goal active。另一并发聊天及其自动接续已按所有者要求
停止并归档；本聊天继续，PG17全面完成/远程归档后才暂停，不进入18/16。

## 最新验证：原创建进程地址空间约束已完成Mac实施阶段

PG17 `0d2be6c8ec965e47abc873bab17806e3f6b9197f`及AS代码
`34998ae9aff418bd76eef44bade40590d4b09d1e`已推原自有功能分支并核完整远程SHA；
AS后续同分支仅文档归档提交由发行锁准确引用。PG实际源码/gitlink/两锁统一采用
新SHA，准确私有安装/运行中版本和配套rmgr/Neon扩展已核验。原创建AS要求有限N，
出生/各原资源命令/Prepare/Quiesce/Guest/VM逐字冻结，Root核真硬限/VA/NNP/无能力/
UID及PID换代；集合重排允许，退出/复用有界重读，不完整在新WAL/写前拒绝。

Mac -race按package/test去重135顶层549父子记录、0fail/skip，新AS自身9顶层68
记录；含正式AS自动Grow→Shrink全控制链、取消/未知恢复/零写前和持久删字段拒绝。
真实PG3项、来源C控制流1项8替身模式、准确Neon扩展2项通过；旧真实读者4模式
拒绝新增字段。六包vet/Mac及Linux Guard lint0，共享22消费者仅编译；双Linux
8个ELF仅编译未执行。Neon CI222项13pass/209环境skip，index18/4/39准确绑定。
首轮Perl重配失败误更新旧325a私有安装，已标记不可复用；新目录exact完整成功，
原失败/lint/替身尝试保留。详情见[阶段验收](DSGA-原创建进程地址空间硬限-Mac验收-20261029.md)
和[机器收据](DSGA-原创建进程地址空间硬限-Mac收据-20261029.json)。

AS含共享VA/占位，不是私有RSS；摘要绑定但原字节账本不加减，不授权降normal/缩VM。
可用私有/后台持续预算与活性、安全降normal、坏缺WAL关闭/受控重开与重复生命周期、
出生无屏障/Runner无Wait/分区/跨boot迁移、Monitor/WSS/配额/RW-RO/可信CPU及全部
写者仍须代码。高fork持续活性尚未证明；Linux真实限制/继承/启动余量、cgroup/
KVM/NeonVM/HugeTLB/物理/性能另验，不能把代码缺口转为平台后验。整体未完成、
Goal active，PG17全面完成/远程归档后才暂停，不进入18/16，不追新上游/独立
控制面、不建镜像/部署/合main；所有者AS AGENTS修改未stage。

## 最新验证：Cleanup恢复准入异常关闭已完成Mac阶段

AS `b4a31b06aba08859a57b7cbd9c4c7836efa2c675`已推既有自有功能分支并核完整远程
SHA，本仓发行锁准确采用。实际容量/完整目标/旧容量Cleanup未知或完成后，复核
原生产预算来源，异常先原生fence、固定容量/revision、UID/RV写前CAS再独立关闭。
旧容量继续引用旧预算，原Cleanup只读也关闭已有服务；原Active/未知/历史/永久头
保留，不Finish、不释放、不反向写。来源恢复与SDK重建不重开，公开恢复及最终
Executor均拒绝已有关闭。补齐旧v2无永久头实际容量Cleanup安全入口；格式18仅
允许其完整原Cleanup专属关闭，不补造预算，编号不是中间产品或PG18交付。

Mac -race采用57顶层386含父子记录：Journal321、Admission32、生产组合33，
采用0fail/skip；Journal新5顶层49记录，生产新8完整目标模式及实际/旧容量各4
模式，正常取消受影响14模式均有成功证据。三个原失败批完整保留，准确采用
成功子树及定向替换，不能称原整包通过。三包vet及工作区增量lint退出0，消费者
19包pass/14无测试文件仅编译，全部测试进程终态。详情及日志/源码SHA见
[阶段验收](DSGA-Cleanup恢复准入异常关闭-Mac验收-20261029.md)和
[机器收据](DSGA-Cleanup恢复准入异常关闭-Mac收据-20261029.json)。

PG17实际源码/gitlink/两锁仍325a；本批PG/Rust未改，不重复未变原生矩阵。
坏缺WAL关闭、受控重开与有界重复生命周期、安全降normal/私有后台持续上界及
活性、出生崩溃无屏障/无Wait/分区/跨boot迁移、Monitor/WSS/总配额/RW-RO/
可信CPU及全部写者仍须代码，不归Linux后验。Go中PG/Root/QMP/Kube为明确peer；
Linux/K8s/KVM/NeonVM/HugeTLB/物理/性能另验。整体未完成、Goal active，PG17
全面实现/Mac验证/远程归档后才暂停，不进入18/16、不追新上游/独立控制面、不建
镜像/部署/合main；所有者AS AGENTS修改不stage。

## 最新验证：PG已派发与跨容量安全关闭已完成Mac阶段

AS业务`5614ce2d`及归档`7a333101c6fff6240d54ca06604c5f9f7da4b6cb`已推既有
自有功能分支并核完整远程SHA，本仓发行锁准确采用。PG已派发/完成而来源失效时，
先原生fence、UID/RV写前确认再独立关闭，固定完整Active/原PG命令/预算/未知。
只允许原Resize前后两代；关闭撞上PG提交后，同owner以严格更高容量和revision
有界接续，原始/直接前驱/当前选择保留。来源恢复不重开，不将关闭作结清许可。

共享Go/Rust包装2准确标识跨容量，包装1原同容量/更高owner语义保持，子请求仍1；
Journal17进一步限制原业务两代。编号只识别持久格式，产品目标是完整PG17自动
扩缩容。Mac -race采用44顶层310条去重父子记录、0fail/skip，含七恢复模式、旧
HTTP Store和正常自动Grow→Shrink/生产结案丢回复；Rust合同5/准确PG17原生3/
真实Router准入4项通过、0ignored。37共享向量一致、11摘要由真实PG独立核验；
四包vet/增量lint及Clippy退出0，共享消费者22包pass/12无测试文件仅编译。
准确证据/失败历史见[阶段验收](DSGA-PG在途跨容量关闭-Mac验收-20261029.md)
及[机器收据](DSGA-PG在途跨容量关闭-Mac收据-20261029.json)。

PG17实际源码/gitlink/两锁325a保持。Cleanup恢复开放异常、坏缺日志关闭/受控
重开与有界重复生命周期、normal/后台私有持续上界及活性、出生崩溃/无Wait/分区
跨boot迁移、Monitor/WSS/配额/RW-RO/可信CPU与全部写者仍需代码，不能列Linux
后验。Go中PG/Root/QMP/Kube为明确peer；Linux/K8s/KVM/NeonVM/HugeTLB/物理/
性能另验。整体未完成、Goal active，PG17全面实现/Mac验证/远程归档后才暂停，
不进入18/16、不追新上游/独立控制面、不建镜像/部署/合main。

## 最新验证：PG尚未派发的资源在途异常关闭已完成Mac阶段

AS `3f8528c2427cd470c6ec657044e8fda081eec130`已推既有自有功能分支并核完整远程SHA，本仓发行锁准确采用。
Prepare/Host/VM/Guest在途、PG仍Planned时，正式PGWorker区分合法原命令未知与
来源失效，认证原Root/出生/资格/Runner/永久头及Prepare/Host来源。异常先fence、
UID/RV写前CAS再独立关闭；Journal v16固定完整Active、原步骤/预算/历史，未知
Quiesce逐字保存原Close并封住迟到写，丢回复/SDK重建/高owner接续不丢原选择。
ReadActiveHeld只读认证原永久头及Start前保存的关闭链，解决取消实际预算进入
后继Active被误关的问题；旧ReadCurrent全局结清条件保持，原来源保护丢失仍拒绝。

Mac race采用32顶层/180含父子记录通过，0fail/skip，新增6顶层39记录。包含
Journal25顶层147记录、九个新生产恢复模式、两种Guest真实失租接管/取消结清/
后继Prepare及原Host保护拒绝、完整自动Grow→Shrink生产Finish/Host结清和QMP
peer实际值1GiB读回；三包vet/最终增量lint0，共享消费者仅编译。原HEAD overlay、
静态Lease/组合期限/非法初始预算及原永久头读者误用的真实失败保留，准确采用
见[阶段验收](DSGA-资源在途异常关闭-Mac验收-20261029.md)及
[机器收据](DSGA-资源在途异常关闭-Mac收据-20261029.json)。不是完整产品E2E。

PG已派发/跨容量与Cleanup、坏缺日志关闭、受控重开/重复生命周期、安全降normal/
私有后台持续边界、出生无屏障/Runner无Wait/分区跨boot、Monitor/WSS/配额/RW-RO/
可信CPU及全部写者仍须代码，不能转列Linux后验。PG17三锁/实际源码325a及冻结
上游保持；未改PG/Rust、不重复未变原生大矩阵，不建镜像/部署/合main。Linux/
真实Kube/KVM/NeonVM/HugeTLB/物理/性能另验。Goal active，PG17全面代码/Mac/
远程归档后才暂停自动接续，不进入PG18/16，不追新上游或独立控制面。

## 最新验证：原Runner出生失败关闭与整Pod结清已完成Mac阶段

AS `4bbcdba6fc668ee571591bc093961136da1244a3`已推既有自有功能分支并核完整远程SHA，本仓发行锁准确采用。
Initialize已fsync nonce但State/Binding创建或读回失败时，先持原flock永久封存，
同nonce补齐仅供关闭的原来源也始终返回nil/error。已确认State UID独立fsync，
后续失败不得同名换UID；原WAL保留，迟到Tick/Start拒绝。Scheduler仍要求完整
Pod终态与原Node CAS，sidecar运行保留成本，丢回复保留保护并重读结清。

Mac Runner全包-race41顶层177含父子记录通过；Scheduler首轮43pass/1fail，
旧v8未知断言改为v9后两项定向通过，成功子树去重17顶层44记录。合计去重
58顶层221记录，新增4顶层14记录；不是221次产品E2E。两包vet/最终增量lint0，
Mac共享消费者仅编译、Runner双Linux架构仅交叉编译exit0。首轮和基线真实失败
完整保留，准确采用与平台边界见[阶段验收](DSGA-出生失败关闭-Mac验收-20261029.md)
及[机器收据](DSGA-出生失败关闭-Mac收据-20261029.json)。PG17三处仍准确325a，
未改PG/Rust、不重复未变原生大矩阵；Linux/QEMU/KVM/物理/性能未执行。

仅同一次Initialize尚存活且可封存的出生失败窗口完成。崩溃无失败屏障、Runner
无Wait/分区/跨boot迁移、更早资源在途及坏缺日志独立关闭、受控重开/有界重复
生命周期、normal/私有后台持续上界与活性、Monitor/WSS/配额/RW-RO及可信CPU/
全部最终写者仍须代码，不能转列Linux后验。Goal active，PG17全面完成/Mac验证/
远程归档后才暂停，不进入18/16、不追新上游/独立控制面、不建镜像/部署/合main。

## 最新验证：资源完成而最终结算未确认的异常关闭已完成Mac阶段

AS `a29c79f825732352e4aac04d2974d28feb5a016b`已推当前自有功能分支并核完整远程SHA，本仓发行锁准确采用。全部资源前驱Completed、Finalize尚Planned/Unconfirmed、Resume仍空Planned时，复用正式结算来源审计，异常后原生fence、同boot/原目标容量核验及UID/RV写前CAS执行独立关闭；同轮覆盖Start后来源漂移。Journal v15固定完整Active，关闭仅引用原目标承诺，不生成成功预算，旧永久头、Finalize/Resize未知和空Resume均保留。来源恢复、SDK重建及更高owner接续保持关闭。

Mac -race Journal 25顶层159含父子记录通过，新增v15占4顶层22记录；五生产恢复模式按子树去重1顶层6记录、v14受影响1顶层2记录、完整正常结案含两项嵌套检查1顶层3记录、原预算及旧v2完整30子模式/1顶层31记录均通过，0fail/skip。三包vet、最终增量lint0，消费者18包pass/6包无测试文件仅编译。原代码overlay、首次正常链、旧预算矩阵和lint失败分别保留；修夹具与测试总期限后采用完整通过矩阵，生产期限和校验未放宽。详见[阶段验收](DSGA-结算未确认异常关闭-Mac验收-20261029.md)、[机器收据](DSGA-结算未确认异常关闭-Mac收据-20261029.json)及本仓版本核验。

PG17三处仍准确325a；未改PG/Rust，不新增原生/Linux执行、ELF编译、镜像或部署；未变Neon CI逻辑按范围复用218项13通过205环境skip，准确index版本另验。更早资源在途、坏缺日志独立关闭、受控重开/有界重复生命周期、normal/私有后台持续上界与活性、其他出生/无Wait/分区/跨boot迁移、Monitor/WSS/配额/RW-RO及可信CPU/全部写者仍须代码。Linux/K8s/KVM/NeonVM/HugeTLB/物理/性能另验；不能将代码缺口列为平台后验。整体未完成，Goal active，所有者AGENTS原改动不stage；PG17全面实现、Mac验证及远程归档后才暂停接续，不进入PG18/16，不追新上游或独立控制面、不合main。

## 最新验证：最终开放尚未发送的异常关闭已完成Mac阶段

AS `3c4e12b9ea695a31e0894ea908be63d5c609b4fa`已推当前自有功能分支并核完整远程SHA，本仓发行锁准确采用。完整Finalize及资源前驱Completed后、Resume仍为空Planned时，先前准入也可能继续开放；现复核完整生产来源，异常后先原生fence，再UID/RV写前CAS执行独立真实关闭。Journal v14以完整Active摘要保留未发送步骤及预算，不补造原开放、Finish、释放或缩VM。来源恢复/SDK重建/更高owner接续均保持原关闭。

Mac -race Journal 21顶层137记录通过，新v14占4顶层22记录；四生产恢复模式按子树去重1顶层5记录通过，含两种丢回复及真实Lease失效。已发送开放两模式、完整正常结案含嵌套检查通过；三包vet/增量lint0，消费者仅编译。原代码overlay真实失败、沙箱监听失败和强化接管测试误用旧Root worker的失败均保留，修测试后只复测接管通过。精确采用、摘要和环境边界见[阶段验收](DSGA-未发送最终开放异常关闭-Mac验收-20261029.md)及[机器收据](DSGA-未发送最终开放异常关闭-Mac收据-20261029.json)。PG17 gitlink/两份锁仍准确325a，冻结祖先检查通过；Neon CI 218项中13通过/205环境skip，不把skip记为业务通过。

这只关闭最终Planned稳定边界的代码缺口。Finalize前其他在途、坏缺日志关闭、受控重开/重复生命周期、normal/私有后台持续上界与活性、其他出生/无Wait/分区/跨boot迁移、Monitor/WSS/配额/RW-RO及可信CPU/全部写者仍须代码。Linux/K8s/KVM/NeonVM/HugeTLB/物理与性能后验，本批PG/Rust未改且没有新Linux执行或ELF编译。整体仍未完成，Goal active；PG17全面实现、Mac验证及远程归档后才暂停，不自动进入18/16，不改独立控制面、不建镜像/部署/合main。


## 最新验证：最终开放异常关闭已通过Mac阶段（2026-10-29）

新增v13保护完整Finalize已永久持有、前驱均Completed且最终Resume已Start的
终段。原Active完整SHA固定全部命令/回执/未知，关闭WAL和完整原生proof独立；
不普通Record/取消/Finish原操作、不释放预算、不缩VM。来源在setter后审计前
漂移也同轮关闭；确认后SDK重建不恢复已被覆盖的旧开放pending，原未知保留。

新模型按用例去重4顶层24记录、旧v10/11/v12模型13顶层91记录、生产9模式
分批-race通过；真实Session失租接续/SDK重建/CAS丢回复及来源恢复不自动重开
已验。最终正常结案1顶层3记录、受影响首次2模式通过；vet/增量lint0、消费者
仅编译exit0。原正常链3种永久故障移至各自关闭生命周期，没有删除覆盖。
首轮Store沙箱监听失败和旧基线真实失败保留，不能整体涂绿或用过滤冒充全量。
准确源码/日志/采用及边界见[阶段验收](DSGA-最终开放异常关闭-Mac验收-20261029.md)
和[机器收据](DSGA-最终开放异常关闭-Mac收据-20261029.json)。所有测试句柄已终态。

本批PG/Rust业务未改，不重编准确325a；PG gitlink及两份PG锁不变。AS中文
提交59622bbd3aa8ee0a559ed9c91967db4a2ce778e9已推既有自有分支并核完整远程SHA；
本仓AS发行锁准确采用该依赖，中文文档随后提交推核。所有者AS
AGENTS原改动不stage、旧stash不动。仍缺Resume尚Planned/其他在途及坏缺日志
关闭、受控重开/重复生命周期、normal/持续约束、其他出生/无Wait/跨boot迁移、
Monitor/CPU等核心代码，不归Linux待验。Goal active；PG17全面完成后才暂停，
本轮不进入18/16、不改独立控制面、不建镜像/部署/合main。

## 最新验证：首次开放未知异常关闭已通过Mac阶段

PG17自有修复`325aeb42a6080d86902d51892b2518a70ec2512b`在冻结b078之上，只允许
关闭跨过未投递revision，开放仍精确next；官方冻结点/清单不变，没有追新。
准确325a干净源码已编译安装，Neon扩展安装通过，完整二进制SHA匹配。新增SQL
与Rust原生SDK在原b078失败、325a通过；新SQL及旧准入4项、Rust新用例1项、
旧接续1项、SDK2项、认证HTTP3项均通过，0skip/ignored。没有重复完整未变PG矩阵。

AS v12独立原首次开放/关闭WAL及完整proof、生产首次安全检查和SDK重建通过。
原InitialHeld/预算/开放Unconfirmed保留；关闭不证明旧工作未执行或可释放/缩VM。
新Journal按用例去重4顶层28记录、受影响v10/11九顶层63记录通过；生产8个模式
分两次-race通过，含原开放未送达/已应用、CAS/setter丢回复、资格丢失两窗口、
真实Session失租接续与setter后来源漂移。正常首次→Grow和旧空闲Root受影响
组合也通过；vet/最终增量lint0，DSGA/Agent/Controller消费者仅编译退出0。

日志及源码摘要、首轮失败、准确采用及环境边界见[本阶段验收](DSGA-首次未知异常关闭-Mac验收-20261029.md)
和[机器收据](DSGA-首次未知异常关闭-Mac收据-20261029.json)。Go组合PG/Root/Kube/
平台仍是peer；原生PG链独立验证，新管理HTTP/Submit入站本批未新增E2E。
验证句柄59913/60160均终态0，进程核对无相关残留，不重复已通过未变测试。

本阶段PG17 `325aeb42a6080d86902d51892b2518a70ec2512b`及AS
`d0a38da5b4d0513d4c28913517195795aa68e1b0`已推既有自有功能分支并核完整远程SHA；
本仓gitlink、两份PG锁及AS发行锁统一采用准确依赖，随后中文提交推核。所有者
AS AGENTS原改动保留不stage。PG17仍缺在途/坏缺日志异常关闭、受控重开与重复生命周期、normal/
持续约束、其他出生/无Wait/跨boot迁移、Monitor/CPU等核心代码，不能转列Linux
待验。Goal active；PG17全面代码/Mac验证/远程归档完成后才暂停，不进入18/16。

## 最新验证：关闭接续持久WAL与PGWorker/管理处理器已通过Mac阶段

原Goal恢复active；原neon heartbeat已通过应用工具更新为PG17全面完成后暂停，
不进入PG18/PG16，未创建重复任务。此前paused及审批失败段为历史，不覆盖本段。

本地v11独立Continuation、完整三份选择恢复、显式更高owner接续与原选择重放已
验证。新组合最终-race1顶层/3记录通过，0fail/skip，115.75秒；真实Session失租/
重复接管、写前CAS/PG前后/Record丢回复、SDK重建与旧Session零派发。PG/Root/
Kube/平台为显式peer，executeControl为正式处理器但未走新管理HTTP/Submit入站。
原Journal9顶层63记录、协议3顶层21记录采用不重跑；首轮受影响旧v10/实例绑定
两顶层3记录通过单列，首轮整条命令因新测试只接受LeadershipLost而实际先返回
context canceled失败，修新测试断言后两新分支通过。三包vet及最终owner vet、
增量lint0、共享消费者只编译exit0。详细源码/日志/未执行见[阶段验收](DSGA-关闭接续持久WAL-Mac验收-20261029.md)
及[机器收据](DSGA-关闭接续持久WAL-Mac收据-20261029.json)。

AS中文模块提交`d12f533bb2e35ad933a178a6d1186234c0bbad12`已推既有自有功能分支并核完整
远程SHA；本仓发行锁采用同一准确依赖。验证进程已终态，PG17 b078冻结、独立
控制面及环境边界不变；候选记录保留历史，本仓随后提交推核，远程以实际Git为准。在途/首次开放未知异常、坏缺日志关闭/受控重开、normal/持续约束、
生命周期与Monitor/CPU仍有代码工作；不是PG17全面完成，不将其归为Linux待验。

## 本次心跳读取到Goal已暂停（2026-10-29）

get_goal实际返回status=paused，工具未说明暂停原因。PG17尚未全面完成，不能
将该状态解释为验收或触发了“PG17完成后暂停”的业务条件。本心跳未启动开发、
构建或测试；候选源码摘要仍与既有收据一致，owner组合日志仍不存在。

旧neon heartbeat仍为ACTIVE且提示自动进入PG18/PG16，已调用应用工具尝试改为
PAUSED并更新为最新PG17停止点，但自动审批服务返回usage limit、明确未执行。
不得绕过审批改写应用配置。后续心跳先读Goal真实状态；paused时不自行复工，
等待所有者明确恢复。源码/候选/既有日志保留，恢复后仍须先完成新owner生产组合。

## 最新本地候选：关闭接续持久WAL及生产入口（未提交）

按所有者最新指示，先完成PG17全部核心代码、Mac可执行验证与远程归档后暂停，
不进入PG18/PG16；当前未满足该条件，Goal active。根AGENTS已记录，原heartbeat
更新遇审批服务断连/超时，工具未执行；恢复必须按最新停止点。

本地新增v11独立Continuation、原UID/RV写前CAS、完整PGWorker三份选择恢复与
显式continue_service_closure管理入口。原关闭/预算/未知不改；当前选择只读
轮询，更高owner先原生Fenced再观察revision，写前确认后派发，不自动重开。
Journal 9顶层/63记录-race及管理协议3顶层/21记录-race通过，0fail/skip；三包
vet终态0，首轮gofumpt修正后最终增量lint0。新生产组合已编写但四次启动均在
创建进程前被自动审批服务502/超时拒绝，没有owner日志/进程，不能算通过或归
Linux后验。详见[当前候选恢复](DSGA-关闭接续持久WAL-开发恢复-20261029.md)及
[候选收据](DSGA-关闭接续持久WAL-候选收据-20261029.json)。

两仓尚未提交/推送；AS HEAD仍ca15c2bd、Neon仍d9701afb及原依赖锁；PG17三处
b078冻结不变，无遗留测试进程，不重跑已通过未变功能。下一步先执行新owner
组合与受影响v10/Worker绑定，真失败先修，通过后再按依赖顺序中文提交推核。
所有者AS AGENTS改动保留；不追上游/独立控制面，不建子代理/镜像/部署/合main。
以下既有已归档阶段仍有效，不将本候选写成已验收或PG17全面完成。

## 新阶段：关闭接续协议、SDK及Compute路由已完成Mac定向验证

AS `ca15c2bd410e42a1c3eb8d1771465a5d5c0a0835`已推既有自有功能分支并核完整
远程SHA，本仓锁采用同一依赖。新增原始/直接前驱/当前关闭的完整选择，固定
同boot/容量/预算，严格递增owner、revision不回退；Rust先取得原生Fenced再关闭，
超时保留未知。原生Stale且setter未发送仅回退本次SDK保留，不清已有未知，避免
迟到owner占住共享SDK。Go恢复完整选择；旧回执/legacy路径不得清新选择。

Mac Go合同-race5顶层43记录、SDK TLS-race3项；Rust合同6项、准确冻结PG17
原生组合1项、真实Router受影响3项及resize观察关闭1项均通过。26向量两仓逐字
一致，四份有效摘要另由真实PG JSONB/SHA核验。原生重启拒绝改绑已验。vet/
增量lint/Clippy通过，已有依赖27warnings和初轮夹具/生成器失败如实保留。
详细边界及复现证据见[本阶段验收](DSGA-关闭接续协议与SDK-Mac验收-20261029.md)
和[机器收据](DSGA-关闭接续协议与SDK-Mac收据-20261029.json)。

**下一步仍须代码**：外层Journal独立接续WAL/UID-RV写前CAS、PGWorker/Session
完整恢复与管理入口及Mac业务组合。本批没有改变v10外层模型，不能称完整新owner
恢复或仅待Linux验收。其他normal/持续约束、异常生命周期、Monitor/CPU与
PG18→PG16缺口不减。PG17三处b078冻结点不变，不追上游/独立控制面；默认关闭，
不构建镜像/部署/合main/发布生产。Goal active，阶段归档不等于整体完成。

## 本轮接续：异常空闲服务关闭Mac阶段已验证并推送源码

审批额度已恢复，准确AS源码f75484e4b650886fb0505ca753e6cb694e6def43已推既有
自有功能分支并核完整远程SHA。随后中文文档归档
`e773bb004ad2e35d56f2628b035e0677b6419995`也已推送核验，本仓发行锁采用
该准确依赖。新六个HTTP/TLS恢复场景1顶层/7条记录、受影响
管理/Session接管/正常自动Grow→Shrink等11顶层/22条均-race通过，0fail/skip；
原模型29顶层/99条采用不重跑。十个共享消费者双Linux架构ELF仅交叉编译，均
exit0且核源码/架构/摘要，未执行，不是Linux认证。两档normal仍均512MiB。

只覆盖无Active、首次开放已Settled且SDK无其他未知的空闲来源异常；独立v10
WAL、写前CAS、原SDK查询/显式原重放及管理栅栏已闭合。来源恢复不自动开放，
保留预算/历史/旧连接，不释放/缩VM。详细证据和剩余业务见
[Mac阶段验收](DSGA-异常空闲服务关闭-Mac验收-20261029.md)及
[机器收据](DSGA-异常空闲服务关闭-Mac收据-20261029.json)。

仍须代码：在途/首次开放未知异常关闭、坏/缺外层日志独立关闭、新owner原关闭
未投递的恢复与受控重开；normal安全降限/私有后台持续约束及活性、分区/出生不
完整/Runner无Wait/外层跨boot迁移、Monitor/WSS/组配额/RW-RO、全部最终写者与
可信CPU。PG17核心完整归档后PG18→PG16，不能用旧版结论代替各版验证。

本次恢复没有重复旧进程、改PG17冻结点或独立控制面。所有者AGENTS未stage、
旧stash未动。不构建镜像/部署/合main/发布生产；Linux/K8s/NeonVM/HugeTLB/
物理/性能和独立控制面后续统一E2E仍后验，缺失业务不转列为Linux待测。
原Goal工具的额度blocked记录保留，本轮按用户恢复授权执行并沿用原heartbeat；
未将目标标complete或paused。下方旧阻塞状态为历史，不覆盖本段实际验证。

## 当前阻塞：连续三轮审批服务额度拒绝

2026-10-29，第三个连续 Goal 回合再次请求“先核进程、再执行六个 Mac HTTP/TLS
关闭恢复组合”，自动审批服务仍返回 usage limit、提示 10:50 AM 后重试（未给
时区）。拒绝发生在创建进程前，不是动作不安全结论；没有 network-first 日志，
六组合没有运行。本轮核对 12 个候选源码与 16 份验证日志摘要均未变化，原分支/
HEAD、未提交所有者 AGENTS、Neon 锁及 PG17 冻结点均保留。

上一轮属于实际进展：三处顺序/未来 floor/完成空间缺口先复现后修复，模型验证
及质量检查通过。本轮不重复它们，不用更多模型用例代替当前必需的网络恢复证明，
也不在关键候选尚未完成验证时叠加其他模块或版本。已达到同一外部阻塞连续三轮
的审计阈值，将调用 Goal 工具设为 blocked；最终状态以工具返回为准。这表示
等待审批服务恢复，**不是功能完成、不是放弃范围，也不是将 Mac 测试转为 Linux
后验**。原执行授权仍保留；不绕开审批，不重置/清除任何未知日志。

恢复审批额度后接续：先核原分支/候选摘要/旧进程，执行六个新恢复组合及受影响
管理/正常路径；有真实失败先修，全部必要检查通过后才中文提交、先推 AS 依赖
并核远程 SHA，再更新/推 Neon 锁。继续总体未实现项和 PG18→PG16 的既定顺序。
本批仍未提交/未推送、不构建镜像/部署/合主分支或发布生产。

## 本地候选追加修复：关闭顺序、未来floor与回执空间

审批服务第二次拒绝六个HTTP/TLS恢复组合，命令未启动、无network-first日志。
继续审查并先用新回归复现，再修复候选的三处缺口：关闭写前点必须晚于原预算/
首次开放/最近结案/永久关闭边界；回执floor不能借用外层未确认的未来owner；
独立关闭未知WAL也须为原生回执保留4KiB完成空间。合法接管只读、正常和实际
容量永久预算、完整codec仍通过。最终按用例去重采用29顶层/99条模型协议
记录，关闭及选择为其中7顶层/44条；三包最终vet/增量lint0。全部先失败及
最终证据见[候选接续记录](DSGA-异常空闲服务关闭-开发恢复-20261029.md)。

代码仍未提交/未推送，Neon依赖锁不变；六个网络组合及受影响管理/正常路径
必须在Mac执行通过后再提交，不归为Linux后验。现有AGENTS/PG17冻结/独立
控制面边界不变，Goal active；不重跑未变模型集合、不绕开自动审批。
以下前一候选及已归档记录保留历史，不能覆盖本段最新修复与待验证状态。

## 本地候选：异常空闲服务关闭，Mac网络恢复组合待执行

AS仍在8214eb5d原功能分支，新增v10持久关闭WAL/生产空闲来源检查/原SDK恢复及
管理栅栏；代码和六个HTTP/TLS组合已写，尚未提交/推送，Neon发行锁未改。
模型/协议按用例去重采用27顶层94条通过，新增关闭/选择为其中5顶层39条；
三包vet/增量lint0。扩展命令一次历史v3无Runner夹具失败已修并复验；误选中
一个端口旧用例因普通沙箱bind失败，均保留日志，不能称整包绿色。

新六组合需要监听端口，自动审批服务仍报额度上限而拒绝执行，提示10:50 AM
后重试但未说明时区；非不安全判定。它们仍是**Mac待验证**，不能归为Linux后验。
代码未过提交门槛，保留本地候选和摘要；恢复先跑新组合/受影响管理及正常路径，
通过后先推AS再改Neon锁。详细合同、失败证据及精确步骤见
[开发恢复记录](DSGA-异常空闲服务关闭-开发恢复-20261029.md)与
[候选收据](DSGA-异常空闲服务关闭-候选收据-20261029.json)。

仅覆盖无Active、首次开放已确认且SDK无未知的空闲边界；关闭只拒新连接，保留
原预算/旧连接，不释放/不缩VM，来源恢复不自动重开。在途异常、坏/缺外层日志、
新owner原关闭未投递及受控重开仍须代码，其他normal/持续约束/生命周期/
Monitor/CPU和PG18→PG16范围不减。PG17 b078冻结、独立控制面和环境边界不变。
原Goal active/heartbeat沿用；本地候选不是阶段验收或整体完成，不重复旧组合。

## 最新归档：自动2GiB到1GiB缩VM的完整Mac控制链

AS准确测试72d8a4208267d935b29c5ea0a2e7d1aeb237bffe、归档
8214eb5d5f73a39ea52dbea24e2467553c085947已推原功能分支并核完整远程SHA；本仓
发行锁采用同一依赖。本批仅两个测试文件，正式源码仍ef3b3313，不重复已通过
Agent双Linux交叉编译、策略/Journal/Agent/PG/Rust矩阵。前阶段Shrink只到Prepare
的边界已在**原资格支持且normal不降低的规格**中推进为完整Mac业务组合。

首次自动Grow至2GiB→生产Finish/Host结清→重建并重新累计无公共客户端低压→
自动Begin Shrink至1GiB→正式Prepare/Host/Quiesce/PG Resize与Release/GuestReady/
Runner ResizeVM/ObserveVM/最终预算/重新开放/生产Finish/Host结清。两规格normal
均512MiB，后台/DSM/2PC保留非零，不靠低占用减小约束或伪造排空。原VM/QMP
回执均读回1GiB，两条Succeeded历史、Active空、准入已开放；QMP扩缩各一次，
Shrink PG Resize一次、关门和重开各一次，无手填领域Completed或手动Finish。

最终Mac -race 1顶层/5含父子记录0fail/skip，120.28秒（包123.025秒）；owner
vet/增量lint0。首轮新增测试字段引用编译错误已修，第二轮119.86秒通过后仅为
本组合留三分钟测试调度余量并补日志再复验；生产请求期限/租约不变，不累计
重复。证据见[阶段验收](DSGA-自动缩VM全链-Mac验收-20261029.md)及
[机器收据](DSGA-自动缩VM全链-Mac收据-20261029.json)。正式源码与ef3b3313
相同已核Git差异，前批双Linux ELF仅编译未执行；本组合PG/Root/Kube/QMP及
Provider释放仍为显式peer，不冒充真实Linux/K8s/热插拔/HugeTLB/物理/性能认证。

**当前剩余代码**：normal安全降限与私有/后台持续上界和活性；异常Root/资格/
日志及越界受控关闭；分区旧进程、出生不完整、Runner无Wait、外层跨boot/迁移；
Monitor缓存/WSS/组配额/RW-RO完整策略、全部最终写者与可信CPU。PG17完整归档
后PG18→PG16独立内核/扩展/发行和各版验证。目标Linux/K8s/NeonVM/HugeTLB/
物理回收/性能和独立控制面后续统一E2E保持后验，缺失业务不能归为仅待Linux。

本批测试/质量/AS推送均终态；接续核分支与未提交后推进上述normal/持续约束和
异常受控关闭，不重复自动Grow→Shrink组合。AS仅所有者AGENTS修改保留不stage，
不apply旧stash。PG17实际HEAD/gitlink/两份锁仍b078736a，上游冻结/独立控制面/
默认关闭边界保持；原Goal active、沿用heartbeat，不构建镜像/部署/合main。
整体尚未完成，阶段归档不是停止。以下历史段落不覆盖本段最新完成与剩余边界。

## 最新归档：可信自动内存策略与原Begin投递恢复

AS策略c1796644、屏障883c6a13、准确生产源码ef3b331309de703c62a273ba8e64d5a060972340，
归档f631f2c3945b5b8229f992d910c2865dbe9db194已推原功能分支并核完整远程SHA。
本仓采用同一发行锁。按VM UID可信配置明确规格/阈值，固定复用Prepare原资格；
原owner空闲轮次只读Root/Runner/PG Work2及同VM RV，持续高压建议Grow、持续更长
无公共客户端低压建议Shrink，紧急优先仍不越闭门/预算/排空/VM门禁。采样间断/
实例/代次/上限/可选资格变化重置窗口，时间水位阻止重复/倒退样本。

自动Begin丢回复保留原完整请求与最后发送RV，恢复先核同Session/Lease再读原WAL。
未接收且RV前进才能关本地投递栅栏；过期同RV使用独立序列CAS屏障，双方提交
顺序和丢回复均已验证，不补NoEffect、不清SDK执行未知、不改原Key/目标/期限。

Mac最终采用19顶层/62含父子记录0fail/skip，四包vet/增量lint0。新正式组合
43.48秒（最终包46.099秒）：自动Grow Begin丢回复→全预算/Host/VM/Guest/PG/准入
扩容→自动Finish及Host结清→重建后持续空闲自动Shrink到正式Prepare；后续明确
Planned。漂移/资格释放/未资格目标零Begin及失租Session零IO负向通过。只采用
最终受影响记录，不重复未变PG/Rust或长租约矩阵。准确源码Agent双Linux ELF
交叉编译通过但未执行，VCS/架构/摘要已核，唯一所有者AGENTS造成modified及
缓存警告如实保留。详见[阶段验收](DSGA-可信自动策略-Mac验收-20261029.md)和
[机器收据](DSGA-可信自动策略-Mac收据-20261029.json)。PG17三处b078不变。

**当前仍须代码**：自动Shrink在Prepare后的完整组合；安全降normal及后台/私有
持续上界/活性；Root/资格/日志异常和越界受控关闭；分区旧进程、出生不完整、
Runner无Wait、外层跨boot/迁移；Monitor缓存/WSS/组配额/RW-RO策略、全部最终
写者与可信CPU全链。之后PG18→PG16独立内核/扩展/发行及各自验证。采样拒绝仅
冻结新建议，不是安全关闭；同VM减Buffer且保持normal的Prepare不代表更小VM或
物理回收。本机PG/Root/Kube/QMP仍是peer，Linux/K8s/NeonVM/HugeTLB/性能后验。

接续先确认上列阶段所有测试/构建/推送已终态、分支和未提交状态，再补自动
Shrink正式Prepare之后的全链及normal降限所需真实安全协议，不重复本批。AS仅
所有者AGENTS未提交保留，不stage、不apply旧stash。原Goal active/heartbeat沿用，
上游冻结/独立控制面/默认关闭边界不变，不构建镜像/部署/合main或发布生产。
阶段归档不是停止或整体完成；以下旧段落保留为历史，不能覆盖本段最新边界。

## 最新归档：正式正常扩缩容自动结案

AS准确源码7cca3f1cc088aed8e512d94b3e5e22c5c23af887、归档
a1c6c05f84a1d40184796e3f571d044ee506c146已推原功能分支并核完整远程SHA；本仓
发行锁采用同一依赖。补齐生产worker全步骤确认后仍占Active、依赖测试辅助
Finish的真实生命周期缺口。独立下一轮严格核原审计、查询原准入、双重认证
PG/Root/Outside资格/Host/Runner及同VM RV才Finish；不重发setter或改原回执/
预算。Finish提交丢回复后Run从原WAL读回，重建不重复历史，Host结清和下一
Prepare通过。五类来源变化均保留原Active；旧领域构造器/取消恢复边界不变。

Mac最终采用7顶层/56含父子记录0fail/skip，新组合53.99秒（包55.733秒）；
两包vet/增量lint0。初次夹具CancelClosed遗漏和宽松解码未知字段失败分别
保留，严格解码后未知/缺失/null/重复字段及上下文负向通过，不累计重复组合。
准确源码Agent双Linux架构仅交叉编译未执行，VCS/架构/摘要已核，唯一所有者
AGENTS导致modified及缓存警告如实归档。详见[阶段验收](DSGA-正常自动结案-Mac验收-20261029.md)
和[机器收据](DSGA-正常自动结案-Mac收据-20261029.json)。PG17三处b078不变。

本组合PG/Root/Kube/QMP仍为明确peer。normal安全降限/后台私有持续上界与活性、
异常来源受控关闭、分区/出生不完整/Runner无Wait/外层跨boot迁移、Monitor自动
策略/全部写者/可信CPU仍须实际核心代码；随后PG18→PG16独立实现验证。原Goal
active，沿用heartbeat；不追新、不改独立控制面、不构建镜像/部署/合main。
本阶段已完成的正常结案不能反复重做，也不代表自动策略或整体功能已经完成。

## 最新归档：CPU完成读回与取消屏障后的PG再出生

Autoscaling CPU准确源码5181b364958928316385f0675529a1894c5d8d69、Root修复
准确源码a009521f1d84af3920aebc8d21e652bcf14650a8，阶段归档
8c4e482cd3afbaab18cdc759b7bf5b2bc8e485de已推原分支并核完整远程SHA；本仓
发行锁采用同一依赖。CPU不再吞OfflineCPUs读取错误，扩缩写后须实际在线数量
读回，部分写失败保留实际效果、下次差量；0/负目标零IO拒绝。受管旧无认证PUT
仍拒绝，本原语修复不等于可信CPU全链完成。

复查前批发现自身取消日志把历史取消命令与当前Birth永久同PID，导致正常PG
再出生后日志无法读取；三类新回归先真实复现，再修复。历史Cancelled保留旧
PID/starttime和水位，当前Birth按原空组/确认约束门槛推进，启动save前校验
整个record；当前取消签发/回执仍核原出生，不借新PG补NoEffect。v2格式不变，
旧Apply/Replay仍拒绝，后继新命令越过持久取消水位。本项不证明外层跨boot恢复。

Mac CPU整包6顶层/15记录、取消/出生受影响14顶层/33记录，均0fail/skip；两包
vet/增量lint0。首次CPU格式问题及Root旧实现复现/沙箱listen拒绝日志保留，最终
授权组4.045秒通过。daemon按5181b364双Linux架构、Agent/Guard按a009521f四
ELF均仅编译未执行，VCS/架构/摘要已核；唯一所有者AGENTS导致modified及缓存
stat权限警告如实记录。详见[本阶段验收](DSGA-CPU结果与取消后出生-Mac验收-20261029.md)
及[机器收据](DSGA-CPU结果与取消后出生-Mac收据-20261029.json)。不重复未变长租约、
PG/Rust测试，不改Controller/Runner；PG17三处b078不变，Goal已核active。

其余核心仍按下表推进：安全降normal/持续后台私有上界、异常Root/资格/日志
受控关闭、分区旧进程/出生不完整/Runner无Wait/外层跨boot迁移、可信CPU写链/
Monitor/自动策略及全部最终写者。随后PG18→PG16独立实现验证。冻结/独立控制面/
不构建镜像/部署/合main边界保持，本阶段归档不等于整体完成。

## 最新归档：原GuestReady未送达的启动交接与持久取消

AS Root primitive a463acb5、准确接线371949821689adf6012d93d4c13d0cecaab99069，
远程归档959465096970031ba633902a7636f93b8991e1e4已推原功能分支并核完整SHA；
本仓采用同一发行锁。Neon认证路由b2a3eb131237f8ab6f589bf80da7cc829187e633。
原GuestReady写前成功而Root未收到时，完整原PG/WAL核验后保留同SDK Pending交
正式PGWorker；普通未知不自动重放/取消，新owner不能重放旧epoch。持久Abort
经Root同flock确认从未接收、原出生和旧约束后fsync取消屏障，才可NoEffect；
丢回复只读屏障恢复，外层再CAS记账，PG/VM/完整实际预算和Host仍独立结清。

Root首次取消同时升级日志v2；旧Guard回滚失败关闭，禁止删除日志/降格式绕过。
取消水位消耗资源revision，后继命令越过Original/Cancelled，不无限存历史正文。
Mac -race去重59顶层/146含父子记录，0fail/skip，四包vet/增量lint0。新真实失租
组合65.14秒，Root setter0、取消POST1、PG Resize0、QMP1，实际预算/Host及下一
Prepare通过。Root真实锁/持久化独立验证，owner组合的PG/root/Kube/QMP仍为peer。
Rust真实UDS/正式认证各1项、3文件格式及受影响库Clippy通过；既有警告保留。
准确源码Agent/Guard双Linux架构4个ELF仅编译未执行，VCS/架构/摘要已核；唯一
所有者AGENTS导致modified及离线缓存警告如实归档。详见[本阶段验收](DSGA-原GuestReady未收到取消-Mac验收-20261029.md)
和[机器收据](DSGA-原GuestReady未收到取消-Mac收据-20261029.json)。

所有本阶段测试/质量/构建/AS推送均终态，不重复已有组合；PG17三处b078不变，
AS仅所有者AGENTS保留不stage，不apply旧stash。Goal已核active，原heartbeat
沿用。安全降normal/持续上界、异常关闭、分区旧进程/出生不完整/无Wait/跨boot
迁移、Monitor/自动策略和全部写者/可信CPU仍需核心工作，随后PG18→PG16；冻结/
独立控制面/不构建镜像/部署/合main边界保持，阶段归档不等于整体完成。

## 最新归档：CPU故障隔离内存状态

AS源码4d8033b298bf88b9e101cabd1a799f2517274af1、远程归档
c7855907e6cd1e1c5d6f0bc11714a440ba3ef3a3。Running/Scaling已共用原Runner内存
来源和直接读取Pod；CPU失败写DSGACPUReady=False，保持Scaling并允许正式Status
写回已确认内存。CPU POST接受后须下一轮GET匹配才Ready，旧无认证PUT未放开；
可信CPU写通道仍有核心代码缺口。CPU错误回包body关闭并限4KiB，不改内存WAL。

Mac按子树采用4顶层/27记录0最终fail/skip，正式Reconcile和Status写回、真实
本地HTTP覆盖读错/写错/超量回包、设置后再GET、坏来源零CPU请求及旧来源/
legacy回归。first新增夹具Create误带resourceVersion失败已修并保留，second
新增整组2.687秒通过；未重复未变子树。vet0、增量lint0。准确Controller双Linux
架构ELF均exit0，架构/VCS/摘要已核，未执行；唯一所有者AGENTS导致modified及
离线缓存权限警告如实归档。详见[验收](DSGA-CPU故障隔离内存状态-Mac验收-20261029.md)
和[机器收据](DSGA-CPU故障隔离内存状态-Mac收据-20261029.json)。

2026-10-29 05:51（Asia/Shanghai）审批恢复后，AS原功能分支已成功推送，
ls-remote核完整SHA为c7855907e6cd1e1c5d6f0bc11714a440ba3ef3a3；本仓发行锁
采用相同依赖。此前额度拒绝没有执行push，历史故障保留在Git；此次未重跑已通过
Controller测试或Linux交叉编译。PG17三处b078不变，原heartbeat/Goal已核active。

以上CPU段为前一阶段历史。本窗口已由本页顶部95946509归档覆盖，不再处于未提交候选；其余核心缺口继续。

## 最新阶段：Agent启动原GuestReady只读恢复

AS源码4f85588c5f29c822a91917d89a99e3605243b145、归档
b90afa0de50f5001782252a6925b33706ae0cb7d已推原分支并核完整远程。修复Agent先等
Root确认、再建PGWorker而原Observe只能由后者执行的环依赖；启动循环只核外层
完整原GuestReady并Observe，不补发、不换epoch/PID/目标、不记录外层步骤或预算。
Root自身可按完整读回补Applied/Held，正式执行器仍重新核PG/VM/预算才Record。
独立HeldRepair未知仍须显式Observe，孤立Pending在RPC前拒绝。

Mac去重10顶层/33父子记录0fail/skip，owner vet0、增量lint0。两个负向场景
第一轮完整响应后仍保持未知，第二轮才取消；修改后的真实失租接管69.31秒，
先过同Agent启动gate再建正式PG，setter/QMP各1、PG Resize0，原Record丢回复、
取消预算与Host结清及下一Prepare通过。准确Agent双Linux架构ELF仅编译未执行，
VCS/架构/摘要已核；缓存权限警告和唯一所有者AGENTS的modified=true如实归档。
详见[验收](DSGA-启动原GuestReady观察-Mac验收-20261029.md)与
[收据](DSGA-启动原GuestReady观察-Mac收据-20261029.json)。

本阶段所有测试/质量/编译/AS推送会话均已终态，AS只保留所有者AGENTS未提交，
不apply旧stash，不重跑未变PG/Rust。PG17三处b078不变；异常来源受控关闭、
安全降normal/持续上界、分区/出生前失败/无Wait/跨boot迁移、Monitor/自动策略/
全部写者与CPU仍须核心工作。PG17完整归档后PG18→PG16；冻结/独立控制面/
不构建镜像/部署/合main边界保持。阶段归档不代表整体完成。

## 当前阶段归档与准确依赖

| 仓库 | 固定功能分支 | 本次采用准确提交 |
|---|---|---|
| Autoscaling | codex/dsga-autoscaling-20261019 | 8214eb5d5f73a39ea52dbea24e2467553c085947 |
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

## 最新阶段：完整Runner出生后未登记launch的关闭

AS源码95e8e2dbf7a3892461af78ffa2dc885bab611af5、归档
45c88cdb0987390536ec7e680aea4908b86f34ca已推原分支并核完整远程。原Runner完整
出生前移至init/磁盘/网络/命令生成前；其后尚无launch的失败，在同flock核完整
WAL并fsync v3关闭屏障，迟到Start/Tick拒绝。原命令/内存未知保留；已有launch
不猜NoEffect或补Wait，发布未知只补同一来源。原Pod删除允许关闭但不允许Start。
Host仍核全部容器终态和Node CAS，sidecar未停不退容量，丢回复可恢复且不重复退还。

同时修复生产命令的SIGTERM私有socket被最终单QMP校验误拒绝，两端统一固定
路径，只额外允许一次该完整powerdown socket；任意路径/选项及重复仍拒绝。
Mac采用三包去重16顶层/41父子记录0fail/skip，vet/增量lint0；Linux runner包
增量lint0，准确源码amd64/arm64 ELF仅编译未执行，架构/VCS/摘要已核。离线
缓存权限警告及唯一所有者AGENTS导致modified=true如实保留。初次Go test参数
位置失败已修，不计通过；最终子树替换，不重复累加。详见[验收](DSGA-Runner未登记启动关闭-Mac验收-20261029.md)
及[机器收据](DSGA-Runner未登记启动关闭-Mac收据-20261029.json)。

本批只关闭完整出生后的未launch失败。出生尚未完整、Runner被杀无Wait、分区/
跨boot迁移、Root/资格异常、安全降normal/私有后台持续上界和Monitor/自动策略/
全部写者/CPU仍须核心代码及组合。PG17三处b078不变，不重跑未变PG/Rust；AS
所有者AGENTS保留不stage、不apply旧stash，测试/质量/编译/推送会话均已终态。
Goal active、既有heartbeat沿用，PG17完整归档后PG18→PG16；冻结/独立控制面/
不构建镜像/部署/合main边界保持，后续继续真实代码缺口。

## 最新阶段：Guest已生效丢回复后真实失租接管

AS测试96ca5c2adcef82fd2abd9819f5401d872971e70f、归档
08d875571479fb20585ef8f83761009ff89f0301已推原分支并核远程。Root已设置丢回复后
真实失租，新SDK恢复原GuestReady Pending；新owner不重放旧命令，只Observe，
Record落盘丢回复后按实际Root/未Grow PG/1GiB VM取消结清预算与Host。Root setter
和QMP各1、PG Resize0；下一新Key Prepare通过。新增Mac-race1顶层/1记录67.77秒、
包70.407秒，0fail/skip，vet/增量lint0；[验收](DSGA-已发送Guest失租接管-Mac验收-20261029.md)
和[收据](DSGA-已发送Guest失租接管-Mac收据-20261029.json)已固定准确源码与日志。

仅测试接线，无生产Go/PG17/Rust或新Linux构建执行，不重复未变矩阵。PG17三处
b078不变，所有者AS AGENTS保留不stage、不apply旧stash。PG/Guest已生效丢回复
接管窗口均已有证据；分区旧进程、异常来源关闭、早期init/无Wait/跨boot迁移、
安全降normal/私有后台持续上界及Monitor/自动策略/全部写者/CPU仍须核心工作。
整体未完整，Goal active、原heartbeat沿用；PG17完整归档后PG18→PG16；上游冻结、
独立控制面、不构建镜像/部署/合main边界保持，下一阶段继续实际代码缺口。

## 最新阶段：PG已发送失租接管与实际预算

AS测试源码ebf064ac0d411662bc6f6086a85d4ff1f034adc1、阶段归档
d8594166517fdc44053fe9118fdfaf8faa2344b8已推原功能分支并核完整远程SHA；Neon
采用同一发行锁。完整首次/Prepare/Host/VM/GuestReady后，PG已提交丢回复，经
真实续约失败、同Lease UID新Session接管和CAS丢回复，原命令/预算/Initial均
保留，旧owner迟到调用及新owner重发旧Resize拒绝。原取消TooLate确认实际
Committed；Record落盘丢回复后仍用同SDK只读Status清原Pending，再按实际PG
成本/1GiB VM结清预算、Cleanup和Host，下一新Key Shrink Prepare成功。Resize
和QMP各1次，不能用旧512MiB Initial覆盖已发生效果。

新增Mac -race1顶层/1记录73.58秒、包76.167秒，0fail/skip；owner vet及相对
69f30248增量lint0。仅测试接线，PG17/Rust/生产Go未变，不重跑未变矩阵或重编
Linux产物；准确23d455fb编译证据按未变范围复用。首次Prepared字段错误和
lint无效语句已修，失败日志不计通过。详见[验收](DSGA-已发送PG失租接管-Mac验收-20261029.md)
与[准确收据](DSGA-已发送PG失租接管-Mac收据-20261029.json)。

Guest已派发、分区旧进程、无launch早期init、无Wait/跨boot迁移、异常来源关闭、
安全降normal/私有后台持续上界及Monitor/自动策略/全部写者/CPU仍须核心代码或
组合验证，不能归为Linux待验。PG17三处b078不变；AS所有者AGENTS修改保留不
stage，不apply旧stash。阶段测试/质量/AS推送均已终态，不重复运行；Goal active、
既有heartbeat沿用，PG17核心完整归档后PG18→PG16；上游冻结/独立控制面及
不构建镜像、不部署、不合main边界保持。

## 最新阶段：PG发现前Root恢复与交接

AS核心11dee1f1、Agent准确源码23d455fb9ed95cdca35474bd670fbc75d8416550、归档
69f302488371aad9441ef17c8c3044874f3499cd已推原分支并核完整远程；Neon采用同一
发行锁。已有完整确认Bootstrap、没有孤立Original时，漂移中的原Guest先固化
身份/fence；PG发现前的只Root端点提供原摘要显式修复/观察。管理专用绑定0 boot
只表示Root阶段，不放宽原PG Instance非零约束，也不允许PG/业务动作。普通轮询
不修复、不清Pending；确认后先结束/注销，再建立真实PG端点。执行槽串行、Run/
Session/调用者三方取消及原WAL保留。详见[验收](DSGA-启动期Root恢复交接-Mac验收-20261029.md)
与[收据](DSGA-启动期Root恢复交接-Mac收据-20261029.json)。

Mac四包去重20顶层/52父子记录、0fail/skip，vet/增量lint0。真实私有UDS→正式
恢复循环及实际阻塞TLS管理调用的Run独立取消通过。Agent/Root Guard双架构共
4个准确ELF仅编译未执行，VCS revision/架构/摘要已核；唯一AGENTS改动导致
modified=true，离线模块缓存权限警告如实保留。首次字段/管理绑定解码及测试
传输类型错误已修复，失败日志不计通过；旧等价子树不重复累计。

本批只关闭“有完整原来源的初始Root约束漂移”窗口；缺/坏来源、未确认Bootstrap/
孤立命令及实际高于原限额、swap/任务/系统异常的受控关闭仍需代码；安全降normal/
持续上界、其他接管/启动/无Wait/跨boot迁移、Monitor/策略及全部写者/CPU继续。
所有本阶段测试/质量/构建/AS推送会话已终态，不重复轮询；AS所有者AGENTS保留
不stage，不apply旧stash。PG17三处b078不变，整体未完整，Goal active，沿用原
自动接续，PG17完整归档后PG18→PG16；上游冻结/独立控制面/镜像/部署/main边界不变。

## 最新阶段：normal最终写者防线

AS安全代码480c0259、归档8f371309dbfadec1b3346dde4914d78beb525571已推原分支
并核完整远程；Neon采用相同发行锁。生产Root运行写在原文件锁内比较Held与
实际normal，上新WAL及写内核前拒绝未经证明的降限；旧pending保持ErrUnknown、
只读Observe仍可确认历史，已确认相同重试只读。受影响Guard整包Mac-race47顶层/
96父子记录、0fail/skip、包7.867秒；vet和增量lint0。准确[验收](DSGA-normal最终写者防线-Mac验收-20261029.md)
及[收据](DSGA-normal最终写者防线-Mac收据-20261029.json)已固定四文件/逐用例/
日志摘要。此防线不是安全降normal实现；相关持续上界及其他核心代码继续。
本批无新Linux构建/执行，不重跑其他未变矩阵。PG17三处b078不变，独立控制面、
上游冻结及部署/main边界不变，Goal active，PG17完整归档后PG18→PG16。

## 最新阶段：原Root恢复接入同一管理循环

AS准确业务463c1ad8031de4d15bc6e7c8ced6b165ff13ca40、归档
378c91022429f1a1d093b4b359ef4dc25cdb346a已推原分支并核完整远程；Neon采用同一
发行锁。resource_state/repair_held_resources/observe_held_resources复用同一
ResourceWorker，在当前Session同一个PGWorker.Run中按原Held摘要显式修复/只读
对账。错摘要零POST；修复生效丢回复后保留Pending，显式Observe清独立栅栏，
总POST1次，外层预算/GuestReady/VM/Host WAL不变。Mac-race4顶层/20父子记录、
两包0fail/skip；四包vet、增量lint0。详见[阶段验收](DSGA-Root管理恢复入口-Mac验收-20261029.md)
及[准确收据](DSGA-Root管理恢复入口-Mac收据-20261029.json)。本阶段没有新Linux
编译/执行；旧12a993e7双架构ELF不认证本次源码。入口仍依赖PGWorker已成功构造，
建立前Root异常、安全降normal/持续约束及其他核心缺口继续，不能归Linux待验。
PG17三处b078不变；所有者AS AGENTS修改保留不stage，不apply旧stash。测试与
AS推送会话均已终态，不重复运行。Goal active，原heartbeat沿用，PG17完整后
PG18→PG16；上游冻结和独立控制面/镜像/部署/main边界不变。

## 最新阶段：Agent私有管理入口与同一owner串行接线

AS公共模块f94d4e86、Agent源码12a993e7677817e9d936c79f072df98be9670afe及归档
b9f9ab7fbb4f79bc8ee5f607a51a5900a4d00a85已沿原分支推送并核远程SHA，Neon采用相同准确发行锁。
默认关闭的私有UDS已实际接Agent taskgroup和当前Session，支持完整Request接受、
状态/owner发现、取消与按原Key/摘要的分域显式恢复。HTTP不另建执行者，全部进
同一个Run；三方取消/原WAL未知和领域fence保留，不改独立控制面或部署文件。

Mac四包去重16顶层/43含子记录0fail/skip，vet/增量lint0；真实UDS→正式
Elector/Session→UID-RV Begin/幂等/零派发取消历史、原PG显式重放及Run取消
正在执行的管理HTTP通过。准确源码Agent Linux双架构仅编译、未执行，VCS
revision一致，唯一AGENTS改动导致modified=true；详见[验收](DSGA-Agent本地管理入口-Mac验收-20261029.md)
和[收据](DSGA-Agent本地管理入口-Mac收据-20261029.json)。不是全预算/物理产品E2E。
Monitor/自动策略、worker建立前的Root异常入口及其他核心恢复、normal持续约束、
全部写者/CPU仍须代码，不归Linux待验；PG17完整后PG18→PG16，Goal active。

## 前一阶段：QMP已生效丢回复后真实失租接管

AS测试源码`a6a2e7616c07f82a33d30fcc11d0722784d1b577`、归档
`909756fe4f7407b86edfa4d4830a397d20591771`已推原分支并核完整远程SHA；Neon采用
同一准确发行锁。原Runner跨may_have_sent并将实际VM改至1GiB后丢回复，新Session
在真实续约失效/同Lease接管后只读回原Completed，无QMP重发或NoEffect伪造。
取消以原Prepare Outside/资格结清实际1GiB，拒绝旧512MiB Initial和替换UID；
随后Host结清、同容量VM no-op的后继PG Grow及下一Shrink Prepare通过。

Mac-race新增1顶层/1记录113.40秒、包116.312秒，0fail/skip；owner包vet和
相对5c8a1bd1增量lint0。仅测试接线，无生产协议/PG/Rust改变，不重复未变矩阵。
首轮错误Model断言与最终Reader来源认证层次见[验收](DSGA-已发送VM失租接管-Mac验收-20261029.md)
及[准确收据](DSGA-已发送VM失租接管-Mac收据-20261029.json)。两个原VM接管窗口
已有证据，其余PG/Guest已派发、分区旧进程/启动/无Wait/跨boot迁移及核心缺口
仍须代码和组合；没有新增Linux执行或ELF构建，整体核心未完成，Goal active。

## 前一阶段：真实失租接管后的原VM取消与后继业务链

Autoscaling测试代码`2501160db2524aacd1daa09f9311ce5250032ed6`、归档
`5c8a1bd1b284beb397c1110c6a2994d48b359dd4`已沿原分支推送并核完整远程SHA。Neon准确
发行锁和本页采用同一依赖，PG17子模块/两份锁仍b078。本批仅测试接线，无生产
协议/PG/Rust改变。首次开放→原ResizeVM未知→旧holder续约失败→同Lease新Session
接管（CAS落账丢回复）→原Key取消→永久实际预算/Host结清→后继完整Grow→下一
Shrink Prepare组合175.02秒通过；旧WAL/Initial/其他持有完整保留，旧worker迟到
调用及新owner重放旧epoch均拒绝。Lease30/20/2秒未改，没有手改Owner或删Lease。

最终Mac-race4顶层/4记录、0fail/skip；owner包vet/增量lint0。源码/日志摘要与
准确映射见[本阶段验收](DSGA-真实失租接管业务闭环-Mac验收-20261029.md)和
[机器收据](DSGA-真实失租接管业务闭环-Mac收据-20261029.json)。不重复未变PG/Rust/
其他Go矩阵，没有新增Linux执行或ELF构建。该窗口完成不外推已派发PG/Guest、
分区旧进程、无Wait/早期init/跨boot/迁移全部闭合，其他核心缺口继续实现。

## 前一阶段：原QEMU确定未启动的关闭和Host结清

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
| 原Root显式管理恢复 | 同一SDK/Session/Run按原Held摘要修复及丢回复只读观察，外层预算/WAL不变；不涵盖worker建立前异常 | [本次Root管理验收](DSGA-Root管理恢复入口-Mac验收-20261029.md) |
| PG发现前Root恢复与交接 | 完整确认Bootstrap的初始漂移先固化原Guest，0 boot管理专用端点显式恢复；未知保留，确认后交接真实PG端点 | [启动Root验收](DSGA-启动期Root恢复交接-Mac验收-20261029.md) |
| Agent可信本地协调入口 | 私有UDS实际接同一Session/Run，Begin/状态/取消/显式领域恢复与三方取消；默认关闭 | [本次入口验收](DSGA-Agent本地管理入口-Mac验收-20261029.md) |
| 已发送Guest失租接管与实际预算 | Root生效丢回复后的原Observe/Record丢回复、实际预算/Host及下一Prepare通过；Root/QMP各1、PG Resize0 | [本次Guest接管](DSGA-已发送Guest失租接管-Mac验收-20261029.md) |
| 已发送PG失租接管与实际预算 | PG已提交丢回复后真实失租、原TooLate回执Record丢回复与同SDK终态核对、实际预算/Host及下一Prepare通过；Resize/QMP各1次 | [本次PG接管](DSGA-已发送PG失租接管-Mac验收-20261029.md) |
| 已发送VM失租接管与实际预算 | QMP生效丢回复后真实续约失租，新Session只读Completed、按1GiB结清原Prepare预算/Host，后继PG Grow及下一Prepare通过 | [本次已发送接管](DSGA-已发送VM失租接管-Mac验收-20261029.md) |
| 真实失租接管后的原VM取消与后继业务链 | 正式Elector续约失败/同Lease接管CAS丢回复，原WAL和预算保留，取消/实际预算/Host/后继Grow/下一Prepare通过；只涵盖该窗口 | [本次接管验收](DSGA-真实失租接管业务闭环-Mac验收-20261029.md) |
| Runner完整出生后未登记launch的关闭 | 同flock缺launch确认、v3持久屏障/迟到Start拒绝，发布丢回复、整Pod终态/Host CAS和Controller消费通过；生产固定powerdown QMP契约已修 | [本次未launch验收](DSGA-Runner未登记启动关闭-Mac验收-20261029.md) |
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
| 2 | 安全降低normal与活性 | 上层执行器与最终Root Guard均拒绝无证明的normal降低，旧未知只读恢复保留；完整后台/私有持续上界及安全降限协议仍有代码工作 |
| 3 | 启动/接管/跨boot恢复 | 原VM未发送及已发送丢回复两个真实失租接管窗口、持久launch后确定NoStart已完成；PG及Guest已生效丢回复的失租接管/实际预算窗口已完成；完整出生后未launch的init失败关闭已完成；分区旧进程、无Wait、出生不完整及跨boot/迁移仍须组合与受控恢复；启动原GuestReady未送达Root的同SDK交接与原取消屏障已闭合，不能自动改绑原出生 |
| 4 | 持续控制与入口 | Agent私有可信请求/取消/分域重放入口已接；PG发现前有完整原来源的Root漂移恢复已接；CPU失败不再吞内存状态已验证归档；受管Guest旧无认证CPU PUT继续拒绝，可信CPU写通道、Monitor/自动策略、缺源/其他异常关闭及全部写者仍须代码及组合 |
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


### 2026-10-30：早期关闭写者与原永久预算来源

AS `cfffcb8f1f3984a2d16f585747b7185fb38c9a63` 的有界来源、Prepare/Host最终门禁、只读在途认证及文档四模块已推自有分支并核SHA；Neon发行锁准确引用，PG17仍79367196。
采用Mac定向race去重26根/136含父子记录0fail/skip，7修改/受影响包vet与增量lint0，33消费者仅编译。真实前驱证明旧/新关闭快照不得新Host Reserve/CREATE/QMP；Prepare晚到CREATE不改变Unknown。六格式及恢复旧Unknown零SettlementEpoch兼容已验证，初测失败/环境/反向证据单列，详见[阶段验收](DSGA-早期关闭门禁与原预算来源-Mac验收-20261030.md)。

本批不形成早期16完整持久恢复/旧预算重开/完成覆盖，Prepare Completed而Host缺来源仍需真实Node屏障；类别18后继、持续预算/安全降normal、直接接线与最终修改范围组合仍未完。仅测试新增/修改/受影响内容，未变PG内核/Neon扩展/SQL/Router复用准确既有SHA证据。冻结不动，不构建镜像/部署/合main；持续目标保持active，PG17全面归档后才停止，不进入PG18/16；Linux目标验收单列。


### 2026-10-30: 早期原预算恢复闭环：Mac 阶段验收

AS `00eda3d5e5fc6ecb76e6e01fcbbf569e85609bda`

本批接上一批关闭门禁/有界来源，补齐类别16最早期的完整业务：Prepare仍Planned或Unknown、其它资源步骤全部Planned且无Cleanup。支持原Initial、普通/独立Finalization及三种实际Capacity永久头，不把未派发资源动作改成成功。原Abort、Prepare Unknown、关闭命令及旧永久头的原始字节/落账序列保留。

Mac普通race采用去重 **42顶层根 / 263含父子记录，0 fail / 0 skip**，不是263个完整E2E。5修改/直接受影响包vet、普通增量lint和native标签compute增量lint均通过（0 issues）；33消费者只编译，不计业务验收。

本批不包括Prepare已Completed而Host缺来源的早期窗口；其中可能已有在途Node.Reserve，仍需真实串行屏障，不能按Host Planned判无预留。类别18/后继消费者剩余组合、持续私有/后台预算及安全降低normal、直接策略接线、准确PG17修改范围最终组合仍保留。本阶段不能视为PG17全面完成。

[早期原预算恢复闭环：Mac 阶段验收](DSGA-早期原预算恢复闭环-Mac验收-20261030.md)

PG17仍为`79367196a74eef82e67cac917877d2e6271d3e70`；Linux/KVM/K8s/HugeTLB物理返还与性能SLO单独验收。持续目标保持active，PG17全面远程归档后才停止，不进入PG18/PG16，不构建镜像、不部署、不合main。
