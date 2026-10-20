# 管理 schema 的角色切换权限与长期客户端

日期：2026-10-20。补强共享Rust客户端276230c；不改PG486338、Buffer ABI5或控制协议1。

此前发现管理扩展时检查了非超级角色的schema CREATE直接/继承权限，但PG17的
成员边可独立设置INHERIT与SET。NOINHERIT且`WITH INHERIT FALSE, SET TRUE`的
成员可以切到管理员，仍有替换扩展/schema对象的能力，不能因CREATE观测为false
而认定管理命名空间可信。

真实PG用例先验证`has_schema_privilege(...,'CREATE')=false`及
`pg_has_role(...,'SET')=true`，原校验确定性漏拒绝1/1，其余7项filtered。日志在
工作区`build/neon-dsg-http-noinherit-negative.log`；失败夹具保留且已关闭集群。
修复使用PG17实际角色SET关系核验，任何非超级角色可切到超级角色时拒绝绑定，
原有直接/继承CREATE及扩展成员核验继续保留。不是修改租户权限来让测试变绿。

同时增加`bind_owned`：SDK可拥有两个Client，供长期服务跨请求保存同一执行栅栏，
避免自引用结构或逐请求新建客户端丢失未确认意图。原借用bind接口兼容，driver
仍由创建者驱动；不引入自动重连或新的VM执行权。原生集群夹具移至tests/support，
保持0700、准确SHA和失败保留机制，供后续HTTP门禁复用。

修复后完整SDK严格8项12.27s通过，零ignored/filtered；长期owned路径另已在
Compute HTTP实际链路验证。两包本地Clippy全targets -D warnings退出0（依赖27条
既有warnings保留）。后续HTTP接入独立提交；完整持久owner、预算和VM链仍须实现。
此修复不代表其他PG大版本已移植；旧版本的角色语义在逐版支持时独立适配/验证。
