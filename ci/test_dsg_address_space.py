"""原生PG最早启动阶段的AS约束解析与Mac平台拒绝。

Mac不具备Linux /proc/RLIMIT_AS生产核验，本文件不模拟该能力；非零来源在
Mac必须启动失败。未配置来源则启动真实独立PG，验证SQL及重启，绝不连接已有库。
"""

import os
import subprocess
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

import test_dsg_postgres as lifecycle


_ENV_KEY = "NEON_DSGA_ADDRESS_SPACE_MAX_BYTES"


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要显式真实PG及配套测试库")
class DsgAddressSpaceStartup(unittest.TestCase):
    def invoke(self, value, *arguments):
        environment = os.environ.copy()
        environment[_ENV_KEY] = value
        return subprocess.run(
            [str(Path(environment["NEON_TEST_PG_BIN"]) / "postgres"), *arguments],
            env=environment, text=True, capture_output=True, timeout=15,
        )

    def test_noncanonical_or_unbounded_values_fail_before_any_mode(self):
        # 覆盖查询模式与真正-D入口；两者均在目录访问/PG内存上下文之前拒绝。
        for value in ("", "0", "00", "065536", "+65536", "-65536", "65536 ",
                      " 65536", "65536\n", "1", "65535", "65537", "unlimited",
                      "9223372036854775808", "18446744073709551616", "9" * 128):
            for args in (("--version",), ("-D", "/nonexistent-dsga-address-space-test")):
                with self.subTest(value=value, args=args):
                    result = self.invoke(value, *args)
                    self.assertEqual(result.returncode, 1)
                    self.assertEqual(result.stdout, "")
                    self.assertEqual(result.stderr, "invalid DSGA original address space limit\n")

    @unittest.skipUnless(sys.platform == "darwin", "Mac明确拒绝Linux生产能力")
    def test_mac_valid_finite_limits_are_explicitly_unsupported(self):
        # 最大合法对齐值也必须通过解析，再到平台拒绝；不能在整数边界回绕。
        for value in ("65536", "134217728", "9223372036854710272"):
            for args in (("--version",), ("--sync-safekeepers",),
                         ("-D", "/nonexistent-dsga-address-space-test")):
                with self.subTest(value=value, args=args):
                    result = self.invoke(value, *args)
                    self.assertEqual(result.returncode, 1)
                    self.assertEqual(result.stdout, "")
                    self.assertEqual(result.stderr, "DSGA finite address space requires the supported Linux resource guard\n")

    def test_absent_source_still_runs_real_pg_and_survives_restart(self):
        # 复用真实生命周期清理，不从调用者环境意外启用新约束。
        environment = os.environ.copy()
        environment.pop(_ENV_KEY, None)
        with patch.dict(os.environ, environment, clear=True):
            pg = lifecycle.DsgPostgresLifecycle("runTest")
            try:
                pg.setUp()
                pg.start()
                expected = os.environ.get("NEON_TEST_PG_SOURCE_SHA")
                if expected:
                    self.assertIn(expected, pg.sql("SELECT version()").stdout)
                pg.sql("CREATE TABLE as_legacy_probe(i integer); INSERT INTO as_legacy_probe VALUES(17)")
                pg.command("pg_ctl", "-D", pg.data, "-m", "fast", "-w", "stop")
                pg.running = False
                pg.start()
                self.assertEqual(pg.sql("SELECT i FROM as_legacy_probe").stdout.strip(), "17")
            finally:
                pg.doCleanups()
