# 自维护 Neon：源码与镜像发行

## Fork 审计

三个 fork 的 parent 都是对应的 `neondatabase` 仓库。初始 Neon、Autoscaling
的 `main` 与上游最新提交一致。Postgres 的 `main` **只有说明文件**，数据库源码
在 `REL_14_STABLE_neon` 到 `REL_18_STABLE_neon` 分支；需要保留这些分支。
不能把“默认分支最新”理解成“所有组件和 PostgreSQL 版本都兼容”。

当前 Neon 源码支持 PG14–17。`ci/distribution.json` 锁定四个 PostgreSQL
gitlink 和一个 Autoscaling commit。`.gitmodules` 中相对地址 `../postgres.git`
解析到自己的 `william-lbn/postgres`。PG18 分支的独立编译不代表本发行支持 PG18。
升级 PostgreSQL 应更新 gitlink、锁文件并通过整个发行的测试。

## 构建入口

主入口是 `.github/workflows/dockerhub.yml`：

1. 在 Neon 和 Autoscaling 仓库设置 `DOCKERHUB_USERNAME`、`DOCKERHUB_TOKEN` Secrets。
2. 启用 Actions，推送 `main` 或手动运行 **Docker Hub Neon distribution**。
3. 默认原生 `linux/amd64`；可单独选择 `arm64`。每次发行仅对应所选架构。
4. 所有镜像共享 UTC 版本，例如 `2026.09.30-123456-<sha12>-r<runid>-a1`。
5. 成功后 GitHub Release 附件 `distribution-manifest.json` 列出完整镜像和 digest。

完整构建可能需要数小时。首次没有缓存，尤其是 Rust、Linux 内核、PLV8、RDKit、
DuckDB 和 Rust 扩展。采用上游 Compute Dockerfile 的 `EXTENSIONS=all`，将其拆成
公共依赖和 39 个扩展阶段，每个 PG 版本都构建。阶段结果通过自己的仓库按 digest
重用，控制并发及磁盘使用；没有改成 minimal 来省略扩展。

上游 CI 文件保存在 `.github/upstream-workflows/`，避免 fork 自动调用 Neon 的
私有 runner、AWS 角色和发布端点。原始 Dockerfiles、构建逻辑和许可证保留。
PG14–16 的 Bullseye 包源改为官方归档；安全包固定到最后一个 LTS 日期快照
`20260831T235959Z`，保留 APT 签名和包哈希校验，以修复 Debian #1147093 的 404。
这维持上游 ABI/ICU 兼容，但 Bullseye LTS 已结束；后续发行版升级需要重新验证
数据库排序规则和扩展兼容性。
H3 的旧源码地址失效，改为维护方的 `postgis/h3-pg` 地址；仍使用 v4.1.3，
下载归档的 SHA256 与原上游固定值一致。

## 完整发行清单：46 个镜像

镜像统一位于 `docker.io/williamluckyli/<name>:<version>`。

| 类别 | 镜像 |
| --- | --- |
| 构建与组合镜像（2） | `neon-build-tools`, `neon` |
| 核心与工具（18） | `pageserver`, `safekeeper`, `proxy`, `pg-sni-router`, `storage-broker`, `storage-controller`, `endpoint-storage`, `storage-scrubber`, `neon-local`, `pagectl`, `compute-ctl`, `fast-import`, `local-proxy`, `vm-monitor`, `storcon-cli`, `pagebench`, `compaction-simulator`, `wal-craft` |
| Compute（4） | `compute-node-v14`, `compute-node-v15`, `compute-node-v16`, `compute-node-v17` |
| Compute 测试（4） | `neon-test-extensions-v14`, `neon-test-extensions-v15`, `neon-test-extensions-v16`, `neon-test-extensions-v17` |
| Compute VM（4） | `vm-compute-node-v14`, `vm-compute-node-v15`, `vm-compute-node-v16`, `vm-compute-node-v17` |
| 连接池与指标（4） | `pgbouncer`, `postgres-exporter`, `pgbouncer-exporter`, `sql-exporter` |
| Autoscaling / NeonVM（10） | `autoscaling-go-base`, `vm-kernel`, `vm-builder`, `neonvm-controller`, `neonvm-vxlan-controller`, `neonvm-runner`, `neonvm-daemon`, `autoscale-scheduler`, `autoscaler-agent`, `cluster-autoscaler-neonvm` |

另有 `neon-build-cache`、`neon-compute-build-v14` 到 `neon-compute-build-v17` 存放中间构建阶段和缓存，
不是运行服务。客户端兼容测试示例、示例 VM 和代码生成容器不属于发行服务。

核心/Compute/Autoscaling 服务来自本 fork 源码；三种指标 exporter 沿用上游固定版本
及校验和的发行二进制，PgBouncer 从源码编译。上游 OSS 无法获取的私有 Subzero
`rest_broker` 功能不在本发行；公开 proxy 功能按原始默认 feature 构建。

## 发布门槛与使用

CI 检查 gitlink 与锁文件、完整二进制/扩展清单、构建阶段合并逻辑、Go race 测试；
对 PG14–17 启动对象存储、broker、pageserver、3 个 safekeeper 和 compute，检查
SQL、vector、PostGIS 以及删除 compute 后重新获取数据；PG16/17 运行上游扩展和
contrib 回归测试（沿用上游 SKIP 清单）。VM 镜像做 qcow2 完整性和容量检查。
发布时核对所有 46 项、相同版本、源码提交、命名空间、digest 和远端 manifest。
全部通过才创建 Release 并更新 `latest`。

部署建议使用 manifest 中的 `image@digest`，并让相关组件使用同一次发行。
不要在生产中混用日期版本或让各组件自行追踪 `latest`。单独组件镜像继承上游
运行环境，并以所选二进制为 ENTRYPOINT；配置、证书、持久卷、服务地址和参数
仍需部署方提供。`neon` 的默认 pageserver 配置是上游演示配置。

Compose 测试不是三节点 Kubernetes 验收。NeonVM 还需要 KVM、CRD、RBAC、
节点网络配置和资源策略。proxy 的 `--help` 检查不代表认证控制平面的集成验收。
本仓库不包含 Neon 商业托管平台的全部控制平面功能。

保留 fork 关系便于比较和同步上游。不要直接用 Sync fork 覆盖自维护 CI；先在
升级分支合并上游、审阅构建和依赖变化、更新锁文件，完成整套测试后再合入 main。
Cargo.lock、扩展下载地址和 OS 包仍依赖外部源；若需要长期离线重建，可进一步
镜像这些第三方源与依赖。密码和 token 只能放 Secrets，不能写入 Git。
