# PG17栅栏准入内核与正式SQL：Mac基本验收

2026-10-23。PG17依赖`13e109b720d7192e1ca29db0ac878884adaaf831`已先推送并
核验；Neon gitlink与ci/distribution.json、vendor/revisions.json同步该SHA。
当前只PG17，原统一功能分支；本模块不等于完整预算/Agent/VM链完成。

## 实现与不变量

新增独立NEON_DSG_ADMISSION_API_VERSION=1，Buffer ABI6现有结构/函数签名及
字段偏移不变。定长准入回执复用目录原已计入ShmemSize的保留cache line，编译
断言保证能容纳；没有额外未预算共享区。每个新共享内存boot清空摘要和许可。

正式请求先显式建立当前issuer floor。内核原子核对同boot、epoch严格等于floor、
当前容量代次及连续准入revision；完整规范意图SHA一致才重放，同revision异源
或改变open冲突，旧revision/owner不覆盖新状态。溢出拒绝，不重置计数。旧测试
入口没有完整摘要，因此调用它会使新协议的来源回执失效，不能冒称正式重放。

开门条件取得resize串行锁，仍有PREPARING未知效果则拒绝；关门不等待resize/
Provider。锁序resize（仅开门）→status，禁止已持access/resize锁的调用方；
spinlock内只有有界比较/复制，无分配、网络、IO、ERROR。返回仅为PG门禁事实，
不意味着已有事务/后台已排空、预算已认证、物理回收或VM执行已完成。

Neon扩展1.9新增neon_dsg_set_admission(text)，安装清单包含升/降级SQL。严格
7字段请求绑定完整VM/Compute/boot、epoch、准入revision、容量代次、open与预算
引用；拒绝重复键/未知或缺字段/错误类型/非规范uint64。规范jsonb全文SHA进入
内核回执，响应保留完整request/hash/disposition与当前snapshot，VM许可恒false。
预算引用是外层承诺，字符串本身不证明资源约束。PUBLIC ACL默认撤销，C再次
检查当前及session内部超级用户，错误GRANT/SECURITY DEFINER不能给租户开门。
旧内核缺增量能力明确拒绝，不用PG大版本猜支持；旧1.8接口不改。

## 实际验证与失败记录

- 真实Mac PG17临时实例新增内核3项全通过，0fail/skip，7.703秒：同连接拒绝/
  恢复SQL、完整上下文重放与冲突、旧owner/迟到开门、真实resize交错、同一
  Postmaster内部恢复后旧许可失效/新boot默认关闭。测试桥只转换参数/事实。
- 正式Neon1.9 SQL首轮2项失败，发现Makefile DATA漏列新升级文件；补清单后
  仅复验该新增范围，2项通过、0fail/skip，3.801秒。覆盖真实扩展升级、规范
  全文摘要、预算引用冲突、直接/SECURITY DEFINER权限隔离、严格JSON、降级
  再升级不打开门禁。旧失败日志保留，未用C测试桥代替正式SQL入口。
- 初次准确SHA重配置缺PERL5LIB而失败；使用已有build/perl-local/lib/perl5后
  保留TAP选项，configure/make/install/C模块均退出0。准确PG与配套Neon/测试
  扩展重编完成。只补产物版本/扩展/新符号冒烟，未重复上述相同功能套件。
- index版本绑定门禁通过18 binaries/4 pinned PG versions/39 extension stages；
  受影响源码/构建门禁12项通过，0fail/skip；Python与shell语法及diff检查通过。

基本测试先使用原fcada基线加已保存工作树补丁构建，未冒称当时二进制带新SHA。
提交后产物明确为PostgreSQL17.11(13e109b720d7192e1ca29db0ac878884adaaf831)，
配套扩展1.9、Buffer ABI6；源码摘要一致，冒烟确认无floor时仍关闭且无VM许可。
各范围不相加为单次全套认证。ci/test-dsg-postgres.sh和正式扩展脚本均接入新增
文件并检查最低测试数，不能由旧套件绿色掩盖新文件未打包。

## 尚未实现与Linux边界

Rust/Go准入契约、SDK与正式HTTP/控制面网关、真实Agent owner和预算/启动准入/
公开worker接线仍需继续；完整后台排空、Monitor/策略/Scheduler/final Controller/
Runner及生命周期等也尚未完整交付。不能把这些代码缺口记为Linux待验；本模块
不会仅凭初始化读JWT发放开门权，默认关闭、Goal active。

真实Linux K8s RBAC/证书/Service、NeonVM/KVM热插拔、HugeTLB、物理回收、
故障与性能长稳后验。Mac门禁与内部恢复通过不能保证这些行为正确。本轮不构建
镜像、不准备或执行部署、不合自有main。完整机器摘要/源码与实际日志路径见
acceptance/dsga-pg17-admission-core-sql-mac-20261023.json。
