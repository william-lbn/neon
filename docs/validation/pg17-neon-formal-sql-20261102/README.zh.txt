PG17正式Neon SQL当前产物首次功能验证

准确PG17 adecb5、PG649f，正式neon.so1a06与rmgrfb42，源/镜像/参数由source-artifact-lock逐项绑定。原f7b18b实际唯一Wait0，4.102秒、2CPU1GiB、NNP/无cap/隔离Unix socket；无新编译、无桥接测试扩展。
真实H/N 256→1024→256→2048，generation1→4；四数组释放verified且无unknown，600行约4.9MB数据校验和跨三次保持。正式capabilities/status、同请求重放、冲突与过期拒绝均保原JSON结果。
本批是一个真实PG+Neon组件目标，不是normal硬限、Root/VM/Host/K8s端到端或production-ready通过。这里的N/H是PG缓冲区数量，不是cgroup normal N硬限。原shrink时准入仍open，不能直接将其冒normal降限授权；数组页对齐回执是事实，可供相关消费者规则回归使用。
functional.sql和原源码/产物锁、进程终态、完整JSON保持原字节。此目录不运行未改存储、HA或迁移矩阵，不改变发行锁。
