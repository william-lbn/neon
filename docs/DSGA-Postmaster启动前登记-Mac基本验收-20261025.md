# Postmaster启动包前登记与Work2

沿原有自有服务分支演进，实际Neon `vendor/postgres-v17` 提交
`9db3b91d3caff9f674786252f0104de75ed8e709` 已先推送核验。其他PG暂缓，默认关闭。
本轮重新fetch Neon fa504217、AS a4bce09e、Neon PG17 56692dfb、社区PG17
19656b77均无新增。控制面完整主线4d86357独立合入5ba50b8，无冲突；新增及
受影响Mac Go24顶层/40含子项race、Web tsc通过。该维护测试使用私有control_ci
及准确旧PG108，571个输入摘要未变；不重复旧全套，不执行附带Linux部署收据。

## 不变量和修改

旧Work1只在InitProcess后计数；真实子进程已fork并接收连接，却可停在RAW/半
startup、SSL/GSS/DNS阶段，没有PGPROC，pg_stat_activity中也不存在，不能当空闲。

- Postmaster使用自身分配的固定PMSignal槽，在fork前先增加待启动计数。私有槽
  与原生32位原子计费入主共享段预算，不改Buffer ABI6、PGPROC或Work公开结构。
- Postmaster不等待backend的status/LW/资源锁，不从共享长度决定寻址范围。
  原生32位原子或硬件内存屏障不支持则拒绝启用，不能降为持共享锁模拟。
- 子进程仍在官方允许共享访问的InitProcess处，在同status_lock内从待启动槽
  CAS到PGPROC STARTUP，并扣原计数；观察不能看到二者同时为零。缺原登记失败
  关闭，不自动补造。背景/辅助进程仍按原生命周期，不误扣普通启动计数。
- 只有Postmaster实际waitpid回收或fork失败的原ReleasePostmasterChildSlot路径
  清理待启动槽，exchange使重复清理无效果。连接close/发送信号不是退出证明。
  早期child不访问新增共享区、不安装清理回调，保留官方超时/SIGTERM直接_exit。
- 受管容量/恢复/关闭拒绝在fork前直接关闭socket，不创建可无限停于启动包的
  未登记dead-end子进程。非DSGA保留原官方协议。临时拒绝可能EOF，控制方按
  原未知请求规则恢复；不主动断开既有连接，不冒充写请求NoEffect。
- Work2以相同字段显式扩大覆盖；正式Neon C、Rust/Go严格消费版本2，拒绝旧1
  或未知版本。Buffer ABI6、SQL签名及扩展1.12保持。必须按准确锁共同升级，不能
  将旧Work1的零计数用作启动前覆盖。固定Simple Query及Pending保留规则不变。

## 实际Mac证据

| 范围 | 实际结论及工作区build日志 |
| --- | --- |
| 旧准确108负向 | RAW已收连接、无PGPROC但startup为0，预期失败；`dsga-prestart-old-negative.log`，不计业务通过 |
| 新原生窗口 | 去重6项通过、0跳过；`dsga-prestart-native-fixed.log` |
| 准确PG重编 | 独立prefix `pg17-prestart-exact-install`，版本17.11 (9db3b91d3caff9f674786252f0104de75ed8e709)；`dsga-prestart-exact-version.log` |
| 准确SHA关键复核 | RAW与持锁SIGKILL恢复2项通过；`dsga-prestart-exact-native.log`，替换候选相应项，不加计数 |
| 官方受影响组 | 登记/交接候选启用DSGA core225/isolation123通过；`dsga-prestart-core.log`、`dsga-prestart-isolation.log` |
| 实际Neon C/Rust communicator | 准确PG配套重编安装通过；`dsga-prestart-neon-build.log` |
| 正式HTTP | 升级、权限、同实例及其他事务门禁1项通过；`dsga-prestart-http-native.log` |
| 实际Go跨仓 | Go race/TLS/生产Router/JWT/SQL/准确PG1项通过；`dsga-prestart-go-http-native.log` |
| 协议及质量 | Go更新协议/SDK race2项、Rust协议1项及相关vet/clippy通过；`dsga-prestart-go-work2.jsonl`、`dsga-prestart-rust-work2.log`、`dsga-prestart-clippy.log` |

六项覆盖RAW空包、半长度头、SSL/GSS协商、真实认证交接、官方启动包超时、无效
取消/畸形包、容量满前fork拒绝、真实锁所有者崩溃恢复。崩溃夹具是私有测试模块
专用SU函数：故意持status_lock后停止自身，Python确认本次backend PID、父
Postmaster及实际T状态后SIGKILL。若Postmaster回收错误获取同锁，恢复必失败；
当前真实reap/新boot/关闭门禁与旧启动槽归零均已确认。此故障函数不进入生产业务。

首轮遗漏回收调用点导致7个失败（含子例），`dsga-prestart-native.log`保留；补齐
后重新运行完整新增6项全部通过。core/isolation曾通过也没有用来掩盖漏回收。
最后只增加原子forget接线：正常已交接槽不改计数，早退/重复回收以新6项及准确
关键复核覆盖；未再次执行不变官方全套。没有以失败次数或重复复核增加通过数。

跨Go门禁真正收到GSS响应N后保持连接、不发startup包，Go必须读到Work2及恰好
一个启动在途；Native原快照不变、Pending保留、零写POST。TLS仅本机终结，不模拟
PGbody或使用用户数据库。CI明确收集本批6项，未继承旧8项来重复测试凑数。

## 未完成代码与目标环境边界

当前观察含所有已fork受管普通连接，但仍不是禁止关门后继续新fork的ack/持续
门禁或完整Quiesce。后台/并行/DSM/预备事务、允许有界的管理/检查点工作与持续
预算须另接真实生产者；不能停掉回收所需检查点而死锁。可信Prepare/Finalize与
system outside、首次准入、Host/Scheduler/最终Controller-Runner-QMP、公开worker/
生命周期及受控跨boot/owner未知关闭仍是代码，不能标成“仅待Linux”。

Linux原子/调度/进程限制、K8s/RBAC/NeonVM/KVM/cgroup2/HugeTLB、真实释放/迁移及
压力/性能长稳未验；Mac不能替代目标认证。Goal active，仅PG17，不构建镜像、
不准备/执行部署、不合主分支。准确版本绑定和摘要以同名Mac收据及远程分支为准。

最终发行索引18程序/4PG/39扩展阶段、相关5项单测通过，语法和原生新6项收集
检查通过；收集不算再次执行业务。PG gitlink及两份锁与已发布9db3一致，AS与
控制面共同绑定已发布e539f2f3；下载Origin Hash和readonly vet通过。准确SHA、
去重计数、源码与本机二进制SHA-256见[本轮收据](DSGA-Postmaster启动前登记-Mac收据-20261025.json)。
