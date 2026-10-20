# 所有权选主与持久PG循环验证

日期：2026-10-21。Autoscaling生产实现`1bf247d06c89d24f141d15f32cf6ca50116b2f0c`、
阶段门禁补强`75187718d7f4e99ed46924305e89ad83a943b0de`均已先推送且GitHub refs
核验。本轮Neon仅补强严格测试门禁/方案/验证记录，Rust/C/PG源码及三处锁不变：
PG17 6b7c3655/Neon1.8/Buffer ABI6，准确已安装二进制与锁一致。

Neon main fa504217…、Autoscaling main a4bce09e…、Neon PG17 56692dfb…、社区
PG17 91246c72…均再次fetch且已完整包含；不覆盖自有功能或制造空维护合并。
本批不是完整R03/全功能交付，Goal继续active，功能默认关闭。

## 本批真实实现

生产owner包使用锁定client-go v0.31.7选主，固定VM UID/incarnation、完整Lease
绑定和不可复用随机进程身份；Lease对象UID变化拒绝当前会话，更新携带UID/RV，
返回对象必须精确对应请求。LeaseTransitions不作为业务epoch，int32回绕先拒绝。
实际epoch从原journal CAS Takeover单调递增，丢回复只读回同一决定；原操作、
已发命令、未知效果和预算引用均保留，不重建空日志。

Session每次授权检查当前Lease及journal。接管/检查API各有期限，避免Lease健康
但VM读取卡住；取凭据还单独受Compute Timeout约束，因为Client.Do超时不覆盖
此前回调。长期Session不继承短期接管context。停止关闭异步回调登记窗口，等
已登记工作实际退出，不提前放Lease；失败/panic也不清未知状态或泄露私密错误。

PGWorker长期持有同一客户端，恢复原Pending、固定boot/代次，取消传播到真实
HTTP，即使调用方给background也有效。实际定时/唤醒循环只查询/业务取消未知
原请求，不自动重发POST；显式Replay匹配持久原Key，closed通道不忙轮询。旧SDK
未知意图不允许导致新步骤被错误登记，跨owner/boot关闭仍等待受控完整协议。

Lease选择并不提供最终fencing。Check与派发之间还可能换主；PG及最终Runner
必须继续独立单调隔离/串行化、查明已发VM动作。签发远端凭据也必须验证真实主体
和完整绑定，不能由调用者数值或本机Session对象充当远端认证。

## 实际链路及门禁约束

Go生产Lease选主→原journal接管epoch2→生产PGWorker循环→TLS→生产Compute
Router/JWT验签→长期双SQL→准确PG，实际2048→4096 grow。PG已提交后注入第二次
503丢回复，循环从原日志/独立查询持久落账，只有显式原意图重放才第二次POST。
随后仍校验旧回执/当前容量分离、600行超bootstrap数据、真实PG重启拒静默重绑。

Kubernetes是本机typed client/HTTP/RFC6902/UID/RV协议夹具，非PG前后步骤是明确
test_only body；没有模拟PG事实，但也没有运行真实apiserver/etcd/RBAC、预算、
Host/Guest/VM或物理释放认证。不能把这些夹具作为生产资源许可来源。

严格脚本现在要求唯一的实际阶段标记，内容为epoch2、4096 buffers、journal已
落账和显式重放已确认；标记在全部阶段断言成功后输出，不含凭据或随机holder。
因此旧Go程序仅有两次顶层PASS也无法满足新门禁。对同一成功日志仅删除该阶段、
保留PASS的反向被同一验证器拒绝，完整日志通过；不是只靠搜索测试名称声称覆盖。

## Mac实际证据

| 检查 | 结果 | 边界 |
|---|---|---|
| 最终受影响race组合 | 254 case pass事件，0fail/用例skip | 12测试包/9无测试包；含父子/fuzz事件 |
| 本模块 | 15顶层/20子测试，共35事件 | 增量不重复计入原219 |
| native标签全部受影响组件lint | 0issues | go.mod/go.sum不变 |
| 最终源码真实跨仓门禁 | 1 Rust外层/2 Go race，8.49s通过 | 零skip/ignored/filtered |
| 带阶段硬约束的门禁 | 候选8.09s通过 | 准确7518771依赖再重编，结果见pinned日志 |
| 生产代码反向 | 8项均实际断言失败并逐字节恢复 | 不以编译错误/夹具修正计数 |
| 门禁反向 | 缺owner阶段但保留PASS的日志被拒绝 | 与生产反向分列 |
| 脚本/版本/格式 | Bash、diff、准确PG绑定检查通过 | 未改PG/C，不重复旧world |

准确`75187718d7f4e99ed46924305e89ad83a943b0de`重编复验8.11s通过，Go正常
4.05s/重启后0.01s，阶段标记恰好一次，参与编译的Go业务源码干净；见pinned日志。

8项反向分别移去固定Lease UID、实际join、journal owner核验、Session取消链接、
旧SDK意图屏障、int32写前溢出检查、VM读独立期限及凭据回调期限。恢复后生产
owner.go/lease.go/pg.go SHA256依次为
e561887fc532223248e4a768ec29a35d203564c9bad65302e48ad4f883fff462、
cb926b8924c355e52cbb4e2f36e67010a7318261ec23e966b633d90df1ca55d8、
f901e5b4af7d50ad59813930e621543518bfd13f60a357e83681a4d635999206；随后完整race重验。

工作区build日志：autoscaling-owner-{negative-final.log,full-final.jsonl,
full-final.stderr,lint-final.log}，neon-owner-native-{final,phase,pinned}.log，
neon-owner-phase-verifier-negative.log。早期HTTP取消夹具body/context问题保留在
owner-pg-first/http-cancel日志，不当作生产缺陷证据；既有envtest缺etcd的0/2失败
仍单列。Rust依赖已有27条warnings不混作本批Clippy/业务失败。未改全局工具链。

## 继续实施范围

尚需接Agent实际受管VM入口与凭据broker/Driver、受控跨owner/boot SDK关闭协议、
全部硬预算/quiesce及清理、Monitor、Scheduler/Controller/最终Runner私有QMP及
资源承诺生命周期；其他PG、Driver/Helm完整产品接线和真实Neon存储E2E也继续。
以上是代码/集成剩余项，不能改称只待Linux。Linux/K8s/KVM/HugeTLB/物理回收/
迁移/性能/长稳后验。本轮不构建镜像、不准备部署、不合main、不发布生产。
