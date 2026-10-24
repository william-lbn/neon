# GuestReady资源执行版本对齐

在既有 `codex/dsga-neon-v3-20261019` 演进。重新 fetch 并核对 Neon main
`fa504217c61bbcaf5c512d75830564541f917f8f`、Autoscaling main
`a4bce09e4d6bd7932ea634a1ed79afe26a9cfd1f`、Neon PG17
`56692dfb680281a963c7470fc7f0fec7f65ecfd4`、社区 PG17
`e05b7e2a45c7a151ad3f8771ba7f5d8b4ddd8d0c` 及自有控制面 main
`7899ec0e8f1aab75598ec37e566d7260568a1c9d`：各现有功能分支均包含完整维护头，
没有挑选少量修复、重建分支、改主分支或移植其他 PG。

AS `d3bd69d246cb40f6116fd1b7c874a9a319004a81` 已先推送和核对自有远程，含
`cb1fe5b8` 预算合同及后续实际 GuestReady 执行器。Neon 发行锁采用完整 AS SHA；
控制面采用同 SHA 的准确远程 Go 版本及校验和。PG17 实际源码、gitlink 和两份
PG 锁仍为 `cbabd5de541a1340ab4675cfc3ef14b3a9372671`；本批无 PG/Rust 源码修改。

实际 Agent/PGWorker 已推进 GuestReady：只消费原账本完整 Prepared，核对当前
PG/floor/原释放、原 Guest/Birth/全 hstate 和实际 VM/Runner，资源 revision
来自独立 root WAL。完整命令先 UID/RV CAS 后派发；回复未知仅 Observe，显式
原重放分开，不改 PID/epoch/预算。写后重读再记录原审计，不等于最终 VM 许可。

AS Mac 去重 4 顶层/21 含子项 race 通过、0 未修复失败/跳过；六包/实际 Agent
vet 通过，实际 Linux Agent/root guard 各 amd64/arm64 共 4 目标编译通过，仅
编译。并发测试屏障曾与单观察池互等，修正为两个实际 worker 后通过，详见 AS
`docs/DSGA-GuestReady资源执行-Mac基本验收-20261024.md`，未掩盖首次失败。

本仓仅发行锁与实施规范改动，实际 `ci/compute.py validate --source-ref index
--require-pg-checkout v17` 退出 0（18 程序/4 PG/39 扩展阶段）；未重复既有 PG、
Rust、路由或 SDK 功能测试。控制面准确依赖下实际消费者 vet 退出 0；Go 下载
Origin.Hash 匹配 AS 远程 SHA。证据在工作区 build/dsga-guest-ready-*，8 份最终
Go 源码 SHA256 在 `dsga-guest-ready-source-sha256.json`。

**完整功能仍未开发完毕**：Prepare/Finalize 可信生产者/系统 outside 约束、首次
独立准入/完整排空、Host/Scheduler/最终 Controller-Runner-QMP、公开 worker/
生命周期与受控跨 Guest/PG boot/owner 未知关闭仍是代码，不是平台待验。
真实 Linux K8s/RBAC/NeonVM/KVM/HugeTLB/物理回收/迁移及性能长稳后验。资源
合同、cgroup 上限、GuestReady 完成都不能替代这些结论。默认关闭，Goal active；
本轮没有构建镜像、准备或执行 K8s 部署、合并自有主分支或发布生产。
