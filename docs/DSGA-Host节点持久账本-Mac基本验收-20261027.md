# Host节点持久账本原语与Neon版本绑定（2026-10-27）

源码位于自有Autoscaling原分支`codex/dsga-autoscaling-20261019`。
代码`6159109676a178c5294d0d78625651e812116714`、中文验收与机器收据
`4e9656a3d6687a1fcfb98cfdc0003ad42be721c8`均已推送并完整远程核对。
Neon的`ci/distribution.json`绑定后者，按先依赖再消费者顺序提交。
完整[中文验收](https://github.com/william-lbn/autoscaling/blob/4e9656a3d6687a1fcfb98cfdc0003ad42be721c8/docs/DSGA-Host节点持久账本-Mac验收-20261027.md)
与本仓[同内容收据](DSGA-Host节点持久账本-Mac收据-20261027.json)可追溯。

固定原Node/Lease UID、完整请求及VM/incarnation/Pod持有，Node UID/RV CAS保存；
同VM/Pod按峰值、不同Pod分开计账。接管保留旧承诺，迟到旧RV拒绝；丢回复先
读原持有，丢/坏/换源不自动初始化。条目/原文/容量/序列有界，不自动淘汰。

Mac新增7顶层/26含子项race通过，0失败/跳过；最终Linux目标增量lint0问题、
原生readonly vet退出0。两个Linux架构的包测试程序仅交叉编译，未执行，也
不是Scheduler业务二进制。原23条和后1条静态问题已补正，原日志保留；不重复
未变PG、HTTP和此前VM/预算模块。准确文件、用例及摘要见收据。

HTTP/RFC6902服务器模拟apiserver，Node/Lease/额度/VM/PG身份为显式fixture。
**实际Scheduler授予/恢复与选主、Host源生产及显式释放尚未接入；域外参数未经
本原语认证。** 不将此基础模块或immutable声明当真实Host预留完成。
可信Prepare/剩余系统上界、安全降normal及持续私有/后台约束、首次预算/准入、
策略/Monitor/Agent公开生命周期、取消未知结清及受控跨boot仍须核心代码。

PG17固定`b078736a4cfdfccce69914feefb5b5e56560c1e8`，gitlink/两锁与实际检出一致。
控制面固定`d84c0b73004a7d8cc3e89f3c3f0e3f06cfa8cba9`，本批未修改；完成独立开发
后再移植并统一端到端。上游仍冻结于2026-10-26 17:29:30，没有再次fetch。
Linux K8s/RBAC/KVM/NeonVM/HugeTLB/真实物理回收/Host/迁移/性能长稳另验。
默认关闭，整体核心尚未完成，不构建镜像、不部署、不合main、不发布。
