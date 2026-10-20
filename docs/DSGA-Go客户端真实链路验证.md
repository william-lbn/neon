# Go 客户端真实链路门禁

日期：2026-10-20。依赖自有Autoscaling功能分支
`9f4669f3a2a9990268f52b58d458a568ebf34480`，已先推送并核验GitHub refs。
Neon/Autoscaling官方main重新fetch仍fa504217…/a4bce09e…，均已完整包含。
本批只修改测试/夹具/脚本/文档；无生产代码、PG子模块或两份版本锁变更。

## 实际执行链

`ci/test-dsg-go-client.sh POSTGRES_INSTALL_DIR AUTOSCALING_SOURCE_DIR`显式编译
带`dsga_native_e2e`标签的Go race测试程序，再由Rust `dsg_go_http`真实门禁驱动：

Go生产Client→本机TLS测试终结点→真实Compute外部Router及JWT验签中间件→
Compute OnceCell长期双SQL客户端→准确PG486338和已安装完整Neon1.7扩展。
不是人工响应夹具。TLS终结点没有放宽生产Go Client的HTTPS/证书要求；向Rust
固定loopback端口转发，禁用环境代理。一次性签名密钥不读用户凭据，JWT只经匿名
stdin管道，不进argv/env/日志/账本。子进程超时或用例退出会清理本次服务/PG；
失败保留私有PG日志/数据，未使用用户现有数据库。

验证内容包括旧scope有效签名被拒绝、1024→256→2048 Buffer扩缩、包含引号/
反斜线/HTML字节的完整意图摘要、历史result1024与当前observed2048分离、实际
TooLate取消、同Key改预算Conflict。PG/HTTP结果均不能发放VM许可。

只注入一次故障：第一次实际PG完成的resize回复改为503。Go必须保留完整原意图，
独立查到真实Committed，拦截冲突新请求，显式原意图重放得到Replayed；不透明
重发POST、不用查询成功自动清栅栏。Rust在Go结束后直接SQL校验600行count/sum/
MD5，数据大小超过2MiB bootstrap。随后真实重启PG并保留原Compute OnceCell，
确认旧服务503，另一个Go进程仍不能静默重绑；重启后再次直接校验数据一致。

旧`dsg_http`的Signer/Node/Router/native初始化提取到tests/support/dsg_http.rs，
保留原验签算法、Hadron绕过负向设置、私有PG逻辑，两个门禁复用同一生产入口。
这没有复制或替换生产路由。重构后的原有11项已全部重新执行。

## 复现与实际结果

Mac：Go1.25.13 darwin/arm64，Rust1.88.0；显式Xcode14.4 SDK/clang/CGO sysroot，
Rust缓存CARGO_HOME为工作区build/cargo。没有修改全局工具链或升级项目依赖。
脚本在编译前验证PG源码锁/二进制SHA一致，输出Go HEAD、相关源码dirty状态和Go
版本；提交前候选不是只凭旧HEAD冒充已提交版本。提交依赖后又从准确9f4669f重编：

- 新严格门禁：1个Rust外层/2次独立Go race执行，0failed/ignored/filtered/skip。
  已提交依赖复验4.53s，Go正常0.19s/重启后0.01s。
- 共享夹具的原有严格门禁：SDK8项14.25s、HTTP11项7.19s，零ignored/filtered。
- 两包普通全量：compute_api34、compute_tools30通过。默认SDK8/HTTP4/跨语言1
  共13ignored，已在上两项真实执行；零项doc-test不计通过。
- Go带构建标签的compute包lint0issues；两Rust包全targets、client feature、
  `--no-deps -D warnings` Clippy退出0，remote_storage既有27warnings单列保留。
- Bash语法、diff检查通过。无新依赖或PG/C变更，不重复执行未改的完整PG world。

日志位于工作区build：neon-dsg-go-native-{pinned-final.log,strict-final.log,
prior-strict-final.log,packages-final.log,clippy-final.log}及
autoscaling-dsga-native-lint-final.log。最初共享夹具移动后的路径/可见性编译错误、
未用restart的Clippy错误已经修正，未记成业务故障注入证据。

## 不能据此声称完成的范围

本门禁手工创建ComputeNode运行态、直接启动私有PG，没有跑完整compute_ctl启动
进程、Monitor/Agent持久协调、Scheduler/Runner、Pageserver/Safekeeper/RW/RO/
branch，也没有VM/物理回收认证。PG用例中的预算ID仍只是引用，尚无完整硬预算。
拒绝静默重绑验证的是保留未知状态的安全边界，不代表受控恢复已经实现。

下一步继续可信owner分配和持久实际循环、完整预算/quiesce、PG接管fence、
Compute显式受控重连/旧效果对账、Monitor及最终Runner私有QMP/串行隔离；
其余PG版本/Driver/Helm及完整Neon存储本地E2E仍继续。以上是代码/集成剩余项，
不能归为待Linux验证。Goal保持active，默认关闭、不构建镜像、不部署、不合主分支。
