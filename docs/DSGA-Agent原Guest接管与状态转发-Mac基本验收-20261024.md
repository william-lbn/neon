# Agent原Guest接管和资源状态转发

正式认证Router新增GET `/dsg/v1/resource/state`，复用固定root UDS，不增加
无认证入口。root返回同锁资源日志及当前约束；Original/Held/floor独立于PG boot，
不能以metadata读回推导旧命令NoEffect，也不能代替完整预算/VM完成。

发行AS锁绑定已先推送并核对的b9e393f8fa846b19d14c0246c1da46cf0d82a70f。
实际Agent先持久冻结原Guest再root fence，PGWorker携带同一资源客户端，新PG
POST及显式重放前核对当前持续约束。已有未知PG结果保留原查询，暂态不换owner。
不是另一个脱离运行链的客户端。PG17内核和三处PG锁仍cbabd5，其他PG暂缓。

Mac新增正式路由1pass、0ignored，验证只允许控制scope、拒绝初始化scope及
非法query；不访问本机特权路径，没有重复旧路由。compute_tools全目标clippy
退出0。证据在工作区build/dsga-resource-owner-rust.log及clippy.log。
AS必要race去重8项、相关vet及8个Linux程序目标编译通过，见AS同批中文验收；
只有分段真实传输/逻辑和编译证据，没有Linux root/K8s/物理端到端验收。
暂存版本检查18程序、4个锁定PG及39扩展阶段通过，实际PG17检出和三处绑定
一致，证据为工作区build/dsga-resource-owner-version.log；16个本批源输入已复核。

GuestReady实际资源领域派发/原观察、完整预算/系统上界、首次准入/全排空、
Host/最终Runner-QMP、公开worker及跨boot/owner关闭仍须编码。默认关闭，不将
代码缺口移入Linux待验；本轮不构建镜像、不准备或执行K8s部署、不合main。
