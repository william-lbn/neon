# 正式PG17 early-birth持续入口

正式ci/run_pg17_memory_cases.py --suite early-birth实际执行严格6根，PASS9.563秒/0skip/无UNMAPPED。包含SECURITY DEFINER有效管理员但普通session被拒。只此suite允许unit-native+single-service，原core-smoke等仍只允许single-service；父NNP1/缺根/旧suite混unit三种实际误配均exit2且无qualified结果。正式test-dsg-postgres.sh也准确枚举6根，薄adapter加载准确PG子模块Python源，无副本、无dummy绿色。

使用已有v2准确编译C产物，运行时PG测试与Neon CI发生本批修改；PG C没有变化，源码hash逐一记录。不是纯58d HEAD镜像重编，未跑整个未改上游或全部旧矩阵。新增稳定六根见catalog/result，原生默认关闭、未绑定Root；normal/VM/K8s仍未通过。
