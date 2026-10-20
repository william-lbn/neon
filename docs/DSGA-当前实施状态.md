# DSGA 当前实施状态

更新：2026-10-20（Asia/Shanghai）。V3为实施标准；**整体仍未完成，Goal保持
active**。本页为当前事实入口；阶段性通过不能替代完整交付。默认关闭，不合
自有主分支，不构建镜像、不部署K8s、不发布。

## 当前已经实现的范围

- PG17普通页Provider、固定地址四数组目录、L/H/M边界、Grow、尾部排水与逻辑
  Shrink、逐数组保守回收/regrow；pin/IO/CV、checkpoint工作区与异常恢复保护。
- 安全小池启动、共享布局预算核验、同Postmaster恢复换boot、受管新boot默认
  关闭普通准入，保留真正管理员的恢复通道。
- 内核R09：独立receipt/observed、当前操作外16条终态、epoch/hash绑定、过期
  拒绝、实际业务取消、FATAL遗留对账、完整逐数组释放身份。Buffer ABI5。
- **现有Neon扩展1.7的五个正式管理SQL函数**：capabilities、memory_status、
  request_resize、resize_status、cancel_resize；真正superuser会话检查，严格
  原始JSON/唯一键/类型/范围校验，完整意图hash及固定VM/Compute身份绑定。
  这些是已加载验证的函数，不是以C测试桥替代管理端点。
- Compute启动契约v2：必填VM UID与Compute实例ID，配置/原生启动参数同时固定；
  备份auto.conf/include不覆盖当前绑定，重配不静默改变身份。普通静态spec兼容；
  旧实验v1须补齐身份后受控重启。
- 共享Rust管理客户端：严格响应类型、真实双SQL会话、可信schema/扩展成员核验、
  超时/future取消后保留完整原意图、独立状态/业务取消。已实际运行8项原生集群
  用例；已owned接入Compute HTTP，Monitor仍未接入。另拒绝NOINHERIT但可SET ROLE
  管理员的成员关系，保留旧校验漏拒绝的真实负向证据。见[客户端模块](DSGA-Rust双连接管理客户端.md)。
- Compute五个正式HTTP路由与OnceCell长期双连接：严格专用短期JWT认证，不沿用
  Hadron免认证；绑定Compute/VM/实例/epoch，限制原始JSON，禁止缓存；配置期间
  仍可查询/取消在途操作，后端断连不隐式换客户端。11项实际路由门禁通过，含
  4项原生/TCP链路。见[HTTP模块](DSGA-Compute管理HTTP与运行时接入.md)。
- Autoscaling自有分支已真实接入Agent固定UID/Runner身份、动作原资源快照、
  UID/resourceVersion条件写入及响应核验；保留同Revision分批批准。另完成Go
  请求/查询的无损完整身份契约，以及完整意图/命令的持久执行日志、写前条件提交、
  未知动作/接管/取消恢复与有界历史；受管标记禁止旧路径回退。Go正式消费者已
  编写，且到真实Compute/PG的整体门禁已执行；可信owner分配与日志协调循环仍待集成；
  账本不是租约认证或VM许可。
- 完整Neon扩展的共享LSN锁、独立SLRU接口、安全文件发布与Buffer提示保护；
  原生恢复/逻辑槽/配置样例/资源释放缺陷修复及保留的反向证据。

管理SQL只返回PG/Provider事实，**vm_shrink_permit始终false、production_ready
当前false**。预算引用不是预算证明，PG提交不是VM完成；外层完整资源承诺、issuer
及最终执行约束须继续实现。详见[SQL模块](DSGA-Neon管理SQL与实例身份.md)与
[R09内核接入](DSGA-R09-内核回执与取消接入.md)。

最新批次补齐Go正式五路由消费者/双连接池/未知意图栅栏及ABI5严格事实类型，
拒绝null标量补零，并与Rust一起重算完整PG context摘要；不能借用同Key下不同
VM目标/预算/期限的回执。见[跨语言摘要核验](DSGA-完整意图摘要跨语言核验.md)。
Go客户端尚未接Agent协调循环；Go→真实Compute→PG同一门禁已另完成，含实际
提交丢回复、原意图重放、数据校验及真实PG重启拒绝静默重绑。见
[真实链路门禁](DSGA-Go客户端真实链路验证.md)，它不等于完整Neon存储/VM端到端。

## 分支、源码与上游

| 仓库 | 分支/身份 | 状态 |
|---|---|---|
| 实际PG17子模块 | codex/dsga-pg17-v3-20261019 / 48633803b05c1b7bd9a40d749b5278df9aaf01ac | 已先推送并核验GitHub refs，工作树干净 |
| Neon主仓 | codex/dsga-neon-v3-20261019，gitlink/两份锁均486338 | 管理SQL ad1b26f、SDK 276230c已推送核验；本HTTP接入批次随当前记录提交推送，恢复以Git/refs为准 |
| Autoscaling | codex/dsga-autoscaling-20261019 / 9f4669f3a2a9990268f52b58d458a568ebf34480 | 日志/HTTP消费者及真实Go联调已推送核验；完整运行链仍未完成 |
| control-plane | codex/dsga-control-plane-v3-20261020 / 928eec95… | Driver等运行时尚未完成 |
| Helm | codex/dsga-neon-helm-v3-20261019 / 648488a… | 本业务配置/验收链尚未完成，不准备部署 |

本HTTP批次再次fresh fetch：Neon main仍fa504217c61bbcaf5c512d75830564541f917f8f，
Neon PG17仍56692dfb680281a963c7470fc7f0fec7f65ecfd4，社区PG17仍
91246c72695cdddf62003f6bf4c4ade511f134f4；均为对应功能头的祖先。维护合并
4a39412f…/873715d889保留；没有只挑选部分bug提交或覆盖自有功能。其他版本
移植前须分别重新核对，不能称PG14–16/18已支持DSGA。

repos/postgres仅为实际repos/neon/vendor/postgres-v17的兼容符号链接。
主/稳定分支未合入本业务，见[同步规范](上游同步与功能分支规范.md)。

## 当前Mac验证证据

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
