# M0/M1：官方维护基线和 Neon WAL 兼容的本地验证

日期：2026-10-18。此记录只覆盖本批次，不表示动态 Buffer Pool 或 VM 功能完成。

## 修改范围

PG17 合入官方稳定分支 `65e944d92b082cc16e2265a91bab8b73934a0e74`，保留 Neon
专有 WAL、预取和恢复行为。Neon 主仓仅修改 WAL 解码、neon_rmgr 与对应测试/构建入口。

1. UPDATE 的旧 heap 页只从 MAIN fork 识别，避免把新登记的 VM 引用当成 heap。
2. 显式 VM 引用由 block/FPI 路径处理；仅为未登记的 VM 页生成旧格式元数据。
3. 新 VM 引用经恢复 API 判定；同一 VM 页的两组位在同次 LSN 判定内处理。
4. 保留旧 WAL 重放，支持先升级读者再升级写者。
5. 全量检查暴露并修复 fast-forward 过早返回：仍维护 SnapBuild/catalog_xmin，
   仅跳过实际行输出，避免复制槽推进后 VACUUM 回收所需目录元组。

## 环境和真实结果

Mac arm64，clang 15 与 MacOSX14.4 SDK 配套。PG17 断言/debug/injection 构建，
关闭 ICU/readline/zlib。本地生成物和工具缓存保存在工作区 build/target，未纳入源码提交。

| 检查 | 结果 |
|---|---|
| PG17 核心 regress | 225/225 |
| PG17 isolation | 123/123 |
| PG17 `check-world`，加载真实 neon_rmgr | 79 组，660 项报告检查，全部通过 |
| PG17 test_decoding | 19 regress + 15 isolation，包含曾失败的 oldest_xmin |
| Rust wal_decoder 全部库单测 | 13/13，其中 7 项新增 VM 引用兼容测试 |
| Rust wal_decoder doctest | 通过 |
| 新 PG17.11 WAL → crash/recovery | 3/3 |
| 旧 PG17.8 WAL → 新 PG17.11 recovery | 3/3 |
| PG16 neon_rmgr 编译与恢复 | 编译通过；实际 recovery 3/3 |
| PG16 test_decoding | 19 regress + 13 isolation |
| Neon 原有 CI Python 测试 | 5/5 |
| Ruff 0.11.2、rustfmt 1.88、pgindent、bash -n、diff --check | 通过 |

`check-world` 计数按日志的测试组报告汇总，不等同于所有内部断言数量。该配置未启用
TAP，因此不把 TAP 测试写成通过。WAL benchmark 依赖 Neon 自有 AWS fixture，未运行。

真实恢复测试每项独立建库，启用 checksum 与 `wal_consistency_checking=neon`，
执行 immediate 停机再恢复；跨 VM 页用例主动检查旧/新 heap 页确实属于不同 VM 页。
这些测试使用原生 PG + 真实 neon_rmgr，不使用 mock。它们仍不能替代 Pageserver/RO
端到端或 Linux VM/HugeTLB 验收。

## 重现入口

先完成 PG 构建并安装到开发前缀，再运行：

```sh
bash ci/test-postgres.sh /absolute/pg-build /absolute/pg-install

NEON_TEST_PG_BIN=/absolute/pg-install/bin \
NEON_TEST_RMGR_LIBRARY=/absolute/neon/pgxn/neon_rmgr/neon_rmgr.dylib \
python3 pgxn/neon_rmgr/tests/test_standalone_recovery.py
```

旧 WAL 升级场景额外指定 `NEON_TEST_LEGACY_PG_BIN` 和
`NEON_TEST_LEGACY_RMGR_LIBRARY`；新二进制和新库仍使用上述两个主要变量。
Linux 扩展名和构建环境应使用对应 `.so`，不能沿用 Mac `.dylib`。

Rust 解码器需要四个受支持 PG 版本的真实安装头文件，通过 `POSTGRES_INSTALL_DIR`
指向含 `v14`–`v17` 的目录；不能将四个版本都指向同一套头文件。

## 发布限制

本批次不启用动态共享缓冲池，不更新生产配置，也未验证真实虚拟化环境。后续
发行必须成套固定 PG gitlink、发行锁、Neon/Autoscaling 提交和镜像 digest；按“先
Pageserver/redo 读取兼容、再 Compute 写入”的顺序发布。
