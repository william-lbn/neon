# DSGA 执行与恢复记录

更新：2026-10-20（Asia/Shanghai）。设计以 V3 为准。Goal 保持 active，无显式预算；
单模块完成不能标 complete。中断后读取真实 Git/远程 SHA/进程句柄及日志，不重复
已有构建，也不重复 apply stash。没有用户授权不创建子代理。

## 已核对检查点

- PG17 功能分支 `codex/dsga-pg17-v3-20261019` 当前
  `864d32d9a8cfb23c71ea73caccc2c80360fb18bb`，已推送并核对 GitHub refs。
  包含 S3 `7a1ff181…`、SLRU v2能力 `76cfb197…`、真实关机restartpoint修复
  `e02a09ad8c1c8d6a91cfc3730fd7af111d0ec943`、TAP fixture适配 `864d32d9…`。
- 独立官方维护 `4a39412f154796461f404785d369a3dc9424fcdc` 已推送，含社区
  PG17 `91246c7`、Neon PG17 `56692df` 的全部祖先。最近fresh fetch三仓官方头未变，
  Neon main仍 `fa504217…`，Autoscaling main仍 `a4bce09e…`。
- Neon功能分支 `codex/dsga-neon-v3-20261019`，上个已推送头为 `ef9e20e44…`。
  本批完整Neon存储适配待提交/推送，gitlink/两份锁已绑定上述864d32d9；下一次
  恢复必须查Git/GitHub，不能依据这条提交前记录猜测是否已推送。
- Autoscaling `codex/dsga-autoscaling-20261019` / `ef6cd154…` 仍无DSGA业务实现；
  control-plane `codex/dsga-control-plane-v3-20261020` / `928eec95…` 与 Helm
  `codex/dsga-neon-helm-v3-20261019` / `648488a` 同样仍待业务接入。

## 本批代码与准确身份验证

完整Neon扩展已适配最新PG接口：命名共享LSN锁、删除无调用者旧hook注册但保留
真正smgr缓存更新；SLRU独立下载hook与原生通信器接线、受控落盘、原子不覆盖发布、
失败/取消/FATAL清理及新boot清理SIGKILL遗留；远端页数先检查再乘法/复制。缓存
清理覆盖真实H，每项短Guard并在可能IO前释放。详细职责/锁/失败见存储接口文档。

- 最新PG头864d32d9准确configure/build：45项真实集群专项通过86.415s。
- 新 `ci/test-dsg-neon-extension.sh`：先检查index锁、源码与postgres完整SHA；
  clean/build/install完整neon（含Rust communicator）、neon_test_utils、rmgr与
  测试库，再实际加载运行22项通过41.137s，零skip。最后一轮句柄71887已退出0；
  日志 build/neon-integrated-storage-abi-extension-tail-final.log。新增目录/身份边界，
  并确保清缓存暂停在真实有效tail提示，再Shrink后恢复；不是只在bootstrap前缀暂停。
- postgres --version 已实际输出完整864d32d9 SHA，PG工作树干净。
- 日志 `build/neon-integrated-storage-abi-{build,lifecycle,extension}.log`。
- 候选阶段20项通过34.725s，含真实文件短写、同Backend资源限额恢复、取消/FATAL/
  SIGKILL、新boot清理、LSN缓存多Backend与LRU、真实Buffer尾部和Shrink交错。
  本地PG/COPY协议fixture只验证客户端/解析/下载/落盘，不能代替真实Pageserver语义。
- 旧PG14/15/16各自真实发行头对8个受影响C文件共24次语法/API检查通过；不等于
  这些版本的DSGA功能或完整链接运行已验收。日志 build/neon-slru-legacy-syntax.log。
- PR CI新增宿主完整扩展门禁，没有远程Linux执行证据，不打包镜像/部署。
- 提交前prepare共80项：13项通过、67项因未提供集成环境skip；这些skip不能
  计为业务验证，45+22两套另有上述真实执行证据。Ruff check/format、bash -n、
  源码锁检查及git diff --check通过。日志 build/neon-storage-abi-prepare-final.log。

## 查实并修复的restartpoint持久化缺陷

主库关机通过PreCheckPointGuts预刷；备库CreateRestartPoint却没有预阶段，原共同
阶段仍仅按SHUTDOWN flag跳过Buffer/复制状态，然后推进redo。不是Mac能力缺失。

- 新061在原版前三断言通过（重启前数据可见、仍为备库、redo已推进），随后真实
  报缺表；反向日志 build/neon-restartpoint-negative.log 与完整节点备份保留。
- e02a09ad8c按显式调用路径区分是否预刷；保留主库Neon额外WAL顺序，不改格式。
- 开/关各4个恢复TAP文件75断言通过，每组29s（004/009/040/061）。
- 45项DSGA专项通过93.534s；基础check-world开/关各81组/666项报告检查通过，
  退出码0（这两组基础检查不含TAP）。日志 build/neon-restartpoint-*。

## TAP当前边界

初始8个失败文件中7个已有针对性通过证据：

- 004/009/040由上述真实持久化修复通过。
- custom_rmgrs/001与recovery/027保留实际配置解析出的Neon rmgr再追加测试库。
- recovery/001精确匹配Neon逐次序列WAL的1值，两个真实备库通过。
- recovery/038精确比较关机保存值与重启解码起点，不以checkpoint偏移允许回退；
  临时省略全部关机slot保存时真实失败2/4断言，逐字节恢复slot.c并重编后4断言通过。
- custom_rmgrs 1文件/2断言、001/027/038三文件75断言通过61s。
  日志 build/neon-tap-{preload-custom-rmgr,fixtures-positive}.log、
  build/neon-slot038-{negative,restored}.log；故障版slot.c未提交且当前已恢复。

pg_upgrade/003仍失败：pending检查对非事务逻辑消息设置processing_required，关机
存在Neon文件消息，需要继续确认并安全处理，不能忽略真实应用变更或跳过检查。
首次整体失败后未到达的其他TAP也必须用make -k完整复跑；不宣称全量只剩1个问题。
若新一轮完整TAP已启动，请先查真实句柄/进程及日志，不重新启动同一build的第二份。

## 接续顺序与未实现范围

1. 完成本批Neon提交/推送与远程核对，准备检查/格式/实际结果见提交日志。
2. 完整TAP继续跑并逐项处理；逻辑槽升级须保留真实应用pending保护。
3. 正式capabilities/request/status/cancel/memory_status、管理认证、独立请求回执与
   有界历史、完整私有/并行/DSM/维护/扩展预算与quiesce，Compute/Monitor/Agent/
   Scheduler/Controller/Runner执行fencing和恢复对账仍须编写，不能包装为待Linux测试。
4. 完整Neon存储链本地可行端到端；PG16/15/14逐版最新维护与语义移植；PG18独立
   发行/AIO/SLRU接口宽度适配。再对齐control-plane Driver与Helm分支系列。

## 不可破坏的恢复/环境约定

- 旧stash 6f002a31196f2e623e8386c38a649ff87be2ada4 已apply一次，禁止再apply。
- S3 stash f5c4dfc54b3600a4191b4a7f06d2437db3b8403b 已在合维护后apply一次，
  补丁逐字节一致，禁止再apply。两份保留，标记在build/v3-s3-upstream-stash-*。
- 实际PG源码repos/neon/vendor/postgres-v17，repos/postgres仅符号链接。
- 不构建镜像，不准备/执行K8s，不合主分支，不发布生产；关键代码与commit详细中文。
- Linux/K8s/NeonVM/KVM/HugeTLB/真实物理回收/迁移/目标性能长稳后验，不能以Mac或
  协议fixture代替。上述未实现业务与目标平台未认证必须分开记录。
