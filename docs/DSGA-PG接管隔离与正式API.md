# PG issuer接管隔离与正式API

日期：2026-10-21。按V3继续实施，**完整功能尚未完成，Goal active**。本模块将PG
单调接管隔离接入正式Neon扩展、Rust双SQL SDK、Compute HTTP及Go消费者；不发放
VM缩容许可、不以本机通过代替生产认证。

## 版本及部署约束

PG17先推送`6b7c3655ab74062e117eef7400953cd000679a49`，三处gitlink/版本锁均绑定
该准确SHA；实际重编二进制输出`PostgreSQL 17.11 (6b7c3655ab74062e117eef7400953cd000679a49)`。
Buffer ABI由5升6，Neon扩展由1.7升1.8。新增Snapshot必填issuer_floor_epoch、
独立Fence类型/第六管理函数，客户端严格要求ABI6和六个真实扩展成员签名。

必须一同重编安装PG、Neon扩展和配套C测试库，不可混用旧.so与新结构体/新符号；
ABI协商不是跨结构体二进制兼容。已有运行中的旧实验实例须受控重启升级，不能
SQL升级一个旧进程便宣称拥有新内核。1.8→1.7只移除新增SQL，不重置PG floor或
历史，也不允许旧ABI客户端绕过新隔离。

fresh fetch核对Neon main fa504217…、Autoscaling main a4bce09e…、Neon PG17
56692dfb…、社区PG17 91246c72…均未新增且为当前功能头祖先。不是仅挑若干bug，
没有覆盖自有改动；其他大版本仍须独立维护/移植及实际运行验证。

## 内核保证

同一PG boot内独立issuer_floor_epoch只增不减。接管在状态锁下推进floor；新操作
在持有resize排他锁时，在**同一状态锁**中核验floor、归档旧终态并登记操作。
旧请求不能利用前置检查与登记之间的窗口越过接管；Grow/Shrink提交前再核验。
旧epoch精确保留历史仍可重放，未知低epoch请求为Stale。epoch耗尽不回绕，实际
新boot才重置；原始epoch0桥接只在尚无非零floor时兼容。

接管请求取消可取消的旧PREPARING，但不撤销已发布容量。释放状态锁后条件获取
resize排他锁，忙则Pending；成功获取后对账FATAL遗留、再次检查floor并读取快照。
严格遵守resize→status锁序，不跨等待持status锁。COMMITTED可能仍在同步Provider
执行窗口，不能据此返回Fenced。详见PG `doc/DSGA-issuer-fence.md`和代码中文锁序。

Fenced仅证明PG串行resize/同步Provider已退出；Provider是否物理成功仍须分别读
事实。它不排空普通SQL、不证明VM/Host/Runner结束、不清旧预算或跨层未知意图。
接管不会改变L/H、容量generation或直接发布任何VM许可。

## 正式入口及错误语义

- `neon_dsg_fence_issuer(text)`：完整原始JSON严格解析，共用16KiB、唯一键、
  整数token、类型/范围校验；真实管理员会话；固定VM/Compute/PG boot绑定。
  非零epoch，错实例只返回Stale，不推进floor。PUBLIC无执行权。
- `POST /dsg/v1/issuer/fence`：外部路由专用JWT验签/时效/aud/scope/Compute绑定；
  接管epoch必须**等于**签名epoch，历史查询的低epoch规则不适用于接管。
  内部服务路由404，普通Hadron豁免不能访问。请求协议仍1。
- Rust SDK用独立observer SQL连接，Go用独立observer HTTPS池。发送后错误/超时
  保留完整接管意图的不确定性；NotSent与Unconfirmed分别表达实际发送边界。
  成功Fenced也不清resize pending；旧意图必须继续外层持久对账。Compute现有
  OnceCell断连/换boot仍失败关闭，不静默构造空客户端。
- 响应为requested_instance/requested_epoch/disposition/observed。Fenced和Pending
  要求精确同实例/floor，Stale必须有更高floor或实例差异；Pending允许已COMMITTED
  而Provider仍在途。所有Snapshot操作/释放epoch不得超过floor。

JWT数值绑定/PG floor不是选主或租约认证。可信owner分配、终端Runner隔离、跨层
预算与资源承诺仍须继续实现。production_ready=false、vm_shrink_permit=false不变。

## Mac实际结果及复现

| 检查 | 结果 | 边界 |
|---|---|---|
| 准确6b7源码重编+内核严格门禁 | 80项通过120.900s，零skip | 原69+新11，不重复计数 |
| 完整Neon clean/build/install/加载 | 52项通过78.346s，零skip | 22项存储/30项正式SQL，含真实升级降级 |
| Rust真实双SQL SDK | 10项通过14.42s，零ignored/filtered | 新增接管及旧阻塞取消/未知意图保存 |
| Compute正式HTTP严格门禁 | 12项通过6.08s，零ignored/filtered | 8项认证/路由和4项原生/TCP |
| Go真实HTTP→SQL→PG | 1 Rust外层/2 Go race进程通过4.19s | 候选Go源码dirty如实输出；提交后另复验 |
| 两Rust普通包 | compute_api36/compute_tools31通过 | 默认SDK10/HTTP4/Go1共15ignored已另严格执行 |
| Rust Clippy | 两包all-targets、client feature、no-deps -D warnings退出0 | 依赖remote_storage既有27 warnings单列 |
| Go完整受影响race | 185case pass事件，0fail/用例skip | 含父/子/fuzz种子，10测试包/9无测试包 |
| prepare发现 | 145项，13通过/132环境skip | 80+52已经严格实测，不能把skip算通过 |
| 锁绑定/格式/脚本 | index严格版本验证、Ruff、pgindent、Bash、diff通过 | 其余PG版本不是功能支持证据 |

Autoscaling准确依赖`831a453e3adaa86f163027a1f7319139f78b55c5`推送后再次重编
真实Go门禁通过4.16s（两个Go进程0.17s/0.01s），参与编译的业务路径干净，日志
neon-issuer-pinned-go-native.log。另补内部服务Fence 404断言后定向用例和Clippy
通过，日志neon-issuer-internal-route-final.log；没有把旧全量日志冒充最后新增断言。
新C同时在PG17和PG14/15/16旧头下严格-Werror语法通过，日志neon-issuer-c-syntax.log；
旧版本仅编译不支持分支，不表示DSGA已移植。

全PG off/on world+TAP在提交前同源候选上各成功：各48份TAP报告，231报告文件/
18410报告断言；含可选/平台skip，不能写成全部断言执行。候选与6b7提交的src
二进制diff SHA256均为`2f0d6c4766888773a1a612c2ad05f29e7ee8f6073704a6726cc533866e38c834`，
相差的新增文档不影响源码。准确SHA重编后另执行以上严格业务门禁，不混淆来源。

反向测试实际移除登记时floor检查，Grow/Shrink两个竞态用例均失败（旧请求被
执行）；另将COMMITTED误当Fenced，Provider阻塞用例失败。finally逐字节恢复，
最终Control源码SHA256为`9dbbc8b41d1ee54938d567bedb6fed2e3b0df6dac1817d352b146050a4d85f0e`。

工作区build日志：neon-issuer-exact-{build,control,extension,rust-native,go-native}.log、
neon-issuer-{rust-units,clippy,prepare,native-negative,source-match}.log；候选world
为neon-integrated/v17/dsga-issuer-world-candidate/{off,on}.log。顺序流水线17805
已终态0，禁止重复启动旧句柄或重apply stash。Mac沿用Rust1.88/Go1.25.13、Xcode14.4
SDK/clang/CGO、build/cargo缓存，不改全局工具链。

复现先确保准确PG源码及三处锁一致，依次执行已有脚本：

```sh
bash ci/test-dsg-postgres.sh PG_BUILD_DIR PG_INSTALL_DIR
bash ci/test-dsg-neon-extension.sh PG_BUILD_DIR PG_INSTALL_DIR
bash ci/test-dsg-rust-client.sh PG_INSTALL_DIR
bash ci/test-dsg-go-client.sh PG_INSTALL_DIR AUTOSCALING_SOURCE_DIR
cargo test --locked -p compute_api -p compute_tools
cargo clippy --locked -p compute_api -p compute_tools --all-targets --features compute_api/postgres-client --no-deps -- -D warnings
```

## 继续实施范围

完整V3仍需可信唯一owner和持久协调循环、Monitor接入/受控重绑、全部硬预算/
quiesce与承诺生命周期、Scheduler/Controller/最终Runner私有QMP及跨重启迁移
隔离、其余PG版本、Driver/Helm功能与真实Neon存储本机E2E。以上是代码剩余项，
不是只待Linux测试。Linux/KVM/HugeTLB/物理回收/K8s/性能认证后验；本轮不构建
镜像、不准备部署、不合并主分支、不发布生产。
