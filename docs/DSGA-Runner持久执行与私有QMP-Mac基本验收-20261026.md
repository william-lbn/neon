# Runner持久执行与私有QMP：Neon接入记录

本批遵守2026-10-26 17:29:30冻结截止，不再fetch/merge后续上游；PG17仍为
`b078736a4cfdfccce69914feefb5b5e56560c1e8`。Autoscaling依赖由
`ci/distribution.json`锁定为`c7ae07dacf33f652e5d6370b57a17057014c2e15`；只演进既有三个核心功能分支，不改控制面/SDK或其他PG。
本批Rust/PG源码不变，不重复此前PG/HTTP/Provider验收。

实际Runner已接出生→固定Kube拉取原VM意图→单VM文件锁/完整写前日志→私有
Unix QMP单次set→实际requested/device size/base+plugged读回→原UID不可变
回执和状态发布。Controller只直接读取原Runner/Pod/Node来源，受管模式关闭
TCP QMP/manual入口，显式SA/sysfs/Downward身份/状态卷由实际Pod构造器核验。
Lease命名/UID/owner规则与Agent共用；高floor不能清除旧pending或重写旧epoch。

非no-op原B与目标A不同，may_have_sent先fsync再唯一发送；回复丢失只读原A。
原唯一set未读回完整A前保留未知，不重发、不释放预算。no-op零写；prepared
恢复保留not_sent，生产显式重放传输仍待接。完整原回执先持有再允许新动作。
Host动作期间必须持有max(before,target)+overhead；身份/资格/来源变化拒绝新写。

Mac去重**20顶层/110含子项完整测试名通过，最终0失败/0跳过**，同名补检替换
而不累加。真实Mac文件锁/fsync/rename、Unix socket；Kube API/UID-RV、QEMU/
KVM、Prepare/Host和PG/Root物理领域是明确协议夹具。包含完整扩缩消费、三类
已提交丢回复、旧未知接管、源码前置来源反向、实际Pod组装和共享Lease合同。
readonly vet通过，Runner/Controller的Linux amd64/arm64仅编译通过、未运行。
源码/产物/日志哈希、原失败和后续质量检查见[机器收据](DSGA-Runner持久执行与私有QMP-Mac收据-20261026.json)。

本模块之外**仍缺核心代码**：Agent typed ResizeVM/ObserveVM及当前Root/PG
复核、原Runner UID来源结算/长期绑定；可信完整Prepare/系统剩余/过渡/安全
资格生产者和真实Host/Scheduler持有释放；显式Runner重放通道、取消/未知结清；
安全normal下降、首次准入与受控跨boot/完整公开生命周期。不得用自报CM、旧
VMSettlement、当前VM状态或本模块通过代替这些实现，整体仍不能封版。

Linux后验：真实apiserver/RBAC/领域对象写权限和显式SA、QEMU runas与私有
socket权限/KVM启动、异步/部分virtio-mem热拔、内核普通页/HugeTLB/物理归还、
真实Host预留/迁移互斥及性能长稳。emptyDir只保原Pod内WAL，丢Pod/WAL由
Birth阻断重新出生，跨Pod受控恢复未实现，不能称仅待Linux。控制面完成后再
移植统一E2E。本批无镜像、部署、主分支合并或生产发布，默认关闭、Goal继续。

完整AutoScaling模块说明：[固定源码与验收](https://github.com/william-lbn/autoscaling/blob/c7ae07dacf33f652e5d6370b57a17057014c2e15/docs/DSGA-Runner持久执行与私有QMP-Mac验收-20261026.md)。
