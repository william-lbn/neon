# PG18 Pageserver 控制文件导入验证

真实 PG18 timeline 初始化曾报 CRC 错误。原始 pg_control 可由严格 PG18 读取器通过校验；直接原因是 Pageserver 导入入口仍使用指向旧版本布局的公共 ControlFileData。

本次按 timeline 的实际主版本选择解析器：PG18 使用 v18 布局及对应 checkpoint 编码；PG14 至 PG17 保持原公共布局路径。保留 CRC 和 PG_CONTROL_VERSION 校验，不修补文件字节或跳过校验。

验证范围：

- f39276：两项 control_import_tests 通过，覆盖 PG18 布局、checkpoint、CRC 损坏、错误控制版本及旧版本布局行为；实际 Pageserver 增量编译通过。
- 87f9ff：在原失败运行的停止态独立副本上复用原初始化归档；真实 Controller 创建和独立 Pageserver HTTP 读回均为 PG18、Active，LSN 一致。五个服务随后受控停止，全部退出码为零，无 OOM。
- 9bc766 原始 CRC 失败、原数据和初始化归档保留于 /data/neon-build/validation，不覆盖历史结果。

这仅证明真实 PG18 存储 timeline 初始化；不代表 PG18 Root、安全 normal 降限、内存扩缩容端到端或 Kubernetes 验收。运行采用本地文件存储，不是 MinIO/S3 回归。私密配置、完整日志及二进制保留在 /data，不加入 Git。
