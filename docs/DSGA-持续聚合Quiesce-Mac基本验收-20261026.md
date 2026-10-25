# 持续聚合Quiesce与Shrink派发的Mac证据

本批沿四个既有服务功能分支演进。2026-10-26再次fresh fetch：Neon fa504217、
AS a4bce09e、Neon PG17 56692dfb、社区PG17 bd807012、控制面main af5e704，
五线完整维护均missing0。无空维护提交，无重建/rebase/强推或main合并。

## 已完成的实际模块

AS合同 `568e5645`、实际执行接线 `9740ec96` 及后续完整原来源绑定修复沿原
分支提交；最终精确SHA见机器收据。新Quiesce执行器消费同账本原Prepared，核验原
root出生/聚合来源和PG fence，完整命令同时持久原请求、关闭和当时原root来源，
写前UID/RV CAS后才发送。恢复/重放不得自动采用同数值的新Held；旧仅关闭格式
只读查询，保持Unconfirmed，拒绝补源、结案和写重放；ResizePG回执也须与完整
原命令来源完全一致。该复核修复作为独立中文提交，不改写已推历史。
正式DSM成员一次原生采样包含Prepared/Connection/Work；核原revision/floor/
容量和持续门禁ack、排除观察者后的前台idle。后台/2PC/DSM/私有不补0，不暂停
必要后台，保留原持续约束。完整外部系统成本仍须其受信生产者覆盖。

PGWorker恢复关闭方向和原Pending；丢回复只原查询，NotFound不重发，显式
Quiesce重放独立于最终开放/资源/Resize。新Shrink及可能形成新效果的重放在
ResizePG前重新核原Quiesced、root来源/出生和当前PG门禁/工作事实。正向逻辑
PG提交后仍停在独立物理ReleasePG步骤，不跳过Guest/VM/Host/最终预算。

当前持续策略保留或提高normal memory.max；无证明的normal降低在Quiesce关门
前及GuestReady新派发/重放前拒绝，未知只读查询仍可达。**可执行私有/后台预算
和安全降normal仍缺代码**，不以有限策略替代V3最终要求或用内核OOM回收内存。
PG17仍5462、Neon1.15、root协议与原生ABI均不变，所有VM许可false、默认关闭。
详见[AS实际代码与范围](https://github.com/william-lbn/autoscaling/blob/codex/dsga-autoscaling-20261019/docs/DSGA-持续聚合Quiesce-Mac验收-20261025.md)。

## 实际验证及失败保留

- Mac Go race新合同/执行器和受影响GuestReady/最终准入恢复：最终5顶层/33含子项
  去重PASS，0测试skip；quiesceexec无独立test文件的1包级skip如实记录，实际
  执行器由owner测试调用。覆盖并发一次派发、预算拒绝、丢回复/重建/显式重放、
  忙/未ack/漂移、正向PG派发与重开后拒绝、降normal拒绝。真实Session/TLS/
  typed Store和UID/RV CAS执行，Kube/PG原生事实/root内核为明确协议模型。
- 首轮真实暴露新执行器局部err遮蔽503，Unconfirmed保留但调用方误得nil；
  产品已修复。单位fixture容量512/256不相容也被正确拒绝，仅修正模型容量。
  仅替换失败与增强子项，随后新增正向/降限/并发/不完整预算，不重复已过等价项。
  后续源码复核发现关闭写前未保留完整原来源，补齐持久格式和两项恢复场景；
  受影响33项重新通过，替换前版31项证据，不重复累加计数。
- 新Go race/TLS→生产Router/JWT→Neon1.15正式SQL→准确PG17门禁1项PASS：真实
  fence后关闭1次、Postmaster ack和前台idle，真实2PC槽1及DSM pinned 2MiB/
  4MiB额度仍保留；新Quiesced消费通过、降normal拒绝、零Resize POST。此项
  root与外部成本明确为模型；不声称全栈原生执行器或整集群E2E。首轮测试漏
  fence如实Stale，补齐生产执行器已有顺序后只替换失败1项，未放宽产品规则。
- readonly vet及原生tag/最后新增测试的补充vet退出0；实际Agent Linux amd64/
  arm64交叉编译均0，未运行ELF、未构建镜像。受影响Rust测试clippy
  `--no-deps -D warnings`退出0，依赖原27告警保留。Native8/core225/iso5及未改
  正式HTTP不重复。机器收据列原始/替换日志、配方与制品精确哈希。

## 后续复现命令

本批实际采用分批、失败项替换及新增子项定向执行，去重结果以上述收据为准。
以下完整选择供以后相关代码变更后复现，不表示本批又全量重跑了一遍：

```sh
# 在自有autoscaling既有分支；本批Go1.25.13、Mac SDK14.4。
go test -mod=readonly -race -p=2 -count=1 ./pkg/dsga/budget ./pkg/dsga/owner \
  -run '^(TestContinuousQuiesce.*|TestPGWorkerContinuousQuiesce.*|TestGuestReady.*|TestPGWorkerFinalAdmission.*)$'
go vet -mod=readonly -tags=dsga_native_e2e -p=2 \
  ./pkg/dsga/budget ./pkg/dsga/quiesceexec ./pkg/dsga/pgexec \
  ./pkg/dsga/resourceexec ./pkg/dsga/owner ./pkg/agent

# 仍在autoscaling：只编译本机原生集成测试，后由隔离Rust fixture以匿名stdin传凭据。
go test -mod=readonly -race -tags=dsga_native_e2e -p=2 -c \
  -o /tmp/dsga-quiesce-native-go.test ./pkg/dsga/budget
```

再在Neon现有分支，提供已按准确PG5462构建的PG/test_dsg_region/Neon1.15安装
目录；不要改用系统PG或省略严格参数：

```sh
export NEON_TEST_PG_BIN=/绝对路径/准确PG安装/bin
export NEON_TEST_DSG_EXPECTED_SHA=5462f1de54a97b9a9235c69d64d31cb0825dcea5
export NEON_TEST_DSG_GO_BIN=/tmp/dsga-quiesce-native-go.test
cargo test --locked --offline -p compute_tools --test dsg_go_http \
  native_go_quiesce_consumes_real_close_and_keeps_retained_costs \
  -- --ignored --exact --nocapture
```

控制面仅准确锁定已发布AS模块并readonly vet，未改业务入口、不重复路由/数据库
业务测试。Neon发行锁与该已发布AS SHA一致，PG gitlink/两份PG版本锁保持5462。
完整版本、哈希和实际门禁见[机器收据](DSGA-持续聚合Quiesce-Mac收据-20261026.json)。

## 尚未完成的业务与Linux边界

完整可信Prepare/Finalize、SystemOutsidePG来源、可安全降低normal的后台/私有
预算及活性、首次准入、Host/最终VM/公开worker与受控跨boot仍需代码。这些不能
包装成Linux待测。Linux cgroup/HugeTLB/PTE/THP/内核暂态、物理回收、KVM/NeonVM/
目标集群E2E及性能长稳另验。整体Goal active/incomplete；不构建镜像、不准备或
执行K8s部署、不合main、不发布。
