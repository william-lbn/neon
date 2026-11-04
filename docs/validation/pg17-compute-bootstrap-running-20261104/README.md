# PG17 实际 Compute 启动及唯一配置链路

同一业务端点保留原 tenant、timeline、MinIO、PVC 和业务凭据；旧失败 f0eb 物理实例经原 Runtime 协议退役，新 0aa4 实例用新 incarnation 和真实 UID 建立独立证据。新 Runner 7b6 由 118 构建、三节点实际 containerd 核验后，沿自有 Helm 发布；当前 Compute Pod 是 worker-117 上的 cp-d8d85ed796068ab5-fb8mv，观测为 Ready、零重启。

本轮从原 hold、同 Helm 渲染、两个精确 Runner 权限名、21 项真实 SubjectAccessReview 到 seal 全部通过。新 scope 观察器在 seal 前启动，核到真实 QEMU 身份、KVM 文件描述符、容器和 Pod 两级 11GiB 硬限。采样 OOM 为零只表示该时刻事实，不升级为最终持续证明。

原日志已进入 Compute wait_spec。无认证 GET 的 HTTP 400 不算通过；随后以原密钥公钥匹配、真实 ConfigSecret UID 和原配置校验，独立仅 GET 的读取器得到 HTTP 200/status=empty。原 once 客户端没有首次 POST 前 observe 能力，observe-only 要求真实发送标记；此前相关误读已在 Linux 纠正归档，未伪造标记或删除保护。唯一 configure 之前，Root 对外部请求意图的文件和父目录均 fsync；原客户端仅执行一次，22.381 秒返回 HTTP 200/status=running，VM UID 保持，原真实 MAY 和结果均保留，禁止因后续查询重复 POST。

当前验收只到真实启动与配置 API。running 不替代物理 PG 身份、Proxy SQL、资格、四数组回收或真实 normal 硬降限；Grow、安全 Shrink、Regrow、自动策略仍 0/4。下一步沿正式 Proxy:4432 执行既有 512 行确定性数据用例，并使用原 Collector 和 Issuer 收集实际来源后再走原 Owner Begin。这里只聚焦本次内存修改及直接受影响组件，不测试 Kubernetes 平台本身。

原日志、私有参数、镜像和大证据均留 Linux /data；仓库只保留非敏感小收据。证据按各自窗口解释：原 API 窗已结束，新同 UID 观察窗独立续接，不能宣称无缝或复用旧失败实例数据。
