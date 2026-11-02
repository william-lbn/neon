# PG18 新 timeline 沿用已支持的 Neon 存储格式

PG18 initdb 默认启用数据页 checksum；PG17 默认未启用。官方参数说明见 https://www.postgresql.org/docs/18/app-initdb.html 。本轮实际旧 PG18 控制文件为 checksum1，旧 PG17 为 checksum0；PG18 真实 Compute 在 SQL 数据链收到 checksum0 的 WAL 重建页后触发校验错误。

现 Neon 的 WAL redo 内存页输出不经过 PostgreSQL 普通 FlushBuffer 写盘时的 checksum 副本处理。为保持原存储合同，本次仅在新 PG18 timeline 的生产 initdb 命令显式添加 --no-data-checksums；PG14 至 PG17 参数保持不变。

不修改已有控制文件、数据、basebackup、WAL CRC 或页面校验器，不在读后重算 checksum 掩盖错误。原 checksum1 失败归档保留；本改动没有新增 checksum1 远端存储支持。

e1e587：两项精确参数测试通过，零失败、零忽略，实际 Pageserver 增量构建零退出。5c35c8：真实 PG18 initdb 和 pg_controldata 验证新目录 control1800、catalog202506291、checksum0、正常关闭。随后新 timeline 的 Compute/SQL/WAL/冷读取验证独立进行，不能用参数测试替代数据链或内存伸缩验收。

完整日志、二进制和私有配置保留于 /data/neon-build/validation。
