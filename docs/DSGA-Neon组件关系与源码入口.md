# 为什么实现 Neon 内存伸缩需要修改 PostgreSQL

日期：2026-10-19。范围：本文所说的内存伸缩是 Neon **Compute** 的内存及其
`shared_buffers` 在线增减，不是给共享的 Pageserver 调整进程缓存。

## 1. 实际源码依赖

Neon 不是只有 Rust 存储服务。SQL 由经过 Neon 修改的 PostgreSQL Compute 执行；
Rust Pageserver 提供页面，Safekeeper 接收并持久化 WAL。Compute 的共享缓冲池、
Buffer pin、dirty page、IO 状态和替换器仍由 PostgreSQL 的 C 代码管理。

```text
william-lbn/neon
├── vendor/postgres-v17  ── gitlink ──> william-lbn/postgres 的固定提交
│   └── storage/buffer + storage/ipc：Compute 的 shared_buffers/DSGA
├── pgxn/neon：Neon 的 smgr、LFC、WAL proposer 与 PG 集成
├── compute_tools：Compute 启动、配置、重启与预算
├── libs/vm_monitor：Guest 内资源协调、LFC、PG 容量与缩容确认
├── pageserver：Rust，远端页面/版本服务
└── safekeeper：Rust，WAL 持久化服务

william-lbn/autoscaling
├── pkg/agent：扩缩策略、Revision、Monitor/VM 协调
└── neonvm-*：VM 内存设备、实际完成状态与资源回收
```

依据不是目录命名推测：主仓 `.gitmodules` 的 PG17 URL 为 `../postgres.git`，
在 `william-lbn/neon` 中解析为 `william-lbn/postgres`；`postgres.mk` 从
`vendor/postgres-v17/configure` 构建，Compute Dockerfile 同样复制该子模块源码。
官方也使用这种子模块结构，见 [官方 PG 更新流程](https://github.com/neondatabase/neon/blob/main/docs/updating-postgres.md)。

## 2. 必须修改的组件

| 组件 | 仓库/语言 | 本功能中的责任 |
|---|---|---|
| Neon PostgreSQL 内核 | postgres fork / C | 稳定地址、Granule、在线 Grow/Shrink、pin/dirty/IO/tag 排水与安全回收 |
| Neon PG 集成与 LFC | neon/pgxn / C+Rust | PG 控制接口、Neon smgr/预取路径兼容、LFC 与 Buffer Pool 的预算分配 |
| Compute 启动管理 | neon/compute_tools / Rust | 能力发现、启动上限和初值、VM 缩小后按实际预算重启 |
| Guest Monitor | neon/libs/vm_monitor / Rust | 请求 PG 扩缩、确认实际状态、故障/重连对账、禁止无证明的 VM 缩容 |
| Autoscaling Agent | autoscaling/pkg/agent / Go | 目标与 Revision、操作身份、扩缩顺序、重试和过期许可拒绝 |
| NeonVM | autoscaling/neonvm-* / Go/QEMU 集成 | Guest/virtio-mem 实际完成、HugeTLB、迁移互斥和 Host 资源对账 |
| 源码与发行锁 | neon 的 gitlink/JSON/CI | 保证构建出的 Compute 确实包含指定内核与配套扩展 |
| Pageserver/Safekeeper | neon / Rust | 运行页面读取、WAL 解码/重放、恢复、只读副本兼容测试；内存伸缩本身不要求重写持久化协议 |

现有 VM/cgroup/LFC 伸缩与新增 Buffer Pool 在线伸缩是不同层次。当前 Monitor
`try_downscale()` 管理 filecache/cgroup，尚没有本项目要求的动态 Buffer 排水
与物理释放回执。仅修改它或增加 VM 内存，不能让启动时固定的 Buffer Pool 变为动态。

## 3. 本轮曾存在的集成缺口及纠正

最初为隔离开发，将你们的 Postgres fork 检出在工作区 `repos/postgres`。虽然是
同一个 Neon 内核 fork，主仓子模块仍锁定旧的 `1e01fcea...`，所以主仓构建不会
包含这些修改。不能把独立内核回归通过当成 Neon 功能已完成。

现已将该完整检出移入主仓真正的 `vendor/postgres-v17`，并使用 Git 标准
`submodule absorbgitdirs` 管理。旧路径只保留指向该目录的本地符号链接，供之前
的 out-of-tree 构建继续使用；不是第二份独立源码。该符号链接不进入主仓发行。

本地主仓同步 gitlink、`ci/distribution.json` 与 `vendor/revisions.json`，候选
内核为 `b1188732d73d7d5069a18abe014dfaf2bb0753dc`（17.11 + Neon + M2a/M2b）。
推送时先推依赖提交到自有 fork，再推引用它的 Neon 功能分支。只有完整必要验收
通过并由项目所有者确认后才合入自有主分支；候选分支不可当成生产发行版本。
镜像、生产分支和线上配置尚未变更。

## 4. 当前可证明与尚未完成的部分

- 已完成官方 PG17 稳定修复合并及 Neon WAL 兼容修复；Neon Rust wal_decoder
  与实际 neon_rmgr 恢复路径已有验证。
- 已接入真实四数组布局、L/H/M 初始状态、boot 身份、Postmaster 共享内存重建，
  开/关两种配置各完整 check-world 通过，真实数据恢复测试通过。
- **尚未交付在线 Grow/Shrink、完整 Granule/操作协议、Monitor/Agent/VM 链路。**
  此时不能声称内存已经可以在线伸缩，也不能允许 VM 凭这些测试缩容。
- 后续验收必须从 Neon 主仓的构建入口开始，验证相同 gitlink 的内核、Neon 扩展、
  Rust 存储与资源协调。Linux/KVM/HugeTLB 实测按用户要求另行安排。

完整范围仍以 [实施范围与验收](DSGA-实施范围与验收.md) 为准。
