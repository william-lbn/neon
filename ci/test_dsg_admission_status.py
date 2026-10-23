"""真实Neon只读准入对账；不通过重放setter模拟查询，不重复旧门禁套件。"""
import json
import os
import unittest

import test_dsg_neon_admission as admission
import test_dsg_neon_control as control


@unittest.skipUnless(all(os.environ.get(name) for name in admission._ENV), "需显式指定真实PG/Neon配套库")
class DsgAdmissionStatus(unittest.TestCase):
    def setUp(self):
        self.fixture = admission.DsgNeonAdmission("runTest")
        self.addCleanup(self.fixture.doCleanups)
        self.fixture.setUp()
        self.pg = self.fixture.pg
        self.pg.sql("ALTER EXTENSION neon UPDATE TO '1.10'")

    def query(self, request):
        body = self.pg.quote(json.dumps(request))
        # 若错误地等待resize/Provider，此期限必须使测试失败，不能挂成“在运行”。
        return self.fixture.value(f"SET statement_timeout='2s'; SELECT neon_dsg_admission_status('{body}')")

    def memory(self):
        return self.fixture.value("SELECT neon_dsg_memory_status()")["observed"]

    def fence(self, epoch):
        request = {"protocol_version": 1, "instance": self.fixture.capability["instance"], "issuer_epoch": str(epoch)}
        return self.fixture.value(f"SELECT neon_dsg_fence_issuer('{self.pg.quote(json.dumps(request))}')")

    def test_new_owner_queries_original_receipt_without_executing_unseen_request(self):
        request = self.fixture.request()
        before = self.memory()
        missing = self.query(request)
        self.assertEqual(missing["disposition"], "not_found")
        self.assertEqual(self.memory(), before, "查询不能建立floor或应用未见准入")
        self.fence(2)
        self.assertEqual(self.fixture.call(request)["disposition"], "applied")
        self.fence(3)
        self.assertEqual(self.fixture.call(request)["disposition"], "stale")
        before = self.memory()
        fact = self.query(request)
        self.assertEqual(fact["request"], request)
        self.assertEqual(fact["disposition"], "replayed")
        self.assertEqual(fact["observed"]["issuer_floor_epoch"], "3")
        self.assertFalse(fact["vm_shrink_permit"])
        self.assertEqual(self.query(self.fixture.request(budget_commitment_id="foreign"))["disposition"], "conflict")
        self.assertEqual(self.memory(), before, "旧owner观察/冲突不能改门禁或floor")
        close = self.fixture.request(issuer_epoch="3", revision="2", open=False)
        self.assertEqual(self.fixture.call(close)["disposition"], "applied")
        before = self.memory()
        self.assertEqual(self.query(request)["disposition"], "result_expired")
        self.assertEqual(self.query(self.fixture.request(issuer_epoch="99", revision="3"))["disposition"], "not_found")
        wrong = self.fixture.request(instance={**request["instance"], "vm_uid": "foreign-vm"})
        self.assertEqual(self.query(wrong)["disposition"], "stale")
        self.assertEqual(self.memory(), before, "缺失/淘汰/异源都不能写共享状态")
        self.assertEqual(self.pg.sql("SELECT has_function_privilege('dsg_app','neon_dsg_admission_status(text)','EXECUTE')").stdout.strip(), "f")
        self.pg.sql("ALTER EXTENSION neon UPDATE TO '1.9'")
        self.assertEqual(self.pg.sql("SELECT to_regprocedure('neon_dsg_admission_status(text)') IS NULL").stdout.strip(), "t")
        self.assertEqual(self.memory(), before)

    def test_status_does_not_wait_for_real_inflight_resize(self):
        original = self.fixture.request(open=False)
        self.fence(2)
        self.fixture.call(original)
        lib = self.pg.quote(self.pg.library)
        self.pg.sql(f"CREATE FUNCTION dsg_grow_control(text) RETURNS text AS '{lib}','test_dsg_grow_control' LANGUAGE C STRICT; SELECT dsg_grow_control('attach')")
        resize = control.DsgNeonControl("runTest")
        resize.pg, resize.capabilities = self.pg, self.fixture.capability
        request = resize.request()
        request["key"]["issuer_epoch"] = "2"
        process = resize.paused(request)
        reply = self.query(original)
        self.assertEqual(reply["disposition"], "replayed")
        self.assertEqual(reply["observed"]["operation"]["phase"], "preparing")
        self.assertFalse(reply["observed"]["admission_open"])
        resize.finish(process)
