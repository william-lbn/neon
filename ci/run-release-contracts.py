#!/usr/bin/env python3
"""只加载不依赖运行中 PG 的发行契约；原生回归由同 run 的 PG 工作流硬门禁执行。"""
import unittest

MODULES = ["test_compute", "test_source_lock", "test_postgres_config_cache", "test_release"]
suite = unittest.defaultTestLoader.loadTestsFromNames(MODULES)
if suite.countTestCases() == 0:
    raise SystemExit("No distribution contract tests found")
result = unittest.TextTestRunner(verbosity=2).run(suite)
if not result.wasSuccessful() or result.skipped:
    raise SystemExit("Distribution contracts failed or skipped required tests")
