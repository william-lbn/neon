# PG17 准入客户端与 HTTP：Mac 基本验收

## 本批实现

基于已推送 PG17 `b144a23b2beae7e6d214c094121bf4bbfe13cd3d` 与 Neon 1.10，不修改 PG 内核或 BufferABI6。Rust 新增严格七字段准入请求、完整回执与更高关闭模型，复用固定 JSONB 摘要编码器并支持 bool。Go 消费者使用同一原生 PG 摘要向量，对应 Autoscaling `66cff2dd5b7a515fcb01a75b76557a49cd6e8556`，发行依赖锁同步到该提交。

Rust SQL SDK 实际发现并核验 Neon 扩展所属 C 函数/精确签名，拒绝同名伪造函数，无通用任意 RPC 兜底；基础发现继续兼容 1.8，新准入设置/状态分别要求正式 1.9/1.10 入口。SDK 持有原完整准入意图及可选关闭意图，丢回复、busy/stale/conflict、普通 memory/fence、not_found/result_expired 均不自动清空；外层可从已认证持久 journal 显式恢复原值，不能由观察结果重造原请求。

原生 external Router 新增四个固定 POST：`/dsg/v1/admission`、`/status`、`/close-original`、`/close-original/status`（后三者相对于 admission 路径）。全部走专用验签、严格 typed JSON、16KiB、no-store；初始化只读 scope 不能调用它们。执行必须等于当前签名 epoch，原回执查询允许较早 epoch，实际 boot 再由绑定 PG 的 SDK 检查。

设置/关闭使用观察 SQL 连接，不等待被 Provider/resize 阻塞的执行连接。开放需 Running 且不能跨本地未知 resize；关闭和只读查询用于故障收口，不依赖外部 Running 标志。网络期间不持状态 mutex。显式关闭成功只解除未来门禁写栅栏，`prior_may_have_applied=true`、`vm_shrink_permit=false`；实际旧工作排空、预算和 VM 事实仍由外层持久协调器确认。

对应控制面固定路由名单也接入上述四路，使用独立观察连接池且保留原 body/凭据，仍核验 current owner、原 VM/Service；不改造成任意反向代理。自有控制面主线新 `0c322d5` 已独立完整同步为 `95d2002`，其 SSO 维护验证与本模块分开记录。

## Mac 必要验证

本次新增/受影响范围如下，不累计重叠运行，不重跑无关全量套件：

| 范围 | 实际结论 |
|---|---|
| Rust 新准入契约 | 两项基本契约通过；新增原生 PG 摘要向量独立一项通过；0 fail/ignored |
| Rust SQL SDK 实际 PG | 原意图保留/显式关闭一项、resize 在途独立关闭一项最终通过；0 fail/ignored |
| 生产 HTTP Router | 专用 scope/严格输入一项通过；真实 Router→SQL→准确 PG 完整原回执/更高关闭/新 owner 只读一项最终通过；0 fail/ignored |
| Go 消费者 | 新准入协议/未知恢复及受影响旧摘要共三项 race 通过；新增原生 PG 向量独立一项通过；两包 vet 通过 |
| 中心网关 | 两项受影响路由/resize 阻塞时独立准入路径 race 通过；vet 通过 |
| 静态 | Rust compute_api 与 compute_tools 受影响目标 clippy 通过；依赖 remote_storage 的既有警告保留，不宣称零警告 |

原生 PG 使用准确 b144 二进制与配套 Neon1.10，创建独占 0700 临时集群；失败数据/日志保留，所有夹具实际关闭自己的进程。新 bool/ASCII 转义/uint64 高位向量由真实 PG jsonb_out+sha256 生成，并被 Rust/Go 两端检查。

首轮新增并发用例失败原因是把 backend 私有暂停钩子设到观察连接，修正到执行连接后通过。首轮 HTTP 用例错误假设兼容夹具启用启动强制关闭，修正为检查查询保持实际初始状态后通过；没有放宽生产代码。两份失败日志保留。

证据在工作区 build：`dsga-admission-rust-contract-mac1`、`dsga-admission-rust-native-mac1`、`dsga-admission-rust-observer-mac2`（失败）/`mac3`（通过）、`dsga-admission-http-mac`（含首轮失败及最终静态日志）、`dsga-admission-http-native-mac2`、`dsga-admission-go-mac`、`dsga-admission-go-vector-mac`、`dsga-admission-context-vectors`、`dsga-admission-proxy-vet.log`。源码和重现命令均进入功能分支，日志不冒充生产环境认证。

复现受影响契约：`cargo test --locked -p compute_api --features postgres-client --test dsg_admission`；原生 SDK/HTTP 用例须显式提供 `NEON_TEST_PG_BIN` 和准确 `NEON_TEST_DSG_EXPECTED_SHA`，按各新增测试名执行 `--ignored --exact`。CI 原严格宿主门禁新增契约套件，其已有 SQL/HTTP 套件自动包含新原生用例并检查实际执行数量。

## 未完成代码与 Linux 后验的区别

本批完成准入接口/消费者，不表示整体在线内存伸缩完成。真实 Agent 持久预算/启动准入与完整 quiesce、公开创建 worker、Scheduler/Controller/Runner 最终 VM 事实、跨 boot/多 owner 未知关闭的持久恢复，以及 per-compute 暂停唤醒/存储通知来源身份等业务接线仍需继续。尤其不能为绕过过期关闭回执而清空 journal/HWM、重建空客户端，或认为 PG 换 boot 已撤销在途 QMP。

Linux K8s 验收单列：真实 RBAC/Service/证书供给、实际分片存储/网络与租约切换、NeonVM/KVM、内存热插拔/收缩、HugeTLB/物理释放、迁移/故障、性能长稳及端到端。Mac 基本逻辑通过不能免除内核/平台验收。本阶段不构建镜像、不准备或执行部署、不合入自有主分支，其他 PG 大版本暂缓。
