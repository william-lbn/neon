# DSGA 执行与恢复记录

更新：2026-10-21（Asia/Shanghai）。V3是实施标准；Goal保持active、无预算。
本文件为接续入口，旧过程详见模块/TAP记录。恢复先检查Git、GitHub和真实进程，
不能重复已有构建或apply stash；未完成全部业务前不得标记Goal complete。

## 当前源码与分支

### 最新所有权与持久PG循环（优先于全部下文历史）

- Autoscaling生产实现1bf247d06c89d24f141d15f32cf6ca50116b2f0c、阶段验证补强
  75187718d7f4e99ed46924305e89ad83a943b0de均已推送/GitHub核验；所有者AGENTS.md
  7行修改保留。owner包与长期PGWorker已实现，Agent入口/凭据/完整领域循环还需接线。
- 最终254事件/12测试包/9无测试包，0fail/用例skip；native全范围lint0。8项实际
  断言反向恢复3份源码后完整race重验，48130终态退出0，不重新启动。源码SHA和
  夹具失败边界见[本模块](DSGA-所有权与持久循环验证.md)，旧stash不能再apply。
- 真实epoch2循环4096 grow及第二次提交丢回复/持久查询/显式重放，1外层/2 Go race
  已执行；带阶段硬约束候选8.09s，准确依赖复验见neon-owner-native-pinned.log。
  删除阶段仅保留PASS的日志被同一门禁拒绝。PG6b7及ABI6/三处锁/二进制不变，
  主仓当前仅门禁/中文方案记录，推送状态以实际Git/refs核验，不误报全业务完成。
- Neon/Autoscaling main、Neon/社区PG17 fresh fetch均已包含。继续Agent实际接线与
  凭据broker/Driver、受控跨owner/boot关闭、全预算/Monitor/最终Runner/其余PG等。
  Goal active，不构建镜像、不准备K8s、不合main。Linux物理认证后验。

### 最新PG持久派发适配器（优先于下文历史）

- Autoscaling `48f01e2aee8208ba1ab5392a518434e276086066`已提交/推送并GitHub核验，
  只余所有者AGENTS.md修改。新pgexec实际接线Start确认→Compute/PG→完整事实Record，
  原意图查询/业务取消/显式同owner重放与Provider独立证明均实现；不是完整协调循环。
- 最终race219事件，0fail/用例skip，11测试包/9无测试包；native全范围lint0issues。
  4个最终反向均实际断言失败，SHA256 b8355b…逐字节恢复，随后完整race通过。
  日志autoscaling-pgexec-{publish.*,negative-final.log,lint-publish.log}。
- Neon PG6b7/三处锁/二进制不变，API1.8/ABI6前置960551d已推送。真实Go持久恢复
  门禁已通过，准确Go依赖再编复验见neon-pgexec-native-pinned.log；主仓仅扩大源码
  状态记录与中文结论，当前提交推送/远程以Git/refs核验。详见[新链路](DSGA-PG持久执行链路验证.md)。
- 广泛envtest子目录缺etcd导致0/2启动失败/既有清理panic单列，不伪装Mac限制。
  17456最终流水线终态0，不重复启动；旧stash绝不再次apply。全部目标保持active，
  继续可信owner/持久实际循环与完整预算/Monitor/VM链，不构建镜像/准备K8s/合main。

### 最新issuer floor/ABI6（优先于全部后续历史快照）

- PG17 `6b7c3655ab74062e117eef7400953cd000679a49`已提交/推送且GitHub核验，
  工作树干净。主仓gitlink及两份锁均6b7，准确重编二进制已核对。Autoscaling
  `831a453e3adaa86f163027a1f7319139f78b55c5`也已推送核验，AGENTS.md用户改动保留。
- 新内核单调floor/原子登记/Provider退出接管，Neon1.8六正式SQL/HTTP与双语言
  SDK配套完成。兼容/恢复/锁序/反向及完整剩余业务见
  [当前模块](DSGA-PG接管隔离与正式API.md)。不能用旧ABI5库混载；成功Fenced
  不清未知意图或预算，不产生VM许可；可信owner与实际运行链继续实现。
- 唯一准确SHA流水线17805终态退出0：内核80、SQL/存储52、SDK10、HTTP12、真实
  Go外层1/独立race2通过；普通Rust36/31、Clippy通过。普通15ignored已严格另测。
  prepare145中13通过/132环境skip。日志neon-issuer-*；旧句柄不要重新启动。
  Autoscaling完整race185case pass事件/10测试包/9无测试包，0fail/用例skip；两lint0。
- off/on全world+TAP来自与6b7 src diff SHA256相同的提交前候选，各成功但含平台/
  可选skip；实际移除floor登记检查和提前Fenced的反向共3用例均失败、finally恢复。
  不删除失败证据，不重apply 6f002a3/f5c4dfc stash。主仓本批需提交推送核验，
  恢复以实际Git/远程refs为准，不能将候选当远程成果。

### 最新Autoscaling接续（优先于后续阶段快照）

- Neon SDK/HTTP当前功能头6e63d8d57e3df8ce98c9770c8d322d7c296ca686已推送核验。
  Autoscaling在自有codex/dsga-autoscaling-20261019新增0ec15c7固定UID/RV/动作
  原资源快照条件写入、f37811d67cb7d68f58a915e64830342292bf6ed8 Go完整请求契约，
  两项均已推送且GitHub refs与本地一致。PG486338及两份锁/准确二进制未变。
- Go不能把官方进度Revision当不可变ID：同Revision的分批批准保持可用；实际
  业务调用改为GET+UID/RV test+精确资源更新，空身份/陈旧动作/不一致响应拒绝。
  原Go数据源新增可省略UID，VM和Pod来源区分正确；源码中文关键注释/模块提交。
- Mac race受影响组合132个pass事件（含父/子测试/fuzz种子），0fail/用例skip，
  8包通过、9包无测试文件不计；Go请求fuzz1275872次通过，v2 lint 0issues。
  移除两条test操作，实际Revision6倒退5、新对象8slots误写4，两项确定性失败；
  finally逐字节恢复后最终组合通过。日志build/autoscaling-dsga-cas-{full-final.jsonl,
  full-final.stderr,lint-final.log,negative.log}和autoscaling-dsga-contract-fuzz.log。
  62209/88750终态0，不重新poll。默认CLT27 SDK链接错误以既有Xcode14.4解决。
- Autoscaling已有所有者AGENTS.md的7行边界修改保留，不混入业务。完整状态入口
  为该仓docs/DSGA-执行状态与恢复.md。本批尚无Go实际HTTP消费者、持久账本/
  issuer、全部预算或最终Runner fencing；直接QMP旧路径仍须封堵。API模拟CAS
  不等于真实apiserver/etcd/QMP/VM，不能把这些代码剩余项归入Linux待测。

### 最新Compute HTTP与客户端门禁（优先于后续阶段快照）

- SDK276230c已推送核验；owned/SET权限修复f4bd55c998a320120d6d3254c4872feb27da1362
  已独立提交，HTTP接入与中文记录作为下一提交一并推送自有同一功能分支；恢复以
  实际HEAD/GitHub refs核验。PG486338、三处锁及原生二进制不变。
- 正式HTTP五路由、专用严格JWT、长期双连接/进程内原意图栅栏、No-store和
  OpenAPI已完成本机模块验证；Monitor、持久owner/完整预算/VM链仍未完成。
- 最终严格SDK8项12.27s、HTTP11项6.09s，零ignored/filtered（19071终态0）；
  两包普通32/30通过，另默认8/4 ignored已另实测（60986终态0）；两包Clippy
  --no-deps全targets -D warnings退出0（86660终态0），依赖27warnings保留。
  prepare129项：13通过/116环境skip（34305终态0），不把skip或重复执行混计。
- 最新日志build/neon-dsg-http-{packages-final,strict-final,clippy-final,prepare-final}.log，
  反向权限证据neon-dsg-http-noinherit-negative.log。全部句柄已结束，不重新poll。
  SDK/HTTP模块文档与下面最新接续节给出边界，全部目标保持active。

### 本轮R09已验证结果（优先于下面6009历史快照）

- PG17当前干净提交`48633803b05c1b7bd9a40d749b5278df9aaf01ac`已推送自有
  功能分支并核验GitHub refs；新增内核独立回执、16条历史、epoch/hash绑定、
  业务取消与完整逐数组释放身份，Buffer ABI5。PG原48+新19项67项通过122.538s；
  最后补两项锁序/历史隔离后新增21项通过44.279s，均零skip。
- Neon内核绑定批次`545c4cdbe623ff8640ca6f60d6c36293f9d11d71`、正式SQL ad1b26f
  已推送并核验；gitlink/ci/distribution.json/vendor/revisions.json均486338。
  恢复必须用git rev-parse HEAD及gh refs实时核验最新客户端/HTTP提交。
- 准确SHA唯一顺序流水线**70614已终态退出0**，不能重新poll或启动。实际二进制
  输出完整486338 SHA；69项内核通过127.064s、22项完整Neon扩展通过40.470s，
  均零skip。off/on各基础81组/666项；各48份TAP报告、231报告文件/18410报告
  断言，46PASS/2NOTESTS/零FAIL；报告总数含skip，不是全部配置/平台均已执行。
  prepare104项：13通过/91环境skip，真实69+22另有执行证据。脚本
  `build/run-neon-r09-validation.sh`，日志
  `build/neon-r09-exact-{build,lifecycle,extension,world,prepare}.log`。
- 保留旧缺陷反向证据`build/neon-r09-guard-negative-fixed-fixture.log`：Grow/
  Shrink在Guard未释放时业务取消仍等原期限，两项2秒超时；补绑定轮询后通过。
  更早guard-negative是测试夹具错误，不是生产缺陷证据，详细模块文档有说明。
- 准确SHA门禁已全部结束；正式Neon SQL入口也已接入/加载验证，见下一节。
  此模块不是全业务完成，Goal保持active。

### Neon正式SQL/API批次（当前已验证）

- pgxn/neon/dsg_control.c/h已加入Makefile/_PG_init，扩展1.6→1.7及反向SQL完整；
  五个管理SQL函数、superuser会话边界、严格原始JSON、实例绑定/完整意图hash、
  独立receipt/observed和真实取消均已实际加载，不能再按旧快照称未接线草稿。
- Compute启动契约v2新增必填VM/Compute身份，既有配置与原生-c路径同时固定；
  普通静态spec兼容。旧v1必须补身份并受控重启，控制请求协议仍1。
- 最终严格完整Neon脚本**31469已终态退出0**，47项通过85.043s、零skip，含
  原22项及新25项正式SQL测试；clean/build/install Rust communicator及全部配套库。
  Rust compute_api24项、compute_tools23项通过，零ignored；两包--no-deps全目标
  Clippy -D warnings退出0，依赖remote_storage既有27warnings保留。
- 最终prepare129项：13通过/116环境skip；真实69+47已另行执行，不能重复混计。
  PG17/14/15/16头严格-Werror语法、pgindent/Ruff/Bash/diff/锁绑定检查通过；
  旧头仅为无能力时拒绝路径，不是旧版本功能移植。
- 新GUC实际发现占位值非法时仅WARNING回退；独立旧候选三项全部错误地启动
  成功，反向日志neon-r09-api-startup-negative.log保留。补初始化拒绝显式失败后
  三项及受管CLI覆盖非法备份配置均通过。最初45项失败日志不删除。
- 日志build/neon-r09-api-{strict-extension-final,compute-tests,clippy,prepare-final}.log。
  87161/38031/41675/66764/8833/1671/31469/80756等均已终态，不重新poll/启动。
- 该阶段之后已完成共享Rust客户端/Compute HTTP；继续Monitor/Go客户端、持久意图与历史、全量预算/quiesce、
  跨层fencing与真实存储端到端；不是只待Linux实测。API仍production_ready=false、
  vm_shrink_permit=false；默认关闭，不构建镜像/部署/合并/发布。

### R09之前的源码与6009门禁快照（历史）

- PG17实际目录 `repos/neon/vendor/postgres-v17`，分支
  `codex/dsga-pg17-v3-20261019`，当前干净提交
  `6009cf4f7f64892f08e31201ceafb611dba1fbcc`，**已推送并GitHub refs核验**。
- 该头新增三个独立中文提交：逻辑槽升级 `694d751f34`、配置样例 `40777b2e9c`、
  WaitIO资源释放入口 `6009cf4f7f`。保留S3/SLRU/v2/restartpoint及所有此前修复。
- 官方维护组合 `4a39412f154796461f404785d369a3dc9424fcdc` 已包含Neon PG17
  `56692dfb680281a963c7470fc7f0fec7f65ecfd4` 与社区
  `91246c72695cdddf62003f6bf4c4ade511f134f4` 的全部祖先；本批fresh fetch未变。
- Neon分支 `codex/dsga-neon-v3-20261019`；6009绑定/完整宿主门禁
  `6cdc52e`已推送并核验GitHub refs，保留存储集成0f8dbed全部内容。本记录随
  后续wire契约模块提交更新，恢复仍须以git rev-parse HEAD和gh refs核验本提交。
  gitlink、ci/distribution.json、vendor/revisions.json均已绑定6009并通过严格检查。
- Neon官方main仍fa504217…，Autoscaling官方main仍a4bce09e…，均已为各自功能
  基线祖先。自有主分支/稳定分支未合并、未发布；其他PG发行升级与移植尚未完成。
- Autoscaling `codex/dsga-autoscaling-20261019` / ef6cd154…、control-plane
  `codex/dsga-control-plane-v3-20261020` / 928eec95…、Helm
  `codex/dsga-neon-helm-v3-20261019` / 648488a，仍无本业务完整运行实现。

## 准确6009的全部已完成门禁

唯一pipeline **39556已终态退出0**，原PID22114已退出，不能重新poll或重启它。

1. root Make准确重编，postgres --version实际输出完整6009cf4f…。
2. 严格内核专项48项通过92.573s，零skip；含三项新增releasing-owner完成/取消/FATAL。
3. 完整Neon（含Rust communicator）、neon_test_utils、rmgr和测试库clean/build/
   install并实际加载，22项通过37.838s，零skip。
4. 新 `ci/test-dsg-postgres-world.sh` 实际执行off/on完整check-world和TAP，两组
   均通过：各基础81组/666项；各48份TAP报告、231个报告文件/18410个报告断言，
   46份PASS、2份NOTESTS、零FAIL。报告总数含平台/可选skip，不能写全部零skip。
   两个恢复组均52文件/630断言通过；真实027、逻辑槽003及新061都成功。
5. prepare83项：13通过/70环境skip，非业务通过数；上述48+22另有实际执行证据。
   Ruff check/format、bash -n、PG新C/测试C pgindent、git diff --check和锁绑定通过。

日志相对工作区根：

- build/neon-6009-{integrated-build,lifecycle,extension,world}.log
- build/neon-integrated/v17/dsga-world/{off,on}.log
- build/neon-6009-prepare.log

标准build跳过Windows/SSPI/Windows句柄、当前未启用的ICU/readline，以及未启用
PG_TEST_EXTRA的DNS/encryption/xid_wraparound。部分可在Mac另配，不能一概称为
Linux限制；DNS负载测试自身仅Linux/Windows。其他TLS/GSS等配置矩阵未由这组认证。
PR CI新增宿主原生全量门禁和Perl依赖，**没有远程Linux执行证据**，不构建镜像/部署。

## 缺陷与反向验证的保留证据

- restartpoint真实缺陷：备库没有主库PreCheckPointGuts，却按SHUTDOWN跳过
  持久化；旧版新061在重启后缺表。e02a09ad按显式调用路径修复，保留主库额外
  WAL顺序；开/关4恢复文件各75断言通过。日志 build/neon-restartpoint-*。
- 初始TAP八个失败已有逐项关闭证据。preload fixture保留实际解析的rmgr；Neon
  SEQ_LOG_VALS=0的精确值；关机slot保存值与重启解码精确比较。临时省略关机slot
  保存真实失败2/4，源码恢复后通过，未提交故障源码。见build/neon-slot038-*。
- 逻辑槽pending误判：实际WAL含neon-file槽快照，普通/流式输出过滤但fast-forward
  当成业务pending。统一过滤并拒绝SQL占用保留前缀，保留真实应用消息/DML保护。
  仅恢复旧条件真实失败3/35；逐字节恢复重编后开/关各45断言通过13s。
  日志 build/neon-pending-wal-{negative,restored-off,restored-on}-tap.log。
- 回滚WaitIO：此前on完整流复制回归真实ROLLBACK在releasing owner上Enlarge失败，
  继发ProcArray断言，非环境skip/随机可忽略。证据完整保存在
  build/audit-dsga-rollback-waitio-crash/，原on整体失败日志仍保留。
  新路径用专属无父owner登记真正pin，先撤CV再删登记/空owner、最后原生NoOwner
  unpin，不递归执行全局回调。旧WaitIO确定性失败15.544s；源码逐字节恢复/重编后
  八项IO专项通过16.282s，真实主备027/9断言通过51s。
  日志 build/neon-waitio-{negative,restored}-tests.log、
  build/neon-waitio-stream-regress-on.log、build/neon-waitio-releasing-negative-control.log。

全部负向脚本已结束、finally恢复完成。PG工作树干净；不存在正在运行的故障版本。
新PG模块文档分别为DSGA-Neon逻辑槽升级兼容.md、DSGA-Neon配置样例完整性.md、
DSGA-IO等待资源释放修复.md。不能根据旧过程里的“还在运行”重启旧句柄。

## 生产请求契约模块（独立于上面的6009门禁提交）

等待PG全量时，在现有compute_api新增dsg_control.rs及lib.rs导出，详见
DSGA-生产控制请求契约.md。这批只交付wire类型/范围/期限，不是生产端点或许可。
Rust本包21项通过（含9项新增，零ignored）；--no-deps Clippy -D warnings通过，
完整path依赖Clippy因remote_storage已有41项lint失败，记录保留，不改无关源码。
日志 build/neon-dsg-control-compute-api{,-clippy,-clippy-local}.log。
本机独立rustfmt过旧，显式rustup run 1.88.0 rustfmt，未改全局环境。

6009绑定/回归/world脚本及记录已独立提交推送核验。wire类型、导出、契约文档及
CI本包测试另作中文提交；不改变PG锁/内核，不将DTO当成已实现控制API。新的
请求receipt/observed state、历史和控制端点仍须继续编写，不能据共享类型关闭R09。

## 后续必须继续的完整范围

1. 正式Neon管理SQL、内核receipt/history/cancel及共享Rust双连接客户端已完成本机
   模块验证；Compute HTTP/owned运行时已另完成，继续Monitor/Go实际客户端、外层持久
   历史/意图与跨进程重连，不把模块当完整链路。
2. 全部私有/并行/DSM/维护/扩展/后台预算与quiesce、持久承诺；Compute/Monitor/
   Agent/Scheduler/Controller/Runner实际执行fencing和恢复对账。旧Ok不能授权缩容。
3. Provider平台探测、显式HugeTLB实现/账本等尚未完成部分继续审计编码；缺代码
   不能归为待Linux测试。真实Neon Pageserver/Safekeeper/RW/RO/branch本地可行E2E。
4. PG16/15/14逐版完整维护/语义移植；PG18发行/AIO/SLRU适配。旧版本24次C头兼容
   仅为语法/API检查，不能称已支持DSGA。再落实Driver与Helm分支系列代码。
5. Mac可另配的可选测试矩阵与回归按影响继续执行。目标Linux物理回收/HugeTLB/
   KVM/VM迁移/K8s/性能长稳由后续目标环境认证，保持默认关闭和发布门槛。

## 不可破坏的恢复/环境约定

- 旧stash 6f002a31196f2e623e8386c38a649ff87be2ada4 已apply一次，禁止再apply。
- S3 stash f5c4dfc54b3600a4191b4a7f06d2437db3b8403b 已apply一次，禁止再apply；
  保留两份stash与build/v3-s3-upstream-stash-*一致性证据。
- repos/postgres仅为实际vendor/postgres-v17兼容符号链接，不另建独立PG实现。
- 不构建镜像、不准备或执行K8s部署、不合主分支、不发布生产；无授权不建子代理。
- 每批先核对相关官方维护，先推依赖再更新/推主仓，源码gitlink/两份锁/二进制SHA
  同一；中文关键注释及中文模块化提交。上个模块通过不能关闭全部持续目标。

## Rust双连接客户端接续（2026-10-20）

管理SQL提交ad1b26ff5657c7891bbacb2a1f25e8f773d7e0a8已推自有Neon功能分支并
核验GitHub refs；PG486338及三处锁不变。fresh fetch相关Neon/PG官方维护头仍为
当前状态页所列SHA且均为祖先。本批新增compute_api可选postgres-client、严格
事实类型及ci/test-dsg-rust-client.sh；详见DSGA-Rust双连接管理客户端.md。

真实原生8项14.19s通过（终态句柄8917），默认包compute_api32/compute_tools23
通过（63618），最终feature Clippy退出0（4265）；均已结束，不重启旧句柄。
feature默认运行的8项ignored不能算通过，严格脚本已显式全量实际执行。
日志build/neon-dsg-client-{packages,default-final,strict-final,clippy}.log。
SDK初次提交时未接入HTTP/Monitor；后续HTTP接入见下节，Monitor仍待实现，均无VM许可。

## Compute管理HTTP接续（2026-10-20）

SDK 276230c5c90f25c795fa5be33bfdd317161fca4b已推自有功能分支并核验GitHub refs。
本批新增五个正式HTTP路由、独立严格JWT、owned长期双连接/OnceCell、No-store、
完整OpenAPI和11项路由用例；PG486338及三处锁未变。fresh fetch Neon main/
Neon PG17/社区PG17均仍为当前状态页SHA且为功能头祖先。无主分支合并或发布。

真实修复：SDK原先只检查直接/继承schema CREATE，漏掉NOINHERIT但可SET ROLE
管理员成员边。新测试先验证实际权限组合，旧实现确定性失败1/1（其余7 filtered），
日志build/neon-dsg-http-noinherit-negative.log。修复后最终严格脚本完整SDK8项
12.27s/HTTP11项6.09s通过，零ignored/filtered（19071已终态0）。初始夹具SK重复、
拆分import和schema名称查询错误另保留过程日志，不冒充业务负向证据。

最新默认两包32/30通过，另SDK8/HTTP4默认ignored且已另行严格实测；两包
--no-deps全targets Clippy -D warnings通过，remote_storage既有27 warnings保留。
最终日志build/neon-dsg-http-{packages-final,strict-final,clippy-final}.log。Mac可执行
门禁不等于Linux/VM认证。模块文档DSGA-Compute管理HTTP与运行时接入.md。

本HTTP不自动重建失败客户端，不完成持久owner/租约/重连；Monitor、Go、完整预算/
quiesce、最终VM fencing、多PG版本和真实Neon存储E2E仍须继续编码集成。下一模块
从持久操作身份/唯一执行者及完整资源承诺推进，不重复构建未改的PG/C门禁，不把
剩余业务归到Linux待测。全Goal保持active，依既定边界不构建镜像/部署/合主分支。

## Autoscaling持久执行日志接续（2026-10-20）

已先提交并推送自有Autoscaling功能分支c5db379be22fa17ca8763b1b3c3aea90ca12b9da，
GitHub refs一致。fresh fetch官方main仍a4bce09e且完整包含。新增完整原意图/owner/
不可变业务命令、UID/RV条件写前日志、未知动作与接管/取消恢复、有界历史及新VM
显式初始化。受管标记存在时旧Agent/Controller/QMP内存入口拒绝回退，Webhook
保护标记；不自动启用，不把持久化票据当作VM许可或领导权证明。

最终Mac受影响race组合161pass事件，0fail/用例skip；9测试包通过、9无测试包
单列。fuzz实际35835次输入通过，v2lint0issues。RV/UID条件独立反向均失败，
finally逐字节恢复后完整门禁通过。句柄33634/2260/82192均终态0；65732反向
脚本终态0表示已观察预期失败并恢复，不是故障版本测试通过。日志见当前状态页。

当前新增Store仍需可信协调器初始化/驱动，Takeover只保存上层已认证决定，不自带
选主；实际领域事实核验、Go HTTP/Monitor、完整预算、PG issuer fence及最终
Runner私有QMP/持久串行隔离仍是代码剩余项。不能以模型观察包宣称物理完成。
下一批继续真实客户端与控制链，不重编未修改的PG；原子模块/两份锁仍486338，
Autoscaling的所有者AGENTS.md未混入提交。Goal保持active，不因本模块通过结束。

## 正式Go HTTP消费者与完整意图摘要接续（2026-10-20）

Autoscaling已编写五路由实际TLS消费者、双池/未知意图栅栏、严格ABI5事实及完整
context摘要，尚未接入Agent的持久运行循环；Go→真实Compute→PG整体门禁待做。
审计body32KiB/命令8KiB分开，与HTTP32KiB界限一致；null布尔/标量不得补零。
Go最终race183pass事件，0fail/用例skip，10测试包/9无测试包单列，v2lint0issues。
Scheduler既有闭区间随机测试实际误判已修复，保留失败，定向race100轮通过。
已先模块化提交f808bea/ee7f7df并推送，GitHub远程SHA为
ee7f7df4cd162c7dd9efa737d4d869a5f1ce5e4e；Autoscaling仅保留所有者AGENTS.md改动。

本仓Rust同补完整PG意图摘要比较。原检查的漏检有确定性反向1failed/9filtered，
finally已恢复；最初SDK链接失败另存，不能算反向成功。最终SDK8/HTTP11真实门禁
14.30s/7.46s、两包普通34/30、Clippy全targets无本包warning均通过。日志/边界见
DSGA-完整意图摘要跨语言核验.md。终态句柄58135退出0包含三步全成功，无运行故障
源码；PG486338/三处锁未变。依赖分支先推再登记SHA，以Git/远程refs核验为准。

后续继续可信owner与持久协调循环、实际Monitor和完整预算、PG接管fence与最终
Runner私有QMP串行隔离、其他PG版本/Driver/Helm及真实Neon存储E2E；这些仍是
代码/集成剩余项，不是Linux待测。Goal保持active，所有既定环境约定继续有效。

## Go到真实Compute/PG门禁接续（2026-10-20）

自有Autoscaling 9f4669f3a2a9990268f52b58d458a568ebf34480已先推送且GitHub
refs核验一致，仅保留所有者AGENTS.md未提交。新增显式tag的Go race真实消费者
用例，本仓严格宿主脚本/共享HTTP夹具运行；完整结果见DSGA-Go客户端真实链路验证.md。

已提交依赖准确复验1 Rust外层/2 Go进程4.53s、零skip/ignored/filtered，SDK8/
HTTP11共享夹具回归14.25s/7.19s、普通34/30、两包全targets Clippy、Go tag lint
均通过。17115多步流水线和98238准确依赖复验均终态0，不重启旧句柄。
PG/三处锁仍486338；不重编未改PG/C。初始移动夹具路径/可见性和dead_code错误
只算实现过程编译/lint错误，不能当业务反向证据。源码无故障版本留存。

此门禁不跑Monitor/Agent/Scheduler/Runner或完整Neon存储。下一代码工作继续
真实协调/预算、PG issuer接管fence及显式受控重连；不能靠重建空客户端丢旧意图，
也不能把拒绝静默重连当成已经完成恢复。整体Goal仍active，后续代码范围不缩小。
