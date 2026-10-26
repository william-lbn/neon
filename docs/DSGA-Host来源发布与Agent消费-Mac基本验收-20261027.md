# Host来源发布与Agent消费的Neon绑定（2026-10-27）

自有Autoscaling原分支`codex/dsga-autoscaling-20261019`代码
`0d9fc2c9a16fcf1c7e052038e40c8eb642e8b850`、验收及收据
`a305c389618570ae527efcd8a9f7b1e8e55af758`已先推送并核对完整远程SHA。
Neon的`ci/distribution.json`绑定后者。完整
[中文模块验收](https://github.com/william-lbn/autoscaling/blob/a305c389618570ae527efcd8a9f7b1e8e55af758/docs/DSGA-Host来源发布与Agent消费-Mac验收-20261027.md)
及本仓[逐字节收据](DSGA-Host来源发布与Agent消费-Mac收据-20261027.json)说明
13源码、实际用例、失败/中断保留、原日志和准确仅编译产物摘要。

原Node容量CAS→原不可变CM→Node发布原CM UID，最后CAS前消费者不执行。
新Publisher读取原VM/Lease/Prepare/Outside/Runner实际观察和Pod，来源丢失或
重建不自动补证/释放。实际Agent新增只读Consumer，完整引用与摘要先写VM
UID/RV命令，复核后结算ReserveHost，未知只读原命令/UID；Agent无新增Node/
CM写权限。Host v2与Node发布模型v2要求先升级读者；旧29真实读者拒绝v2，
VM Model v4、Runner record v1及PG ABI不变。

Mac去重20顶层/142含子项最终通过、0失败/跳过，7包readonly vet、最终增量
lint0问题，准确Agent/Runner双Linux架构4程序仅编译通过，未执行，不是镜像。
组合包含实际Host生产/消费到Agent/Runner/预算/准入，但Kube/QEMU、Prepare/
Host资格及PG/Root内核仍为明确fixture。原变量/UUID/codec/fake锁等失败记录
保留，最后同名用例只替换不累加；不重复未变PG/HTTP/core225。

**本模块还没有实际Scheduler调用入口，整体核心未完成。** 全授予/重启恢复、
选主引导、其他工作负载完整计账、首次VM持有、显式释放/迁移及剩余可信
Prepare/预算/normal/首次准入/公开操作和受控跨boot仍须代码；不能列成只待
Linux。控制面对标任务完成后再移植并升级其SDK565读者、统一端到端。

PG17保持`b078736a4cfdfccce69914feefb5b5e56560c1e8`及三处准确绑定；控制面
保持`d84c0b73004a7d8cc3e89f3c3f0e3f06cfa8cba9`。上游冻结于2026-10-26
17:29:30，没有fetch新增提交。Linux K8s/RBAC/KVM/NeonVM/HugeTLB/实际物理/
Host/迁移/性能长稳后验；默认关闭，不构建镜像、不部署、不合main、不发布。
