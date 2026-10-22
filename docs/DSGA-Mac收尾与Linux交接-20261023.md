# DSGA Mac 收尾与 Linux 交接（2026-10-23）

本记录交付本批已实现模块及其 Mac 证据。**完整 V3 仍未完成，不能称为只剩
Linux 验证。** 功能默认关闭，不合自有 main、不构建镜像、不准备或执行 K8s
部署，不发布生产。Linux 可以先验收已完成模块，完整业务 E2E 须先补齐下列代码。

## 仓库与官方基线

本轮 2026-10-23 05:07（Asia/Shanghai）fresh fetch，检查完整祖先关系：

| 来源 | 已完整包含的 SHA |
|---|---|
| 官方 Neon main | `fa504217c61bbcaf5c512d75830564541f917f8f` |
| 官方 Autoscaling main | `a4bce09e4d6bd7932ea634a1ed79afe26a9cfd1f` |
| 官方 Neon PG17 维护 | `56692dfb680281a963c7470fc7f0fec7f65ecfd4` |
| 社区 PG17 维护 | `e05b7e2a45c7a151ad3f8771ba7f5d8b4ddd8d0c` |
| 自有 control-plane main | `c8767714fa91fb5836d1b7da2d3e5e31f37dc78e` |

这是 fetch 时点的完整提交范围，不是对未知 bug 或未来提交的保证。PG17 独立
维护 `d70986fd29d8293058c5780df982fce23e09eecf` 与功能组合分别验证；官方 5 个
文件与新维护头逐字节相同，无冲突并保留作者/注释。没有以社区 PG 替代 Neon。

| 自有仓库 | 交接分支 | 依赖/功能提交 |
|---|---|---|
| postgres | `codex/upstream-pg17-20261023` | `d70986fd29d8293058c5780df982fce23e09eecf` |
| postgres | `codex/dsga-pg17-v3-20261019` | `9cf674a53bcadaef7b3faecacf61af3c1cf59aac` |
| control-plane | `codex/dsga-control-plane-v3-20261020` | `0f929586e1f5db15982a129d0b0e944ae33aeadc` |
| autoscaling | `codex/dsga-autoscaling-20261019` | `8e1e0ea93d5f2775376622a38ed10f39d9d1fc8c`（本批源码未改） |
| neon | `codex/dsga-neon-v3-20261019` | 本提交的 gitlink 与两份版本锁，PG17 均为 `9cf674a` |

PG14–16 发行锁及对应功能没有在本批升级/移植；PG18 尚须发行接入。分支存在
不代表这些大版本已经实现弹性能力。用户要求仍保留：PG17 完整后逐一移植验收。

## 本批实际修改

完整纳入新的 PG17 currtid 恢复期拒绝、空 tsquery 二进制输入、bigint 参数、psql
补全与逻辑复制测试维护。新增带中文说明的真实主备回归
`vendor/postgres-v17/src/test/recovery/t/062_neon_currtid_recovery.pl`：核对实际恢复
状态、正常访问和 unlogged/视图拒绝的 SQLSTATE、无元组输出及错误后继续服务，
并覆盖空 tsquery 二进制 COPY 与高 32 位非零阶段号；不以源码字符串替代运行。

自有控制面完整同步大对象流式存储及 TOTP 分支迁移主线，保留原始 058 双分支
前向 join，新 059 无冲突。Mac 构建修复显式 `.tsx` 组件导入；MFA 测试修复上一窗
验证码跨窗竞态，未放宽生产验证/限流/重放规则，关键原因均有中文注释。
详细结果见自有 control-plane 同名交接文档与其中机器摘要。

## Mac 验证及准确版本

开发机为 Mac arm64；使用 Xcode SDK 14.4。Go API 使用 1.26.5、Autoscaling
门禁使用 1.25.13，未改变各仓声明版本；Web/Auth 使用 Node 24.19。

| 范围 | 实际结果 |
|---|---|
| PG17 独立维护候选 | 225 核心 + 123 隔离 + 14 真实主备/边界全部通过 |
| PG17 功能组合候选 | 225 核心 + 123 隔离；035 逻辑复制/062 边界共 97 项 TAP 通过 |
| PG17 准确 `9cf674a` 构建/原生 | 构建与安装版本均核对同一 SHA；80 内核 145.272 秒、52 正式 SQL 95.511 秒通过，0 fail/skip |
| PG17 准确核心/主备 | 225 核心、123 隔离通过；DSGA off/on 的 001/035/062 各 160 项 TAP 通过，0 fail/skip |
| 实际 Go → TLS → Rust Compute → SQL → PG | 1 Rust/2 Go race 进程通过，9.52 秒；0 skip/ignored/filtered，含重启、epoch2 持久循环及未知回复对账 |
| 完整控制面 API race | 1369 pass：380 顶层/989 子；11 测试包/9 无测试包；0 fail/skip |
| 控制面数据库 | 真实 SCRAM PG17 准确 `0edff73d`、PostgREST 16.4 已运行并关闭 |
| Auth 服务/Web | 真实 PG/Better Auth/SMTP 13 项、Web 43 项通过，0 skip；两者类型检查及 Vite 构建通过 |
| 静态/跨编译 | 控制面全部 vet、Linux/amd64 CGO0 全部 cmd 编译通过；非 Linux 运行 |
| Go 冻结计划 → Rust | 本次实际 1 Go/1 Rust、11 样本（2 有效/9 拒绝）通过，源文件/契约摘要一致 |
| prepare | 145 项中 13 通过/132 环境 skip；真实 PG 门禁单独运行，不能称 145 全执行 |

准确版本与日志 SHA256 见[机器验收摘要](acceptance/dsga-mac-close-20261023.json)。
恢复 TAP 每轮分别是 001 的 63 项、035 的 83 项与 062 的 14 项；已读取逐文件
原始 TAP，核对无 skip/TODO/not ok，不能只用 make 的退出码代表所有用例实际运行。
80/52 与 Go/Rust 门禁使用同一准确安装版本，候选日志没有被用来替代本次结果。
Go 链路仍不是完整中心控制面公开创建、真实 VM 或完整 Neon 存储链 E2E。

候选和旧准确 SHA 结果分层保留，不挪用为新准确版本认证。本次首次准确门禁
被版本锁预检拦住：工作树已更新而 index 仍是旧 gitlink；三处同步暂存后预检
通过，才运行实际内核测试。原失败日志保留，没有绕过校验或将未执行记为通过。

复验使用 `ci/test-dsg-postgres.sh`、`ci/test-dsg-neon-extension.sh` 和
`ci/test-dsg-go-client.sh`，分别传实际构建/安装目录及自有 Autoscaling 源码。
提交前执行 `python3 ci/compute.py validate --source-ref index --require-pg-checkout v17`。
Go/Rust 冻结计划契约由 `ci/test_dsg_creation_plan.py` 使用新 report 目录实际执行。
核心/隔离用对应目录 `make check`；恢复 TAP 用 001、035、062，off/on 分别提供
预载匹配 rmgr 与对应 DSGA 开关的 `TEMP_CONFIG`。脚本和逐项原始日志保存在本工作区
`build/run-upstreame05b7-*`、`build/upstreame05b7-*`，摘要索引随本提交保留。

本批没有重做新 SHA 的全部 check-world、可选 ICU/readline/SSL/GSS 组合或 Web
浏览器业务 E2E；它们不能笼统归因于 Mac 硬件不支持。旧准确 `1edef` 的全 world
结果只属于旧版本。当前验证足以交接本批模块，不是完整 Neon 产品认证。

## 尚须完成的代码

1. 完整安全 Renderer、普通租户/私有管理凭据与真实角色/SET ROLE 隔离、公开创建
   选择、VM 模板及真实 worker；旧 Driver 的租户 cloud_admin SUPERUSER 不能直接
   沿用后开放 DSGA 准入。已有 typed 核心/冻结计划不是完整公开创建链。
2. 完整预算/quiesce、Monitor/Scheduler/Controller/最终 Runner fence、跨 owner/boot
   受控关闭、其余 PG 大版本，以及完整 Neon 存储链 E2E。这些属于编码/集成缺口，
   不能转记成只有 Linux 待测，也不能据本批测试将 Goal 标为完成。

## Linux 集群验收责任

以下按 [V3](DSGA-架构复核与实施规范-V3.md) 的 T01–T22/G0–G5 验收；先锁定
本表自有提交及真实构建身份，任意失败/skip/未执行分别记录，不启用生产开关。

| 目标范围 | 必须提供的真实证据 |
|---|---|
| Linux PG/Provider | 同版构建回归，普通页/HugeTLB 能力与明确拒绝，逻辑提交和物理回收分别计量；Mac Provider 结果不得代替 |
| K8s 身份与恢复 | 实际 CRD/admission/RBAC/投影令牌/TokenReview、Lease/VM/Config UID、原 journal 与未知结果重读/重放，错误主体拒绝 |
| VM/Host/Guest | KVM/NeonVM/virtio-mem 实效、Host/Guest/HugeTLB 真实回收，迁移互斥；PATCH 或 QMP 已接收不是完成证据 |
| 故障与 Neon 存储 | SQL 负载/存储一致性、主/只读回放、pin/IO/长事务、取消/断线/丢回复、控制进程/PG/VM 重启及 owner/boot 切换 |
| 容量与性能 | 预算约束与排空、资源预留/释放顺序、连接保持、TPS/P99/CPU/GetPage/内存基线及长稳；amd64 首期，aarch64 独立验收 |

本轮没有创建镜像或部署脚本，也没有执行上述集群操作。后续镜像来源和部署
必须使用自有已验收提交；不能为测试临时绕开权限、预算、所有权或未知结果屏障。
