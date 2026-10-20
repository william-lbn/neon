# DSGA 执行与恢复记录

更新：2026-10-20（Asia/Shanghai）。设计以 V3 为准。Goal 已按项目所有者授权启用，
保持 active；单模块完成不能宣称整体交付或标为 complete。当前没有预算限制，
中断后依据真实 Git/远程状态和本地日志接续，不重复后台构建或 apply stash。

## 当前可核对检查点

- PG17：`codex/dsga-pg17-v3-20261019`，S3
  `7a1ff18108b311b6787dc5fb3c6dd9d13de5a462` 已推送并在 GitHub 核对。
- 独立官方维护：`codex/upstream-pg17-20261020` /
  `4a39412f154796461f404785d369a3dc9424fcdc` 已推送并核对。
  含 Neon PG17 `56692df` 与社区 `91246c7` 的全部祖先；由 `873715d889` 合入功能分支。
- Neon：`codex/dsga-neon-v3-20261019`。上个已推送检查点 `de8f02a9ae890f752d0eed844fbc8da54210293d`；
  本批正在提交 S3 绑定与16项真实集群测试，gitlink/两份锁已改为上述7a1ff18108。
  后续是否已提交/推送必须核对 Git 和 GitHub，不能依据这条提交前记录推断。
- Autoscaling：`codex/dsga-autoscaling-20261019` / `ef6cd154…`，尚无 DSGA 业务实现。
- control-plane：`codex/dsga-control-plane-v3-20261020` / `928eec95…`；
  Helm：`codex/dsga-neon-helm-v3-20261019` / `648488a`；均尚无 DSGA 业务提交。

## 本批实际验证

- 未含 DSGA 的独立维护：构建、独立 rmgr、基础 check-world 79组/660项报告检查通过。
- 维护+S3 组合：45项真实集群通过86.968s；原生1regress+5isolation通过；
  动态关闭/开启 check-world 各81组/666项报告检查通过。TAP未启用。
- 按最终 PG SHA 由主仓重新 configure/build，`ci/test-dsg-postgres.sh` 安装测试库、
  clean/rebuild rmgr 后45项真实集群通过84.343s，零skip。日志
  `build/neon-integrated-v3-shrink-91246-{build,lifecycle}.log`。
- 其他本批日志：`build/v3-shrink-91246-{build,module,lifecycle,native}.log`、
  `build/check-world-v3-shrink-91246-{off,on}.log`。
- 寿命探针反向验证：仅回退归还顺序仍由已有 VA/L 防护保护，不能称旧版缺陷；
  同时显式引入最后unpin后 `buf->buf_id` 读取则 Backend 信号终止、测试按预期失败。
  两份源码逐字节恢复、重编后正向通过。日志 `build/v3-shrink-collision-{negative,restored}.log`。
- 完整 Neon 扩展构建新增发现：官方主仓扩展仍注册已被最新 PG17 删除的三个
  set_lwlsn_block hook，并使用已删除核心 LastWrittenLsnLock，导致编译失败。
  已确认官方 PG 删除提交 bb65878e60 说明其不再必要；下一模块须保留实际 smgr 的
  LSN 更新与缓存正确性，不能仅删除功能。communicator Rust 静态库已成功构建；
  C 扩展尚未构建成功，日志 `build/neon-pgxn-s3-{communicator,neon-compile}.log`。

## 不得隐去的 TAP 失败

首次启用 TAP 的独立维护组合整体失败，8个文件失败，详见
[基线审计](DSGA-TAP基线验证-20261020.md)。其中备库重启后缺表问题未确认根因。
这是未含 DSGA 的维护组合证据，不是原始官方各头分别失败的证据。Mac可复现，
不能归入Linux待验，也不能通过关闭TAP声称问题已解决。日志
`build/check-world-pg17-upstream-91246-{basic,tap}.log`。
工作区 Perl 依赖已装在 build/perl-local；不改系统 Perl。install-world 的 SGML
因 DocBook DTD 缺失失败，官方 install-world-bin 的代码/扩展安装成功。

## 正在进行与后续顺序

1. 完成 Neon S3 绑定提交/推送并核对；不把本地 commit 当远程成功。
2. Neon 扩展最新 PG 兼容修复。`neon_test_utils/neontest.c` 已有未提交改动：
   clear_buffer_cache 覆盖真实H，逐项短Guard，复制提示后释放Guard再进入可能写回的
   ReadRecentBuffer/ReleaseBuffer；须实际构建、加载与并发测试后另作模块提交。
3. TAP 失败继续定位，尤其真实备库重启持久化；不能在未查明时发布生产。
4. 正式 capabilities/request/status/cancel/memory_status、独立请求回执和有界历史、
   管理认证、完整私有/后台预算与quiesce、Compute/Monitor/Agent/Scheduler/Controller/
   Runner执行fencing与对账。当前测试函数不是正式入口，尚未编写的业务不能包装成待Linux测试。
5. 完整 Neon 存储链本地可行端到端；逐版 PG16/15/14 维护升级和语义移植；PG18
   单独处理发行集成与AIO。每版开始前重新核对完整官方维护。
6. control-plane Driver、Helm 分支系列接入，遵守各自 signoff/契约/schema要求。

## 恢复与环境不变量

- 旧stash `6f002a31196f2e623e8386c38a649ff87be2ada4` 已apply一次，禁止重复apply。
- 新S3暂存 `f5c4dfc54b3600a4191b4a7f06d2437db3b8403b` 已在合维护后apply一次，
  并核对补丁一致，禁止重复apply。两份均保留；新标记见 build/v3-s3-upstream-stash-*。
- 实际源码在 repos/neon/vendor/postgres-v17；repos/postgres 仅符号链接。
- 不创建子代理，不构建镜像，不准备/执行 K8s，不合主分支，不发布生产。
- Linux/K8s/NeonVM/KVM/HugeTLB/真实物理回收/迁移/目标性能与长稳按V3后验。
  这些环境边界不减免当前完整代码和Mac可执行验证。
