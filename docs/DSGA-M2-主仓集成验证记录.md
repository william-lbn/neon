# DSGA M2：Neon 主仓接入与 Mac 验证

日期：2026-10-19。本记录覆盖已完成的 Provider/初始布局和版本绑定；在线扩缩、
Granule 协调、Monitor/Agent/VM 业务链路仍在后续开发范围，不能声称功能已完整交付。

## 从主仓构建的证据

Postgres 的真实源码现位于 `vendor/postgres-v17`，由主仓 `postgres.mk` 构建。
候选 SHA 为 `b1188732d73d7d5069a18abe014dfaf2bb0753dc`，对应 17.11。
`ci/distribution.json`、`vendor/revisions.json` 和主仓 gitlink 已同步。

Mac 上清理旧构建并启用依赖跟踪后，从 Neon 根目录执行：

```sh
make -j4 postgres-v17 POSTGRES_VERSIONS=v17 \
  BUILD_DIR=<workspace>/build/neon-integrated \
  POSTGRES_INSTALL_DIR=<workspace>/build/neon-integrated-install \
  DISABLE_HOMEBREW=1 \
  PG_CONFIGURE_OPTS='--without-icu --without-readline --without-zlib --enable-debug --enable-cassert --enable-depend --enable-injection-points'
```

编译器和 SDK 使用与 M0/M1 相同的 Xcode 15 + MacOSX14.4.sdk 组合。
实际 `postgres --version` 输出包含：

```text
PostgreSQL 17.11 (b1188732d73d7d5069a18abe014dfaf2bb0753dc)
```

这证明构建入口已经接到所选 fork 提交。原 `repos/postgres` 只是本地兼容符号链接，
不是另一套独立源码。

## 结果与限制

- 已验证的 PG 源码迁入新分支后逐文件 diff 一致；开启/关闭各 81 组、662 项
  `check-world` 结果仍对应相同源码，详见内核 M2b 记录。
- 用上述主仓构建的二进制和测试库重新运行 `ci/test_dsg_postgres.py`：4 项真实
  集群业务测试通过，含连续两次同 Postmaster crash recovery、boot 更新、
  2 万行数据校验、B-tree amcheck、拒绝非法配置及关闭兼容。加载真实 neon_rmgr。
- CI Python 逻辑验证：12 项通过；通用 prepare 没有 PG 环境时额外 4 项集成测试
  明确 skipped，不将它们记为实际通过。显式环境下上述 4 项已经独立执行通过。
- 新增源码锁检查覆盖 HEAD/index、gitlink 不符、revision manifest 过期、短 SHA、
  未初始化/错误检出和未提交源码；防止“代码已改但 Neon 构建未使用”的问题。
- Ruff 检查新 Python 文件、Python 编译检查和 diff 检查通过。

日志：`build/neon-integrated-build.log`、`build/neon-integrated-lifecycle.log`。
Linux/KVM/HugeTLB、Pageserver/Safekeeper 完整端到端和性能验收尚未完成。
所有功能保持默认关闭，不合入主分支，不据本记录批准 VM 缩容。
