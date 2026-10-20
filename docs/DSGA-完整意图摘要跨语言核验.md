# 完整意图摘要的跨语言核验

2026-10-20。复核Go HTTP消费者时发现Rust validate_resize虽检查摘要格式，却只
关联kind、Buffer目标及generation，不能独立发现同Key下VM目标、预算ID、期限
不同的回执。现在重算完整Request的PG context_sha256并比较，Go同步实现。
摘要证明完整意图关联，不认证issuer租约、预算或VM执行权。

PG管理SQL的摘要为JsonbToCString后的SHA256。本实现只适用已Validate的固定
Request schema：键按字节长度/字节序排序，逗号/冒号含空格，整数无指数，所有
ID为ASCII graphic。没有扩大到任意Unicode/浮点JSON；不混用journal的领域分离
摘要。sha2复用工作区已锁定版本，Cargo.lock只增加compute_api的直接依赖边，
没有升级第三方版本或PG/发行锁。

context_v1两向量由准确PG486338的真实jsonb_out/sha256生成，含全部高位与引号/
反斜线/HTML字节；Go与Rust文件逐字节共享。Rust原生SDK门禁另在隔离集群查询
PG独立计算值进行比较，并继续验证实际管理SQL回执。response_v1是人工schema
夹具，不能包装成物理释放证明。普通状态查询无完整原请求时仍只做Key/快照校验，
协调器须用保存的完整原意图再ValidateResize；未知结果不能自动清除外层承诺。

真实反向：只移除新摘要比较，full_context_hash_rejects_changed_budget_vm_target_or_deadline
确定性失败1/1、9filtered，说明旧检查漏掉差异；finally逐字节恢复，前后源码SHA256
同为0a3a53872fff76ff120be6162fddef9b7ce97aba7f614acd77e763b89bf61335。
最初默认CLT27 SDK链接失败不是业务反向证据，已单列sdk-error日志；使用已有
Xcode14.4 SDK/clang及工作区build/cargo缓存完成最终验证，没有修改全局环境。

- 严格原生脚本`bash ci/test-dsg-rust-client.sh POSTGRES_INSTALL`：SDK8项14.30s、
  HTTP11项7.46s，0failed/ignored/filtered，使用准确486338 PG与完整Neon扩展。
- 两包普通全量：compute_api24+10=34，compute_tools9+3+5+7+6=30通过；原生
  SDK8/HTTP4默认ignored，已经上项实际执行，零项doc-test不算业务用例。
- 两包`--all-targets --features compute_api/postgres-client --no-deps` Clippy
  `-D warnings`退出0；remote_storage既有27warnings保留，未改无关源码。
- Neon fresh fetch main仍fa504217且是功能头祖先；PG源码/三处版本锁不变。

日志工作区build/neon-dsg-context-{negative.log,strict-final.log,packages-final.log,
clippy-final.log,check-final.log}。Go正式TLS消费者、null标量拒绝、32KiB完整回执
持久化及Scheduler随机测试修复见Autoscaling分支的中文模块文档。Go当前TLS端是
协议夹具，Go→真实Compute→PG同一门禁及持久协调循环仍须集成；不替代Linux/VM
认证。整体仍未完成，Goal保持active，默认关闭、不构建镜像、不部署、不合主分支。
