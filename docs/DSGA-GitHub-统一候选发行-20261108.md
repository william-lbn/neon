# DSGA GitHub 统一候选发行说明（2026-11-08）

## 目标与实际范围

新入口为 `.github/workflows/dsga-release.yml`。它监听
`codex/dsga-neon-v3-20261019` 和 `ci/dsga-unified-release-*` 的 push，按准确依赖提交
执行原生 PG 门禁、镜像构建、存储/SQL smoke、Helm 打包，最后在 **Neon 仓库**创建一份
完整 prerelease。全过程不需要向 PostgreSQL、Autoscaling、Helm 仓库写 Release 的跨仓 PAT。

这次改造不能单凭本地合同测试宣称已经云端发布；真实镜像构建、registry push、GitHub Actions
和最终 Release 必须以首次云端 run 为准。原来分支里的三节点四动作验收是独立历史证据，不自动
转移为本次重新编译产物的物理缩容资格。

## 固定来源

| 依赖 | 功能分支 | 当前完整提交 | 作用 |
| --- | --- | --- | --- |
| PostgreSQL 17 | `codex/dsga-pg17-v3-20261019` | `203e1e83eff9dcb73e03cbeff685f51f7da9539c` | 原 PG17 功能、原生门禁、按实际系统页几何核验四数组预算 |
| PostgreSQL 18 | `codex/dsga-pg18-v1-20261031` | `cc9430edcce7b863ffcf0db5ddf3cccc1395065c` | 硬预算实现、原生门禁、配置视图期望及两层权限测试修复 |
| Autoscaling | `codex/dsga-autoscaling-20261019` | `c7a3b3d777261ab2b9d85ca7dd67295f5ab20803` | 固定源码和可复用 workflow 为同一提交 |
| Helm | `codex/dsga-neon-helm-v3-20261019` | `bb9bce7aeae604366c687650cc318ca87d18ebb7` | 固定打包代码、模板和发行锁生成逻辑 |

Neon 的来源为触发 run 的完整 `github.sha`；所有 checkout 均固定事件 SHA 或锁定的依赖 SHA。
PG17/18 gitlink、`vendor/revisions.json`、`ci/distribution.json` 三处一起更新。PG14–16 保留原冻结锁。
`.gitmodules` 的 PG17/18 branch 元数据也指向对应功能分支；构建仍严格使用完整 gitlink SHA，
不会在构建时追随分支的新 HEAD。
PG17/18 reusable workflow 的 `uses@SHA` 也与内核源码锁一致，同一次 Neon run 中执行真实构建及回归；
不是查询另一个分支上曾经绿色的结果。

Autoscaling 在固定源码的一次性 root、只读根容器中执行完整默认 Linux Go suite。
`owner`/`journal` 顶层测试按稳定 hash 分成八片，其余包在第零片完整执行；
保留 `-race -count=1`、Example 与 Fuzz seeds。独立聚合器要求八片来自同一
source/architecture/run/attempt，并核对原始枚举、唯一执行和全部终态；现有条件 skip
逐项记原因，不算通过。任一片失败、取消、缺失或混入旧 attempt 都阻止后续镜像构建。
重跑应选择 **Re-run all jobs**，避免拿旧 attempt 的成功片拼成本次证据。

依赖必须按 PG 两个分支 → Autoscaling → Helm → Neon 的顺序推送完整提交。依赖未在远端可读取时，
GitHub 无法展开 `workflow_call`。不要 squash/cherry-pick 后继续使用原来的 SHA；若需要改写提交，必须
重新核对并更新所有源码锁和 workflow 引用。

## 统一版本与制品

版本格式：`0.2.0-dsga.<GITHUB_RUN_ID>.<GITHUB_RUN_ATTEMPT>.amd64`。

同一版本用于镜像 tag、Git tag、Release、全部 Chart version 和发行锁。镜像仍发布到
`docker.io/williamluckyli`；部署使用 `distribution-manifest.json` 中的 digest，而非可变标签。
当前矩阵共 49 个镜像，数量由锁自动计算和核对，不能使用过时的“46 个镜像全部通过”文案。

| PG 版本 | profile | 运行时镜像 | 第三方全扩展测试镜像 |
| --- | --- | --- | --- |
| 14、15、16、17 | `full` | `compute-node-vN`、`vm-compute-node-vN` | `neon-test-extensions-vN` |
| 18 | `dsga-core` | `compute-node-v18`、`vm-compute-node-v18` | 不声明、不生成 |

PG18 核心包仍编译 PostgreSQL/contrib、Neon 必需扩展、Compute 进程和现有伴随组件；不把 PG17
第三方扩展清单直接套给 PG18。三个 `pg_stat_statements` 补丁目标文件与已审核 PG18 来源为相同 blob，
PG18 的 1.11→1.12 SQL 不重新定义 reset 函数，可复用现有授权补丁；CI 另以实际构建和 smoke 验证。
Compute 二进制 `postgres --version` 带准确 PG SHA，组装后从刚发布的 image digest 拉取运行并核对。

Helm 工作流产生 15 个 Chart 包、12 组 PG17/18 digest values、原始 distribution manifest、
`dsga-release-lock.json`、`helm-package-receipt.json`、`index.yaml` 和 `HELM-SHA256SUMS`。
最终 Neon Release 再附 Autoscaling capabilities、说明和覆盖全部文件的 `SHA256SUMS`。
仅六组运行组件的镜像由此次发行绑定；控制面及其它独立产品镜像不因此被重新构建。

## 门禁与发布顺序

1. `prepare` 校验镜像清单、PG 三重来源锁、跨仓 workflow SHA、凭据是否存在与 amd64 平台；
   只执行不依赖运行中 PG 的发行契约测试。不能全目录 discover 导入缺环境的原生脚本。
2. 同 run 的 PG17/18 workflow 实际执行断言构建、官方 `make check`，以及 PG17 SQL/isolation
   或 PG18 resize/配置原生测试。记录二进制 hash、源码、run/attempt；任何失败或必需 skip 均拒绝。
3. 所有编译使用准确源码，基础镜像、组件、VM builder/kernel/daemon 互相引用实际 digest。
   每个会写 registry 的 job 在写前核对 run/attempt；核心镜像还实际加载每个 PG 大版本的
   `postgres --wal-redo`，防止漏装 `neon_walredo` 等依赖。
4. 聚合时要求完整 49 镜像集合及每项 name/kind/repository/commit/tag/digest/架构/run/attempt
   一致；Autoscaling 的 capabilities、kernel image、11 个镜像摘要必须与最终镜像记录逐项相等。
   PG17/18 两份原生测试收据必须来自本次准确 SHA、同 run/attempt。
5. Compose 用 digest 固定存储和 Compute 镜像，运行 SQL、实际交付扩展及 Compute 本地文件系统
   移除后的持久化恢复；PG14–17 保留已有全扩展门禁，PG18 明确使用核心包预加载清单。
6. 前述全部成功后调用 Helm 可复用 workflow，离线 lint/template、包内锁和最终包 metadata 校验。
7. 最后才创建准确源码 Git tag 和 draft prerelease，上传完整资产后转为 published prerelease。
   `--latest=false` 固定使用；不会执行任何镜像 `:latest` 提升。已发布同版本拒绝覆盖。

## 重跑与失败恢复

所有跨 job artifact 名包含当前 attempt，或包含已含 attempt 的发行版本。下载只接受本 attempt，
不会把第一次失败生成的部分记录与第二次新镜像拼在一起。PG 原生 workflow 的并发分组在被 Neon
调用时包含调用 run ID，多个 Neon 分支锁定同一 PG 也不会互相取消。

失败后使用 GitHub **Re-run all jobs**。仅 Re-run failed jobs 会复用旧 prepare version；下游写入前
会因 run_attempt 不一致拒绝，从而保护旧镜像标签和收据。排障先看失败 job 的实际日志及同 attempt
artifact；不要手工把未完成发行的 tag 提升为 latest。

旧 `dockerhub.yml` / `validate-existing.yml` 继续只允许 main 的 legacy 入口；遇到这份带
`postgres_profiles`/`helm_commit` 的 DSGA 锁时会在昂贵构建前明确拒绝 latest 流程。功能分支应使用
新入口。现有 main 没有被合并或改写；将来把候选转为 main 稳定发行需要单独审核稳定发布策略。
新 workflow 若尚未进入默认分支，GitHub 的手工 dispatch 可能尚未注册；此时按上述顺序 push 到
已配置的功能分支即可自动触发，不要为使按钮出现而合入 main。

## 凭据与权限

Neon 仓库需要既有 `DOCKERHUB_USERNAME`、`DOCKERHUB_TOKEN` Secrets，账号需对
`docker.io/williamluckyli` 的组件仓库具有 push 权限。GHCR 中间编译缓存/CI 测试工具镜像沿用
`GITHUB_TOKEN` 的 `packages: write`；最终 Release job 单独获得 `contents: write`，其余保留最小权限。
如果组织限制可复用 workflow/Actions，按平台策略为这些精确自有仓库与固定 action SHA 放行；
不把 PAT 写入仓库或日志。

## 当前候选资格边界

统一工作流明确仅支持 linux/amd64，避免已有 Chart 的 amd64 约束误配 arm64。发行清单固定：

```json
{
  "release_tier": "candidate",
  "vm_system_mode": "legacy",
  "qualification": {
    "production_ready": false,
    "physical_memory_release_qualified": false
  }
}
```

Autoscaling 公开默认 kernel 构建不包含已审核 normal profile 的完整 artifact/config/notes 来源。
本流程不会放宽 allowlist，不会把默认 kernel 的成功编译伪装成 normal 资格，也不会自动打开 Chart
的实验、Compute、自动伸缩或 Qualifier 开关。PG18 的现有受管 Chart 要求 trusted-runtime-v1，
本候选 legacy VM 不满足该要求，PG18 values 中正确 digest 可供后续使用，但直接启用仍由 schema 拒绝。
要交付 normal 物理缩容发行，需要补齐审核内核来源、normal-split-v3 构建计划、PG ELF 身份、实例
bootstrap/资格证明，并对这组实际新镜像做 KVM/目标 K8s 的联合验收。

## 本轮本地验证

- 38 项发行契约/源码锁/配置缓存测试通过，包含 23 项新增跨仓拒错与实际 record 生产者形状检查。
- 修改的 GitHub Actions 通过 actionlint；shell 通过 `bash -n`；提交前 `git diff --check` 通过。
- 使用 Neon `make_manifest` 生成的明确合成镜像记录，真实执行 Helm 工具链打包 15 个 Chart，再由
  Neon 最终资产逻辑验证 Helm 收据、包 hash 和完整校验和。此项验证跨仓数据合同和打包代码，
  没有构建/推送任何数据库镜像，也没有执行云端 Actions、KVM 或 Kubernetes 部署。

## 已完成的独立云端门禁

| 轨道 | 实际 GitHub Actions | 结果 |
| --- | --- | --- |
| PG17 `203e1e83…` | [42456653415](https://github.com/william-lbn/postgres/actions/runs/42456653415) | 225 项上游回归、DSGA SQL、5 项 isolation 全部通过；三处精确预算及错误预算拒绝均有真实结果 |
| PG18 `cc9430ed…` | [42456320694](https://github.com/william-lbn/postgres/actions/runs/42456320694) | 230 项上游回归、10 项 resize、4 项 configuration 全部通过；入口与函数权限分别验证 |
| Helm `bb9bce7a…` | [42454937549](https://github.com/william-lbn/neon-helm/actions/runs/42454937549) | 61 项工具测试、15 个 Chart 及 34 个负向渲染场景、6 组 DSGA 模板检查通过 |

以上为对应准确提交的独立质量证据，不是统一镜像已发布的证明。统一发行仍在自己的同一 run/attempt
内重跑必需原生门禁，并消费实际镜像 digest 生成最终 Helm 包；是否完整发布，以该 run 的成功状态、
`distribution-manifest.json`、`helm-package-receipt.json`、`SHA256SUMS` 和公开 Release 为准。
