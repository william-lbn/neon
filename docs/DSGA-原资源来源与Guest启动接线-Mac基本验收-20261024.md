# 原资源来源与Guest启动接线

本批依赖AS `818ca7713f22a026868973a43897a7d1bdf941c0`，先推并核对后绑定发行锁；
控制面cedabd5绑定同一共享来源模块并生产配置。PG17 gitlink和两份PG锁仍准确
`cbabd5de541a1340ab4675cfc3ef14b3a9372671`，本批未改内核/C扩展，不重复未变
PG/SDK/Web或真实PG功能用例。默认关闭，只有PG17。

实际产品代码路径：原平台v2显式额度/原VM UID/公钥→确定性config.json和resource.json
→同一原owned immutable Secret/UID→Controller直接重读/核验→Runner有界解码
→只读runtime ISO→Guest固定路径/原字节封存/root cgroup和实际启动限额→每次
vmstart和PG出生重读原事实。资源来源不含私钥/口令，旧缺额度计划不按当前env
补写。首次来源只接受pristine，运行后重启/迁移还须完成保留未知效果的受控恢复。

AS产品构建/启动源码接入静态root帮助程序和受限SUID出生，根进程/线程迁移
不再全员可写；残留/Kernel模式不会降级。旧未认证Guest写只保留固定管理TLS
目录的cert/key/CA三键有界供给，禁止任意路径和CPU写。没有把此TLS路径说成
新认证资源服务，也没有以普通HTTP成功作预算或VM许可。

Neon bookworm/bullseye补充sysinit在受管模式只check-guest，不能再由旧parser
把memory控制权交给postgres。Linux受管Compute停止旧未认证WS Monitor/LFC
调整，避免与原预算/fence通道并行；legacy保持原行为。Mac未执行该Linux分支。

实际验证：

- AS冻结23个代码/构建/模板输入，Mac定向7顶层/28含子项pass事件，0fail/skip；
  相关vet通过，guard/daemon/Runner/Controller双架构8次Linux编译通过；Go模板
  渲染及vminit/vmstart语法通过。源码摘要前后相同。
- CP冻结9个Go/模块输入，新增4顶层与受影响模板2顶层，共24含子项pass事件，
  0fail/skip，race/vet通过；生产Renderer与实际Runner解码器的字节合约通过。
- Neon两YAML实际解析后提取shell，`sh -n`均通过；受影响compute_tools Mac
  clippy编译exit0，无本包新增告警，原remote_storage的41条依赖警告保留。
  Linux cfg分支/BusyBox/内核并未在Mac执行，不能用语法/编译代替运行证据。
- 日志分别在工作区`build/dsga-provision-as-final/`、`build/dsga-provision-cp-final/`、
  `build/dsga-provision-neon-scripts/`及`build/dsga-provision-neon-clippy.log`。
  Kube对象/exec/内核边界明确是夹具，未执行K8s或Guest；初始缺依赖/缓存权限
  和格式化路径诊断保留，最终冻结检查单独记录，不累计重复轮次。

尚未完成的**代码**：独立认证root资源通道、完整PrepareBudget/GuestReady/
FinalizeBudget/首次准入、完整排空、Host/最终Runner执行、公开worker/生命周期
以及跨PG/Guest/owner受控关闭恢复。这些继续编码，不能改成仅Linux待测。

后续Linux验证原UID/immutable/RBAC/Pod到Guest完整链、实际root安装/SUID/nosuid/
全线程降权/PID/迁移、只读ISO/掉电持久化、TLS正向供给/轮转、真实限额/完整
HugeTLB池/物理回收和端到端性能。本轮没有构建镜像、安装特权文件、准备/执行
K8s、合main或发布；整体Goal仍active/incomplete。
