PG17 显式默认加载器构建与真实闭包组件验收

默认 DSGA_RUNTIME_PROFILE=legacy，旧构建仍保留原RPATH/ldconfig。显式 default-loader-v1 仅PG17/amd64，实际configure --disable-rpath；最终镜像核心lib逐同字节复制到root-owned独立neon-pg17-core，默认路径alias指向该核心副本，不依赖可替换local/lib。拒绝preload/hwcaps，移除本镜像cache；运行时仍必须核完整只读祖先、原文件和实际加载器，脚本本身不发资格。

原精确构建来源：Neon1fed7eb + PG17b95 + 两个当时未提交覆盖文件，9899项快照及原Git archive摘要保留。实际PG输入7364项由Docker层sha256sum检查，最终C为eb3ac9、最终PG ELF9f3；readelf无RPATH/RUNPATH。PG stage418.80秒通过；后续复用该准确stage、顺序构建工具/Neon扩展/运行时957.66秒通过，外网显式127.0.0.1:7890已验证。实际worker逐阶段2CPU/2GiB或8GiB且无额外swap，命令/worker/探针Dockerfile/日志完整保留。缓存未清。最终镜像47ef74/ID789ea199已docker pull并现场image inspect。

实际运行时完全只读、禁网，用已提交AS cf369严格源解析/闭包函数（未改判据）的隔离DEV探针执行。16个ELF逐hash、RO完整父链及严格元数据通过；实际uid/gid1000 glibc --list 的全部规范文件集与候选闭包相同，改hash拒绝。探针14.01秒/build0/runtime0；它是加载闭包集成组件S1，候选清单不是原ISO/Birth权威。原AS函数/DEV探针/实际字节与收据均归档，未改AS业务代码。

P2独立审查971c6252无本批组件阻断。该镜像EXTENSIONS=none，39社区扩展省略；不是全部发行、完整Source13、normal降限或单机/K8s端到端。动态扩展共享mount及业务TLS数据输入仍须后继闭合。后继父锁提交不重标原构建来源；最终联合版本需要最终锁重建/核验。

worker-118查看：docker images --no-trunc 127.0.0.1:5001/neon/compute-v17
准确tag：no-rpath-pg-b95b142190cd-20261031t193328z-45711b
准确digest：sha256:47ef74c7bee6018c1e8578486d1196bd0c9b6d84040f6ccdba97823dcd019e55
