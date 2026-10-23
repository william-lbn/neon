"""原生PG固定主段/DSG数组去重成本；只跑新基本行为，不重跑旧伸缩套件。"""
import os
import unittest

import test_dsg_neon_control as control


@unittest.skipUnless(all(os.environ.get(name) for name in control._ENV), "需显式准确PG/Neon配套库")
class DsgSharedCost(unittest.TestCase):
    def setUp(self):
        self.fixture = control.DsgNeonControl("runTest")
        self.addCleanup(self.fixture.doCleanups)
        self.fixture.setUp()
        self.pg = self.fixture.pg
        self.pg.sql("ALTER EXTENSION neon UPDATE TO '1.11'; CREATE ROLE dsg_cost_app LOGIN")

    def cost(self):
        # 此观察不应等待resize或Provider。若错误取锁，明确失败，避免无限运行。
        return self.fixture.value("SET statement_timeout='2s'; SELECT neon_dsg_shared_cost()")

    def check_cost(self, fact):
        self.assertEqual(fact["protocol_version"], 1)
        self.assertFalse(fact["vm_shrink_permit"])
        main, initial = int(fact["startup_main_upper_bytes"]), int(fact["startup_arrays_upper_bytes"])
        self.assertGreater(main, 0)
        self.assertGreater(initial, 0)
        snapshot = fact["observed"]
        self.assertEqual(main + initial, int(snapshot["startup_shared_upper_bytes"]))
        charged = sum(int(array["committed_upper_bytes"]) for array in snapshot["arrays"])
        self.assertEqual(int(fact["current_main_and_arrays_upper_bytes"]), main + charged)
        self.assertNotEqual(int(fact["current_main_and_arrays_upper_bytes"]), main + initial + charged)
        return main, initial

    def test_shared_cost_deduplicates_boot_arrays_and_keeps_runtime_charges(self):
        before = self.fixture.state()
        first = self.cost()
        baseline = self.check_cost(first)
        self.assertEqual(first["observed"], before, "成本查询不能改gate/floor/容量/Provider")
        self.assertEqual(int(first["current_main_and_arrays_upper_bytes"]), int(before["startup_shared_upper_bytes"]))
        self.fixture.call(self.fixture.request())
        grown = self.cost()
        self.assertEqual(self.check_cost(grown), baseline)
        self.assertGreater(int(grown["current_main_and_arrays_upper_bytes"]), int(first["current_main_and_arrays_upper_bytes"]))
        self.fixture.call(self.fixture.request(revision=2, generation=2, target=256, kind="shrink"))
        shrunk = self.cost()
        self.assertEqual(self.check_cost(shrunk), baseline)
        self.assertEqual(shrunk["observed"]["high_buffers"], 256)
        # Mac不支持物理释放时，逻辑H降低也不能把仍登记的占用直接扣成bootstrap。
        self.assertEqual(shrunk["observed"], self.fixture.state())
        self.assertEqual(self.pg.sql("SELECT has_function_privilege('dsg_cost_app','neon_dsg_shared_cost()','EXECUTE')").stdout.strip(), "f")
        self.pg.sql("GRANT EXECUTE ON FUNCTION neon_dsg_shared_cost() TO dsg_cost_app")
        denied = self.pg.sql("SET SESSION AUTHORIZATION dsg_cost_app; SELECT neon_dsg_shared_cost()", check=False)
        self.assertNotEqual(denied.returncode, 0)
        self.assertIn("superuser session", denied.stderr)
        self.pg.sql("ALTER EXTENSION neon UPDATE TO '1.10'")
        self.assertEqual(self.pg.sql("SELECT to_regprocedure('neon_dsg_shared_cost()') IS NULL").stdout.strip(), "t")

    def test_shared_cost_observes_actual_prepare_without_waiting_for_resize(self):
        before = self.cost()
        process = self.fixture.paused(self.fixture.request())
        fact = self.cost()
        self.check_cost(fact)
        self.assertEqual(fact["observed"]["operation"]["phase"], "preparing")
        self.assertEqual(fact["startup_main_upper_bytes"], before["startup_main_upper_bytes"])
        self.assertEqual(fact["startup_arrays_upper_bytes"], before["startup_arrays_upper_bytes"])
        self.assertGreaterEqual(int(fact["current_main_and_arrays_upper_bytes"]), int(before["current_main_and_arrays_upper_bytes"]))
        self.fixture.finish(process)
