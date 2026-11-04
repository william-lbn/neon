# PG17 Runner 初启读取修复：镜像与三节点归档

此前测试 Compute 的 QEMU 已启动、Guest 已进入 initial-boot，Runner 初启授权链一次 StateClaim GET 超时导致启动终止。修复仅对 Started 专用上下文中的瞬态纯读取作有界重读：每次完整重验原 Owner/Lease UID、代次及身份；不延长原三分钟截止时间，不重放 QMP、Prepare、CAS 或启动，不放宽 Unknown、安全门及真实 normal 降限要求。源码与直接测试在 autoscaling 原功能分支 aafcc8a 中，七个定向入口共三十项通过。

本次只从已验证的原镜像替换 Runner ELF 与 revision 标签。完整 Docker 配置仅在内存中比较，没有保存 Env 或凭据。原 OCI 包复用并经 SSH 到 117/119，实际 socket 的 k8s.io 中三节点均读回同一 manifest 和 image ID。首次调度输入名错误、首次预载缺纯 digest 别名的失败均原样保留；后续只补同 descriptor 名称映射，未重新编译或导出。

118 上可查看：`docker images --digests neon-pg17-visible/neonvm-runner`。

所有大包、日志与原始证据留在 receipt.json 所列 Linux /data 路径。仓库仅保存小型非敏感归档。已通过原 Runtime 协议退役旧失败物理 VM，保留 tenant、timeline、MinIO、PVC、凭据和历史；新物理实例另行绑定和验证。本收据不是 Compute 就绪、SQL、资格或内存端到端通过证明，四项集群内存动作仍未验收。
