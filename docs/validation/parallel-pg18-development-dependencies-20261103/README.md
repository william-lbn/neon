# PG17/PG18 并行开发依赖闭合

本次只同步已推送的开发依赖，不表示安全 normal、生产 Source 或 Kubernetes 验收。

- PG17 仍使用 6c6ac086bc5aa69ff2da0b40da7ae087e08e5265；默认发行版本矩阵仍为 PG14–PG17。
- 独立 PG18 gitlink 与 revisions 同步到 1e9bf010a841195752f2948ef54ecb34a0fcc749，包含内置 WAL proposer 固定槽权限门适配。
- autoscaling 同步到 5d7f276286a0f2caf057545a9cc98388a41f6a21，包含 normal 降限恢复、Quiesce 就绪等待与明确 PG18 构建选择；镜像清单加入已有构建配方的独立平台资格发布器。清单条目不代表已部署或资格已签发。
- source_lock 校验 revisions 中所有已登记的并行版本与 gitlink/实际检出，保持返回的默认构建矩阵不变，拒绝短 SHA、缺失及不匹配引用。

Linux 原始结果：`/data/neon-build/validation/parallel-pg-source-lock-20261102T213218Z-e9759e`。9 个定向单元测试通过；提交前校验实际 index，PG17/PG18 检出准确且已跟踪源码干净。未扩大功能回归矩阵。完整大日志和产物保留在 /data，不写入仓库。
