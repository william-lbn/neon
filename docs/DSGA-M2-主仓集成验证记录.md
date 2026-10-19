# DSGA M2：Neon 主仓接入与 Mac 验证

日期：2026-10-19。本记录覆盖已完成的 Provider/初始布局、访问边界、真实扩容、回收边界和版本绑定；
完整在线扩缩、Granule 回收、Monitor/Agent/VM 业务链路仍在后续开发范围，不能声称功能已完整交付。

## 从主仓构建的证据

Postgres 的真实源码现位于 `vendor/postgres-v17`，由主仓 `postgres.mk` 构建。
候选 SHA 为 `2d60467e0218be4ad48761d9878e163dc74b2976`，对应 17.11。
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
PostgreSQL 17.11 (2d60467e0218be4ad48761d9878e163dc74b2976)
```

这证明构建入口已经接到所选 fork 提交。原 `repos/postgres` 只是本地兼容符号链接，
不是另一套独立源码。

## 新增官方修复与回收契约

连续 fetch 发现社区 PG17 更新至 `94035972af3f10cdaf9ff2b5d232ad5f45463469`，
已通过维护提交 `1509896b1f83a087b97fa5c15288c0283660135d` 和功能合并
`823644b93da60235465b4875259817296a04fccf` 纳入，未重写公开历史。
新增 ReleaseTail 覆盖独立映射的末页 padding，保留活跃起始边界；普通区间的
向内对齐契约保持原样。Mac 明确拒绝物理回收，不把边界验证当成 Linux 回收验收。

## 结果与限制

- M2e 在最新官方维护基线上的开启/关闭配置各 81 组、664 项 `check-world` 报告检查通过，未启用 TAP。
  模块回归及隔离测试覆盖旧提示范围、ERROR 清理，以及真实 pg_buffercache 扫描的
  正常唤醒、查询取消、连接终止三条路径；详见子模块 M2c/M2d/M2e 中文验证记录。
- 用上述主仓构建的二进制和测试库重新运行 `ci/test_dsg_postgres.py`：5 项真实
  集群业务测试通过，含连续两次同 Postmaster crash recovery、boot 更新、
  2 万行数据校验、B-tree amcheck、拒绝非法配置及关闭兼容。加载真实 neon_rmgr。
- 新增真实扩容用例：同一连接扩容后继续查询并回滚 SQL，容量保留且重放不重复；
  跨 Granule 扩容、1.2 万行数据与新增 Buffer 使用、immediate stop 后真实 WAL 恢复
  及索引校验。已在上述新 SHA 二进制实际执行通过。
- CI Python 逻辑验证：12 项通过；通用 prepare 没有 PG 环境时额外 5 项集成测试
  明确 skipped，不将它们记为实际通过。显式环境下上述 5 项已独立执行通过。
- 新增源码锁检查覆盖 HEAD/index、gitlink 不符、revision manifest 过期、短 SHA、
  未初始化/错误检出和未提交源码；防止“代码已改但 Neon 构建未使用”的问题。
- Ruff 检查新 Python 文件、Python 编译检查和 diff 检查通过。

日志：`build/check-world-dsg-tail-latest-on.log`、`build/check-world-dsg-tail-latest-off.log`、
`build/neon-integrated-m2e-build.log`、`build/neon-integrated-m2e-lifecycle.log`。
Linux/KVM/HugeTLB、Pageserver/Safekeeper 完整端到端和性能验收尚未完成。
所有功能保持默认关闭，不合入主分支，不据本记录批准 VM 缩容。
