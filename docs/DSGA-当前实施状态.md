# DSGA 当前实施状态

更新：2026-10-19（Asia/Shanghai）。本文件只记录当前状态；设计以 V3 为准，历史验证以模块记录为准。

## 当前目标

项目所有者已确认按 V3 恢复实施，先完整完成 PG17，再逐版移植。S1/M2f 已完成 Mac
模块验收：最后 unpin/原生 waiter 的访问寿命，以及分配栅栏与 mapping/access 锁序。
尚未实现完整 Shrink，后续进入 checkpoint 工作区寿命和恢复预算切片。

## 分支与维护基线

- PG：`codex/dsga-pg17-v3-20261019`，M2f 提交
  `b5785f810de56b583b6548a8697561989cd9aa8a`，已推送并核对远程 SHA。
- Neon：`codex/dsga-neon-v3-20261019`；V3 标准 `2422ce9`、构建身份修复 `9df2ba8`；
  本模块将真实测试和 gitlink/两份锁同步到上述 PG 提交，最终提交以分支记录为准。
- Autoscaling：`codex/dsga-autoscaling-20261019` / `ef6cd154aa3e60d72cab1e6b64abeb217ff9b245`，无 DSGA 业务改动。
- 官方维护已单独验证/中文提交：`6a003bbf61c7ba00247f151939fc08f5c20e352a`，包含
  Neon PG17 `56692dfb…` 及社区 PG17 `63de1f8fa42060e10326d2d9a0d48e559594ab86`。
- Neon/Autoscaling 官方 main 未新增；PG14–18 维护头已重新 fetch。其他 PG 的发行升级/功能移植尚未完成。
- PG 维护/业务依赖已推送并核对；Neon 集成只推当前新功能分支，远程状态以 GitHub ref
  为准。自有主分支/稳定分支未合并，无生产发布。

## 当前验证事实

| 检查 | 实际结果 | 边界 |
|---|---|---|
| 最新官方维护独立 check-world | Mac 79 组/660 项报告检查通过 | TAP 未启用；不是 DSGA 语义完成度 |
| 纯 M2e + 新维护组合 | 核心225、isolation123通过 | 不包含后续 M2f 业务 |
| M2f 最终主仓生命周期/访问测试 | 新 SHA 二进制及配套库9项通过，23.938s | 包含访问探针、原生pin三路径及恢复；不是完整Neon存储端到端 |
| M2f 分配栅栏 SQL | CTAS1200行及内容、真实 Buffer ID 边界、上下界拒绝通过 | 不等于 H 已缩小 |
| mapping/access 确定性交错 | 真实 mappingX/accessX 的指定顺序完成，模块1项regress+4项isolation通过 | 未修改原生锁算法 |
| M2f 开关完整回归 | Mac 开/关各81组/665项报告检查通过 | TAP未启用；不等于全部DSGA语义/物理能力已完成 |
| Linux普通页 / KVM / HugeTLB / 完整存储端到端 | 未执行 | 后续按 T15–T22 单独验收 |

日志保存在工作区 `build/v3-*` 与 `build/check-world-pg17-upstream-63de.log`。

## 未提交草稿与恢复注意

M2f 旧草稿备份 stash 为 `6f002a31196f2e623e8386c38a649ff87be2ada4`，已 apply **一次**，
禁止重复 apply；备份尚保留。旧未完成 unpin spec 另存 `build/m2f-unpin-before-v3.spec`，
现改用真实多客户端测试验证原生 BufferPin，未修改 isolationtester。

实际构建 `build/neon-integrated/v17`、安装 `build/neon-integrated-install/v17`。
主仓实际 `postgres --version` 已核对为 b5785f8 完整 SHA，配套模块重新编译后通过9项
真实集群测试，日志 `build/neon-integrated-v3-m2f-lifecycle-verified.log`。构建缓存回归
检查已在旧规则负向对照中捕获旧 SHA，在新规则及实际主仓重建中通过。
pgindent、Ruff通过；CI prepare为13项逻辑通过、9项环境测试skipped，后者已另行真跑。

下一批处理 checkpoint 工作区寿命和恢复预算，再推进完整 Shrink、API 与外部控制链。
