# 启动硬约束与原位PG出生的Mac基本验收

本批按V3第8.3节补齐实际启动限额核心及受管PG启动调用点，依赖AS
`c5f5f7eb24827a81b067c5bb964604bc1fcc8590`已先推自有汇总分支并核对远程，发行
锁随后绑定。本批PG源码未改，gitlink与两份PG17锁仍准确cbabd5de541；仅当前PG17。

AS新增PG不存在时的BootstrapCommand，真实执行固定memory.max/禁swap/整个池
约束，空组核验、写前持久原意图、未知只读对账及当前floor显式原样重放。原
Bootstrap仍保留；运行后不能用旧大启动额度覆盖当前Held。仅floor的旧私有日志
可读，但不能授权新PG出生。完整细节在AS同名中文验收文档。

Neon `ComputeNode::postgres_command`为sync-safekeepers、临时启动及正式启动
统一选路：受管Linux固定`/neonvm/bin/dsga-resource-guard launch-postgres`，固定
生产PG路径，缺能力不回退cgexec。帮助程序入固定组后清能力/全线程降权并原位
exec固定PG，保持Child PID/信号/wait与实际PG相同，不引入sudo父进程。
普通postgres真实UID只能用该出生入口，资源写接口必须真实root。持锁核验当前
Held和实际约束、无未知动作、空组；CLOEXEC后PG不长期阻塞资源锁。

Linux已有受管标记但未冻结DSGA spec时也在启动前拒绝，不能绕回非受管路径。
标记缺失不会替受管spec授权，root帮助程序仍必须实际核对原配置/预算。

受管Compute跳过官方无创建预算的临时512MiB预热池；root标记存在但spec未到也
只跳过预热，不因此授权启动。Mac仍用直接实际PG路径，便于基本逻辑验证。
帮助程序出生只启动PG，普通SQL仍受既有关闭准入控制；没有VM许可。

实际检查与证据：

- Rust新argv/路径门禁和预热预算两项通过，0fail/ignored，26项未选；clippy通过
  （原依赖警告保留）。最终3个生产Rust文件在检查前后摘要一致。
- 受影响的真实Mac `ComputeNode::start_postgres`→初始化/HBA/原角色用例1项通过，
  0fail/ignored，27项未选，1.93秒；准确PG cbabd5de541及Neon1.11。临时实例正常
  停止。只复验这条修改了的实际启动路径，不重跑未变PG/SDK/Router套件。
- AS最终新增3项与受影响7项race共10项通过，0fail/skip；12个Go文件摘要稳定，
  vet通过，Linux amd64/arm64静态程序编译通过。文件/内核/exec边界夹具分清，
  不称作真实SUID、cgroup、内核或NeonVM端到端。
- 日志在工作区`build/dsga-bounded-birth-rust/`、`build/dsga-bounded-birth-go-final/`；
  `native-start.log`末尾的worker FATAL来自管理员停止夹具，不是用例失败。

补充Linux模式不一致门禁后，只复验新增两项unit/clippy，通过；最终源码清单在
`build/dsga-bounded-birth-rust-final-policy/`。实际Mac启动用例使用补门禁前输入，
输入摘要另保留`native-start-source-sha256.json`；后补代码只在Linux分支执行，
Mac实际启动分支未再改变，依所有者要求不重复该用例。两轮unit不重复累计。

最终复核clippy日志发现3个新链式表达式告警：分析器把`.postgres_command()?`
误计为已spawn之后的提前退出。已将三个入口明确拆成“先可失败地构造Command，
再spawn”，没有屏蔽lint，未改变命令/参数/启动顺序。最后只做受影响clippy编译，
exit0，compute_tools无新增告警；原remote_storage的41条依赖告警保留。
证据`build/dsga-bounded-birth-clippy-separated.log`及
`build/dsga-bounded-birth-final-compiled-source-sha256.json`。这次等价拆分不重复
已有功能测试；前述各阶段输入与日志均保留，不将它们说成最终源码一次性全量通过。

仍须接线的**业务代码**：原VM可信sysinit配置/关闭旧根组迁移权限、避免旧
cgconfigparser重新委派限额、root程序/配置安装权限、原启动额度生产者与独立认证
root通道、完整预算/独立启动准入/排空/Host与最终VM执行、公开worker及生命周期、
跨PG/Guest boot和旧owner部分写的受控关闭恢复。当前旧Guest配置会被明确拒绝，
不能不接线就直接启用Linux受管功能，也不能把这些业务缺口列成Linux待测。

接齐后Linux目标环境验证实际安装/SUID/nosuid、全线程/保存身份与能力、迁移和
进程遗留、PID/信号/wait、SIGKILL锁恢复、swap/HugeTLB/物理回收及完整K8s/性能。
本轮没有构建镜像、安装特权文件、准备/执行K8s、合main或发布；Goal保持active/
incomplete，默认关闭。
