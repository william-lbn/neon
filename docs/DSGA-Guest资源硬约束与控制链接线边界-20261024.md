# Guest资源硬约束与控制链接线边界

依赖AS `7ee052a3347132b538723cda98ae7d1ac02fd99e`已先推送自有
`codex/dsga-autoscaling-20261019`并核对远程SHA，Neon发行锁绑定该提交。
PG17 gitlink、`ci/distribution.json`和`vendor/revisions.json`中的PG17仍一致
指向`cbabd5de541a1340ab4675cfc3ef14b3a9372671`，本批未修改PG内核或Neon运行代码，
不重复未变PG/SDK/Router功能测试，不构建镜像。

实际代码位于AS `pkg/dsga/resourceguard/`及
`neonvm-daemon/cmd/resource-guard/main.go`；详细中文说明见同仓
`docs/DSGA-Guest资源硬约束执行器-Mac基本验收-20261024.md`。

完成的范围是Linux实际硬约束适配器及其恢复规则：固定真实cgroup/root权限/
Postmaster进程归组、memory.max、禁swap、全部HugeTLB hstate禁overcommit及
池大小读回，VM/Guest/cgroup身份、连续revision、issuer floor、私有持久日志、
原意图与旧Held约束、取消不放开同步写锁、原查询及当前owner显式原样重放。
不监听旧neonvmd无认证网络端口。已有日志/身份丢失和换Guest boot均拒绝自动重置。

最终Mac新增7项race检查通过，0fail/skip；两新组件vet通过；Linux amd64/arm64
程序交叉编译通过。最终8个Go输入文件在验证前后摘要一致；失败的初始跨平台
类型/boot UUID检查及修复证据保留。原始日志在工作区
`build/dsga-resourceguard-final2/`，全套只覆盖新增模块，内核是明确的测试夹具，
没有将Mac文件测试称作真实cgroup、HugeTLB或端到端验证。

尚未接齐的业务：原VM受管权限/启动、Compute固定授权通道、PrepareBudget/
GuestReady/FinalizeBudget及独立启动准入、完整quiesce、系统/Host承诺及最终VM
执行/恢复、公开worker和生命周期。特别是旧官方cgroup把限额管理权交给postgres，
目前内核适配器会拒绝该权限，不能不改受管权限便宣称已可生产运行。外部上界
必须由真正的持久承诺生产者提供，root执行器回执不单独构成完整预算或VM许可。
跨PG/Guest boot及旧owner部分动作的受控关闭恢复也仍需编码，不能算Linux待测。

后续Linux验收：真实权限和进程出生/退出/迁移、内核限额回收/OOM、swap、全部
HugeTLB池的预留/部分分配/surplus/HVO、SIGKILL与同步写锁/掉电恢复、物理回收及
完整NeonVM/K8s端到端/性能。当前Goal仍active/incomplete，仅PG17，默认关闭；
未部署、未合main、未发布。交叉编译成功不能代替这些目标环境证据。

同轮控制面维护：完整合入自有主线5ee3bc4，27个文件与主线相同，OpenAPI保留
两侧协议。受影响Go9项race及Auth新增1项、vet/类型检查/Web构建通过。维护合并
b009b4a与验收7773b5b已推送自有功能分支；未触发任何部署或重复未变DSGA测试。
