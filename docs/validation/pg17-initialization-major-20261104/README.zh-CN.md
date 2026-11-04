# PG17 初始化版本校验：生产格式修复与定向回归

本次真实端到端中，Compute 已初始化并返回 HTTP 200/running，原生 capabilities 和 memory 均为 200，但 initialization 为 503。新增固定阶段日志明确返回 Major=2：失败发生在版本识别，不是正常内存降限的安全门。

生产入口 compute_ctl.rs 使用 get_pg_version_string(&cli.pgbin)，后者通过原 PgMajorVersion.v_str() 生成 v17 或 v18。此前本 fork 的私有初始化选择器只接受 17/18，误拒正常生产输入。

本次只增加两个有限格式匹配：v17/17 对应 PG17，v18/18 对应 PG18。实际 SQL 查询返回的大版本仍必须精确等于选择的版本，未知/畸形/其它大版本仍拒绝。存储、角色、已加载认证策略、PG boot、配置代次和响应合同均不变。没有删除 Unknown、normal 或准入安全检查。

## 定向用例

- initialization_requires_exact_supported_pg_major：已支持格式必须精确匹配实际 major，跨 major 必须拒绝；不支持或畸形字符串仍拒绝。
- initialization_accepts_original_pg_major_v_str_producer：直接以仓库原 PgMajorVersion 的 PG17/PG18.v_str() 作为输入；原 PG14/PG16 生产编码不能误入本实现。

只构建 compute_tools/compute_ctl，复用已验证的离线 SDK、PG 头文件、Cargo 缓存。没有执行未修改的上游全量测试。当前构建状态与最终源码/产物身份以本目录 receipt.json 和 Linux 原证据为准，不把编译通过等同端到端通过。

本轮筛选 initialization_ 同时匹配原来标注 ignore 的 HBA 真实集成用例；它没有执行，不计入通过。原编排器要求只有两个匹配项，故终态如因此为 FAILED 必须原样保留。定向验收须另核两条完全限定测试名、各阶段退出、编译产物、真实资源和原进程退出，不能改写原失败记录。

## 实际链路与剩余门槛

本轮物理 VM e910 的唯一配置请求已真实执行一次，返回 200/running，耗时约13.4秒；MAY 与 Root 持久意图保留。资格只读采集也已成功，未发布资格/Source/SQL路由。由于需更换包含本修复的 Compute，不继续为这台即将按正式流程退役的实例发布路由。

修后须用新镜像、新物理身份，保留原逻辑 endpoint、tenant/timeline、凭据与存储数据，重新完成真实 initialization、当前实例资格、Proxy SQL、Grow、安全 normal Shrink、Regrow 和原所有者自动策略。PG17/PG18 原内核 gitlink、共同协议、Runner 和保护资产不在本次修改范围内。PG18 接受生产格式不等于 PG18 完整端到端已经通过。

本记录不含密码、签名 seed、JWT、原配置/Secret 正文或 SQL 认证信息；大日志和镜像留在 Linux /data。

本次定向验收结果：两条目标测试通过，编译、strip、源码核对和资源断言退出均为0，Compute ELF SHA256为652a13bde22ab34c48ee0b409f8a21924d1510377f1b9d92790963b13d51bb23（71153008字节）。实际SDK2CPU/3.75Gi/swap0，OOM为0。原统计FAILED及unit exit1原样保留，预算已按原终态reconcile0释放。修后真实Guest/SQL/内存端到端尚待下一镜像验证。
