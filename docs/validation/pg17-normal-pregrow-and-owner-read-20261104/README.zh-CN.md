# PG17 首次扩容与 Owner 资源读取的阶段修复

本次只收口两个直接阻断真实内存伸缩的问题。首次扩容保留出生数组 A 硬额度，避免合法的 N 增长因 A 已有余量而被拒绝；安全 normal 降限仍必须降低真实数组域并保持四数组释放证明。正式 Owner 为 resource_state 增加有界公平读取，不让周期审计连续占用串行执行队列；每次读取后仍须完整审计及原到期续租，写命令和恢复入口保持原链。

三根 normal 精准回归及四根 Owner 精准回归分别通过，均有准确源码、race、退出状态、资源及独立审查记录。候选 Guest 已经过原 Collector、完整文件系统比较、镜像/原盘一致性及八个关键文件的真实磁盘读回；Agent 薄镜像只替换正式 ELF，配置和安全权限保持。过程中的夹具错误、工具 frontend 超时及打包脚本后置异常均保留，不作为测试通过；续接只补未完成的验证，没有重复构建已完成镜像。

ci/distribution.json 的 autoscaling_commit 指向包含两处修复的同分支提交。实际候选组件保留各自真实编译快照：Root/Builder 来源为 normal 修复提交，Agent 来源为其后的 Owner 修复提交，Compute 沿用已验证的 v17/v18 初始化修复。详细身份见 receipt.json，不能把组件不同构建快照误写为同一个源树。

目前候选尚未通过部署后的真实 Grow、安全 normal Shrink、Regrow 和自动策略。接下来经 SSH 预载 worker-117/118/119，使用自有 Helm 更新受影响组件，再按原正式生命周期保留数据换新物理身份。测试聚焦本功能及直接受影响组件，不测 K8s 平台通用功能。PG17/PG18 gitlink 和公共合同未变；PG18 独立轨及最新完整控制面目标仍未验收。

在 118 可用 docker images --filter=reference='neon-pg17-visible/*' 查看已拉入 Docker 的可见标签；精确 OCI digest、CRI k8s.io 预载和部署状态分别归档，Docker 标签存在不代表已经部署。
