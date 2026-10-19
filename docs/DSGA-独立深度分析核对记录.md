# 独立深度分析核对记录

日期：2026-10-19。输入：工作区 `DEEP-ANALYSIS-20261019.md`。后续设计继续以 V3 为准；
本记录只处理审核结论与实际代码的差异，不扩写一套竞争方案。

## 采纳的结论

审核时完整 Shrink、外部 API、Compute/Monitor/Agent/NeonVM 联动尚未实现，R01/R04
均为后续必须解决的事项。恢复实施后的已关闭子片与剩余项见[当前实施状态](DSGA-当前实施状态.md)。Mac 不提供 Linux 物理回收证据，check-world 汇总不能代表
DSGA 全部语义完成度，也不能据此给出生产可用结论。将当前状态收敛到单一文档，
保留模块验证记录，冻结 V3 主体并集中实现。性能、真实 Linux Provider 和完整
端到端分别验收；不根据未经权重定义的“30–35%”推算进度。

## 锁序问题：采纳风险，修正具体推导和修法

审核准确定位了已提交 `StrategyFreeBuffer` 的 mapping→access 路径，M2f 必须消除它。
但以下两点不能直接作为实施依据：

1. 当前固定 PG17 的 `LWLockAttemptLock()` 对共享获取只检查 `LW_VAL_EXCLUSIVE`。
   等待者存在及 `LW_FLAG_RELEASE_OK` 不等于排他持有位；后者用于唤醒控制。
   因此“只要排他等待者排队，后来的共享者必然被阻塞”的三方死锁推导不适用于
   当前代码，不能把它写成已经证明或复现的现有故障。持 accessX 的未来回收者
   与持 mappingX 的归还者则确实构成必须消除的反向锁序风险。
2. 消除一个反向边即可打破这条环，不需要同时改掉两个方向。当前选择保留
   `access→mapping/header` 的受保护扫描路径，并在归还路径完全移除 access 获取。
   不采用“一律 mapping→access”的建议。`InvalidateBuffer` 还明确禁止自身持 pin，
   所以也不能直接用“先 pin 再原样调用 InvalidateBuffer”替换 Guard。

M2f 的验证为：returner 持真实 mappingX 后暂停；另一进程持 accessX 后暂停；
先恢复 returner，必须能够调用 StrategyFreeBuffer 并结束，再恢复 fencer。
若仍有 mapping→access，此顺序无法完成。源代码审计与确定性测试共同作为证据，
不能仅以压测“没有遇到死锁”代替。

## 其他结论的边界

- PG 的 `shared_memory_size` 不包含独立 mmap，必须在新状态接口解释统计边界；
  但操作系统仍能感知 mmap，不能说 cgroup/overcommit 因 PG 未登记就“不知道”映射。
  具体计费和预留成本依内核/backing/配置实测，不概括为所有配置全部立即驻留。
- 长 checkpoint 可能超过 Grow 期限并导致拒绝，属于明确活性问题；只有 checkpoint
  实际持续超过请求期限时才必然超时，不能概括为所有 checkpoint 都使 Grow 失败。
- 只读管理接口仍需权限、ABI、快照一致性和边界检查，不能称为“零风险”。
- “Neon 主仓零业务”只可用于尚未实现的 DSGA 外部控制链。主仓已存在单独维护
  的 Rust WAL 解码和 C rmgr 兼容改动；它们不证明内存控制链完成，也不应被忽略。
- 继续尊重项目所有者的环境约定：Mac 完成能执行的业务验证，Linux CI/实机另验；
  不因暂缺 KVM 就暂停所有可验证的内核、协议及接口工作，也不虚报物理完成。

## 恢复实施后的引用审查补充

M2f已消除归还路径的反向锁序；S2已拆分checkpoint工作区租约与短数组保护，具体
验收以当前状态和模块记录为准。R05继续发现恢复快速helper缺Guard，以及非自身pin
的IO等待者在ERROR/FATAL先放Guard后仍引用CV的窗口；已单独实现/验证对应修复。
临时pin仅在WaitIO内部取得并在返回前释放，再由InvalidateBuffer按原协议复核，
不是在持pin状态下直接调用原样InvalidateBuffer，仍遵守前述限制。
FATAL先释放所有LWLocks，因此不能只登记退出回调；必须有原生pin覆盖CV清理到资源
释放的间隔。真实备库和正常/取消/终止交错是本子片证据，尚不关闭全部R05。

源码依据：PG `src/backend/storage/lmgr/lwlock.c` 的 `LWLockAttemptLock`/
`LWLockWakeup`，`src/backend/storage/buffer/bufmgr.c` 的 `InvalidateBuffer`/
`DropRelationBuffers`，以及 M2f 的 `StrategyFreeBuffer`。
