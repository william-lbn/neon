# Neon 管理SQL与固定实例身份

日期：2026-10-20。依赖PG17 `48633803b05c1b7bd9a40d749b5278df9aaf01ac`、
Buffer ABI5；在现有Neon扩展中实施，不是独立PG项目。扩展从1.6升级至1.7，
同时提供反向升级脚本。新函数默认拒绝PUBLIC，并在C中复核权限。

## 已实现的管理入口

| SQL函数 | 输入/行为 |
|---|---|
| `neon_dsg_capabilities()` | 控制协议、Buffer ABI、是否启用/绑定、历史上限、Provider能力 |
| `neon_dsg_memory_status()` | 带完整实例身份的当前PG/数组预算与回收事实 |
| `neon_dsg_request_resize(text)` | 严格解析完整不可变意图，调用真实内核协调器 |
| `neon_dsg_resize_status(text)` | 按完整实例/boot/epoch/revision/opID查询独立回执 |
| `neon_dsg_cancel_resize(text)` | 登记同一操作的业务取消；不发送进程信号 |

函数属于实际安装扩展的schema，管理客户端必须解析扩展schema并安全限定函数名，
不能信任租户search_path。输出为JSONB；输入特意是**text**，不能由客户端/SQL
先转JSONB，丢掉重复字段后再交给函数。所有入口均为VOLATILE/PARALLEL UNSAFE；
状态不随SQL事务回滚。降级只移除SQL入口，不撤销共享容量/历史/已计入成本。

普通租户和Neon弱superuser不能调用。ACL误授或SECURITY DEFINER也不能绕过：
C同时检查当前角色与会话的真正PG superuser；内部cloud_admin保留管理能力。
管理入口尚不代表HTTP认证/外部issuer/完整预算校验已经接入，详见下文边界。

## 解析、身份与回执

输入与Rust `compute_api::dsg_control`协议1一致，使用相同JSON测试向量。
所有64位身份/字节/期限用规范ASCII十进制字符串，拒绝数字强转、前导零、符号、
空白、指数、Unicode数字和溢出。协议版本/Buffer数是小范围JSON整数；先检查
原始数字token，防止PG JSONB把`1e0`规范化成`1`后误接受Rust不接受的请求。

解析先限制16KiB，再用原生JSON解析检查数字语法；构建JSONB时启用每层唯一键
检查。重复字段（含Unicode转义后相同的键）、未知/缺失字段、数组/标量/NULL
类型不符均拒绝，不以宽松map/最后一个键获胜归一化。所有ID为1–128字节可见
ASCII；输出由原生JSON转义处理，允许ID中合法引号，不能直接拼入JSON字符串。

固定身份来自Postmaster参数`neon.dsg_vm_uid`和`neon.dsg_compute_instance_id`。
缺绑定时capabilities明确instance_bound=false，管理操作拒绝；不能猜测VM名字、
endpoint ID或沿用备份身份。请求外层实例不符返回属于原请求的STALE receipt，
observed仍带实际当前实例。即使IDLE，快照也携带完整VM/Compute/PG boot身份。

经唯一键/类型/范围校验后，将完整请求的原生JSONB规范序列化计算SHA-256，绑定
内核意图。键顺序、空白和等价转义不改变绑定；同ID改VM目标、承诺引用、期限、
方向/Buffer目标/generation都不能冒用已有结果。此hash只用于本boot内语义绑定，
不是跨语言签名标准、身份认证或资源许可；普通请求不能自行提供可信hash。

receipt与observed分别携带自身身份，has_result=false时result为null。数组的
release_key绑定自身操作，generation/字节数保持字符串。Provider尚未返回的
NONE/PENDING/WAIT_CHECKPOINT状态明确has_result=false/status=unknown，不能因
共享结构默认枚举0而虚报OK。真实NOOP、已验证/不支持/失败各自保留回收事实。

startup_shared_upper_bytes包含启动主共享段与bootstrap数组，不能直接与当前
各数组上界再次求和，否则会重复计入启动数组。完整运行时分类账/预算仍须另行
实现，不能从这份局部快照拼出私有/后台内存已经受到约束的结论。

新请求按Unix毫秒期限计算有界剩余等待，检查纪元加法/无符号下溢，最多60s；
历史查找优先于新执行期限。已过期的合法原意图仍可查询/重放，不能改期限冒用
同一ID。业务取消的终态、ERROR/FATAL和SQL ROLLBACK语义沿用已验证的内核契约。

## Compute受管启动契约v2

DSGA启动对象显式升级至v2，新增必填`vm_uid`和`compute_instance_id`；未启用
DSGA的普通ComputeSpec保持兼容。旧实验v1缺身份，不能自动补空或从名字推断，
必须补身份并受控重启。控制请求协议仍为1；两者是不同命名空间。

身份由认证控制面分配并绑定实际VM/Compute执行实例；Compute在生成配置前验证，
写入配置时使用现有escape_conf_value，启动使用原生Command参数数组的`-c`，
不经过shell。与小池/上限/预算一样，命令行优先于auto.conf和末尾include；
SIGHUP/HTTP reconfigure/HCC refresh拒绝改变整个启动契约及实例身份。

新测试发现原生custom GUC占位符重放遇非法值可能只WARNING并回退默认。check
hook仍沿用原生返回协议，额外记录初始化期间拒绝；注册完成后显式拒绝启动，
不能静默变成空/旧身份。正常backend延迟加载使用ERROR。非法空白/超长/Unicode
三项在旧已安装候选上独立启动均失败于“错误地启动成功”，保留反向日志；不是
只验证语法、也不把already-running当成其他坏身份被正确拒绝。

## 验证证据和发布边界

最初实际SQL七项通过13.558s，扩大后二十项通过35.272s。严格组合首次45项发现
上述GUC回退问题，之后已修复并补独立负例、发布后ERROR和FATAL跨回执测试；
最终完整清理/构建/安装/加载后47项通过85.043s，零skip（原22+正式SQL25项），
脚本31469退出0。最终prepare129项为13通过/116环境skip，69项内核及47项扩展已
另行真实执行。旧失败日志不删除；Native off/on门禁沿用准确486338已完成记录。
BUSY用例随后将客户端启动余量由100ms放宽至2s，仍验证真实串行竞争而非启动
性能；仅重验该用例，其他业务代码没有变化，结果见neon-r09-api-busy-margin.log。

- Rust compute_api全包24项、compute_tools全包23项通过，零failed/ignored。
- 两包`cargo clippy --no-deps --all-targets -- -D warnings`退出0；依赖仍有既有
  remote_storage 27条warnings，不宣传整个工作区零警告。
- 新C在PG17及旧PG14/15/16头上严格-Werror语法检查；旧版本只证明拒绝能力路径
  可编译，不证明已移植DSGA。PG17实际构建完整Neon/Rust communicator并加载测试。
- 真实SQL覆盖升级/降级、G→S→G/历史、完整意图hash、嵌套重复键/整数词法、
  超大/畸形/缺失输入、uint64极值、BUSY/取消/Provider pending、权限/definer、
  rollback/ERROR/FATAL、受管命令行优先级及缺身份/坏身份/禁用能力。

日志位于工作区`build/neon-r09-api-*.log`。原始无SDK编译失败也保留；显式使用
固定Xcode clang/MacOSX14.4 SDK后实际重编，不能据环境修正声称业务已通过。

**本接口只交付PG逻辑与Provider事实，始终返回vm_shrink_permit=false，当前
capabilities.production_ready=false。** budget_commitment_id只是完整意图绑定
引用；认证调用方仍必须验证持久承诺内容、当前issuer及完整资源上界，不能把
填了一个ID当成已证明预算。外层持久历史、全量私有/后台预算和quiesce、真实
Compute/Monitor/Agent/VM最终执行fencing、HTTP安全路由与其他PG版本继续编码。
真实存储本机端到端也须继续，不能把这些代码/集成缺口归为Linux待测。
Linux物理回收/HugeTLB/KVM/NeonVM/K8s与目标性能认证按既定边界后验；默认关闭，
不构建镜像、不部署、不合主分支、不发布。
