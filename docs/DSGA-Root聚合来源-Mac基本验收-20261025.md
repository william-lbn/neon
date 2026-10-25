# Root聚合来源的实际接线与Mac验证

本批继续沿四个既有服务分支演进，不重建、rebase或强推。只做PG17当前链路，
PG17仍为 `5462f1de54a97b9a9235c69d64d31cb0825dcea5`，Neon扩展仍1.15。
fresh fetch核对Neon fa504217、AS a4bce09e、Neon PG17 56692dfb、社区PG17
bd807012、控制面main af5e704，五线完整维护均missing0，无空维护提交。

AS模块 `a3d91b8a10bbd7e20b6a3710ae220bf9018d1621` 已先推送自有既有分支并
核验远程完整SHA。它从认证root View的真实限额读回生成不可变聚合来源，接入
现有Agent/PGWorker GuestReady路径，新审计v2必带 `pg_aggregate`。来源绑定原
Guest/cgroup、Bootstrap/Held和出生身份；旧v1保留历史原样，未知不借旧来源结案。
后续动作须重新Match认证View，摘要本身不认证，也不是跨系统原子快照。

金额仅为PG计费域memory.max加整个Guest HugeTLB池；不把Outside声明认证成
系统上界，不再加Shared/DSM/RSS/已用或空闲池。进程迁移不迁移既有页收费，
memory.max不单独保证免OOM或严格瞬时物理上界；互补来源仍须覆盖其他计费域。
说明依据[Linux官方cgroup v2文档](https://docs.kernel.org/admin-guide/cgroup-v2.html#memory-ownership)。
详见[AS原分支实现与验证](https://github.com/william-lbn/autoscaling/blob/codex/dsga-autoscaling-20261019/docs/DSGA-Root聚合上界生产-Mac验收-20261025.md)。

Mac实际Go race首组5顶层/14含子项通过，0失败和测试跳过；resourceexec没有独立
test文件，日志有1包级skip，实际执行器由owner测试调用。新增空资源域子项随后
单测通过：效果后空域阻止Completed，恢复只Observe原命令，仍1次POST，审计v2
来源吻合；未重复首组已过子项。去重共5顶层/15含子项，readonly vet退出0。
Kube/root内核/PG成本为协议模型；实际TLS/双客户端/Session/typed Store与UID/RV
CAS真实执行。这不是Linux真实资源或整集群端到端验收。

实际 `autoscaler-agent/cmd` Linux amd64/arm64交叉编译均通过，未执行ELF、未
构建镜像；编译来自提交前同业务工作树，不宣称发布制品/VCS标签认证。原生DSM8、
core225、相关隔离5及正式HTTP均未改，不重复测试或重复计数。

控制面准确自有模块 `v0.0.0-20261025142910-a3d91b8a10bb` 显式下载并核验
Origin.Hash，go.sum最小追加2条，readonly vet退出0；无业务入口改动。Neon仅
更新AS发行来源锁，PG gitlink和两份PG锁保持同一5462。机器哈希及绑定门禁结果
见[收据](DSGA-Root聚合来源-Mac收据-20261025.json)。

完整可信Prepare/Finalize、SystemOutsidePG生产者、后台/私有持续约束或Quiesce、
降低上限不造成OOM及必要后台活性、首次准入、Host/最终VM/公开worker/受控跨boot
仍须代码；这些不是Linux待测项。Linux后续真实cgroup/HugeTLB/内核PTE/THP、
物理释放、KVM/NeonVM及性能长稳另验。本轮不构建镜像、不准备或执行K8s部署、
不合main、不发布；默认关闭、所有VM许可false，整体Goal active/incomplete。
