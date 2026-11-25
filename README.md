# Neon 数据面与组合发行

Neon 是计算与存储分离的 PostgreSQL 平台。本维护分支提供存储引擎、Compute 工具、PG17/PG18 运行时构建及跨仓库组合发行；控制台与控制 API 由 Control Plane 仓库提供，虚拟化与资源安全由 Autoscaling 仓库提供，完整部署由 Helm 仓库统一编排。

## 数据面组件

| 组件 | 职责 |
| --- | --- |
| PostgreSQL Compute | 执行业务 SQL，通过 Neon 存储协议访问数据 |
| Pageserver | 提供数据页，处理 WAL 并管理持久存储层 |
| Safekeeper | 接收和保全 WAL，为 Pageserver 提供可靠日志来源 |
| Storage Controller | 管理存储节点、租户分片、放置与存储操作 |
| Storage Broker | 为存储成员提供发现与协调信息 |
| Proxy | 提供 SQL 入口与连接路由，结合控制面完成认证和唤醒 |
| compute_ctl 与 Guest 工具 | 管理 Compute 启动、配置及运行观察 |

当前架构和发行关系见[标准产品边界](docs/标准产品与发行.zh-CN.md)。细节源码目录及上游协议资料见[开发资料目录](docs/SUMMARY.md)，不能将上游或本地开发能力等同于本分支已验收的产品功能。

## 内网标准部署

使用 [Helm 中文入口](https://github.com/william-lbn/neon-helm/blob/codex/dsga-neon-helm-v3-20261019/README.md)取得不可变发行包，填写显式站点输入后一次安装。无需公网 IP 或公网域名；内部 DNS、内部 CA、真实存储和内网镜像仓库由相应层提供。不要将 `docker-compose` 或 `neon_local` 开发模式当作完整产品部署方案。

当前标准资格覆盖已验收的 Linux/amd64、KVM、主 CNI、声明的本地持久存储和 PG17/PG18 运行规格。新增 Node 连续接入暂缓；其他规格、生产 HA、完整 TLS、灾备、外部存储或可选产品需要独立验证。

## 发行与开发

`ci/distribution.json` 固定维护源码，`.github/workflows/dsga-release.yml` 的复用工作流固定相同提交。源码锁与镜像摘要分别记录；CI 成功、镜像构建或源码更新都不能单独证明现场部署和业务端到端通过。

维护分支为 `codex/dsga-neon-v3-20261019`；PostgreSQL 分别维护于 `william-lbn/postgres` 的 PG17/PG18 对应分支。依赖先提交再更新本仓库引用，不随意移动 PostgreSQL gitlink，不改写冻结历史发行。中文提交说明要记录原因、兼容性、实际检查及待办。

上游本地构建、开发环境和项目背景原文保存在 [ef037b9 快照](https://github.com/william-lbn/neon/blob/ef037b91b69cbdce4beba83e0cd282e230bb5f3b/README.md)。原作者源码注释、法律许可和协议字段保留；产品入口与新增运维描述使用中文。
