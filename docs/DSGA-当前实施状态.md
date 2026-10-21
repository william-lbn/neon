# DSGA 当前实施状态

更新：2026-10-22（Asia/Shanghai）。V3为实施标准；**整体仍未完成，Goal保持
active**。本页为当前事实入口；阶段性通过不能替代完整交付。默认关闭，不合
自有主分支，不构建镜像、不部署K8s、不发布。

## 最新接续：2026-10-22 创建计划、058前向兼容及PG17维护

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
