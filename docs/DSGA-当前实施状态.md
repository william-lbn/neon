# DSGA 当前实施状态

更新：2026-10-19（Asia/Shanghai）。本文件只记录当前状态；设计以 V3 为准，历史验证以模块记录为准。

## 当前目标

项目所有者已确认按 V3 恢复实施，先完整完成 PG17，再逐版移植。当前批次是 S1/M2f：
最后 unpin/原生 waiter 的访问寿命，以及分配栅栏与 mapping/access 锁序；尚未实现完整 Shrink。

## 分支与维护基线

- PG：`codex/dsga-pg17-v3-20261019`，当前已提交头
  `b2f05855ca7feaa800cf1491c5366d0a33e7fdef`；M2f 尚未提交。
- Neon：`codex/dsga-neon-v3-20261019`，已提交头 `8103321197c8ba9a64f27d52fa624c8b68a1ba7f`；
  V3 文档及真实集群测试正在完善，gitlink/版本锁尚未提升。
- Autoscaling：`codex/dsga-autoscaling-20261019` / `ef6cd154aa3e60d72cab1e6b64abeb217ff9b245`，无 DSGA 业务改动。
- 官方维护已单独验证/中文提交：`6a003bbf61c7ba00247f151939fc08f5c20e352a`，包含
  Neon PG17 `56692dfb…` 及社区 PG17 `63de1f8fa42060e10326d2d9a0d48e559594ab86`。
- Neon/Autoscaling 官方 main 未新增；PG14–18 维护头已重新 fetch。其他 PG 的发行升级/功能移植尚未完成。
- 本轮提交尚未推送，自有主分支/稳定分支未合并，无生产发布。

## 当前验证事实

| 检查 | 实际结果 | 边界 |
|---|---|---|
| 最新官方维护独立 check-world | Mac 79 组/660 项报告检查通过 | TAP 未启用；不是 DSGA 语义完成度 |
| 纯 M2e + 新维护组合 | 核心225、isolation123通过 | 不包含后续 M2f 业务 |
| M2f 生命周期/访问测试首轮 | 9项真实临时集群测试通过，23.307s | 后续新增 helper/spec 后须最终复验 |
| M2f 分配栅栏 SQL | CTAS1200行及内容、真实 Buffer ID 边界通过 | 整个模块最终回归尚未通过 |
| mapping/access 确定性交错 | 正在完善 | 不将缺 expected 或 setup 失败计为通过 |
| Linux普通页 / KVM / HugeTLB / 完整存储端到端 | 未执行 | 后续按 T15–T22 单独验收 |

日志保存在工作区 `build/v3-*` 与 `build/check-world-pg17-upstream-63de.log`。

## 未提交草稿与恢复注意

M2f 旧草稿备份 stash 为 `6f002a31196f2e623e8386c38a649ff87be2ada4`，已 apply **一次**，
禁止重复 apply；备份尚保留。旧未完成 unpin spec 另存 `build/m2f-unpin-before-v3.spec`，
现改用真实多客户端测试验证原生 BufferPin，未修改 isolationtester。

实际构建 `build/neon-integrated/v17`、安装 `build/neon-integrated-install/v17`。
本批完成前须：模块回归、开/关完整 check-world、最终9项集群测试、pgindent/Ruff、中文
记录与模块提交；然后先推 PG 依赖，再更新 Neon 的 gitlink、两份锁及实际构建身份。

下一批处理 checkpoint 工作区寿命和恢复预算，再推进完整 Shrink、API 与外部控制链。
