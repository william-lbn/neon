"""Neon正式1.9管理SQL准入：升级、完整原意图和权限；不重复内核套件。"""
import copy
import hashlib
import json
import os
import unittest
from pathlib import Path

import test_dsg_postgres as lifecycle

_ENV = (*lifecycle._REQUIRED_ENV, "NEON_TEST_NEON_LIBRARY")


@unittest.skipUnless(all(os.environ.get(name) for name in _ENV), "需显式指定真实PG/Neon配套库")
class DsgNeonAdmission(unittest.TestCase):
    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        neon = self.pg.quote(Path(os.environ["NEON_TEST_NEON_LIBRARY"]).resolve())
        self.pg.config(
            f"shared_preload_libraries='{self.pg.quote(self.pg.library)},{neon}'\n"
            "neon.last_written_lsn_cache_size=1024\n"
            "neon.dsg_vm_uid='dsg-vm-1'\nneon.dsg_compute_instance_id='dsg-compute-1'\n"
            "dynamic_shared_buffers_require_admission=on\ndynamic_shared_buffers_startup_budget='64MB'\n"
        )
        self.pg.start()
        self.pg.sql("CREATE EXTENSION neon VERSION '1.8'; ALTER EXTENSION neon UPDATE TO '1.9'; CREATE ROLE dsg_app LOGIN")
        self.capability = self.value("SELECT neon_dsg_capabilities()")

    def value(self, sql):
        return json.loads(self.pg.sql(sql).stdout)

    def request(self, **changes):
        result = {"protocol_version": 1, "instance": self.capability["instance"].copy(),
                  "issuer_epoch": "2", "revision": "1", "expected_capacity_generation": "1",
                  "open": True, "budget_commitment_id": "original-startup-budget"}
        result.update(changes)
        return result

    def statement(self, request):
        raw = request if isinstance(request, str) else json.dumps(request)
        return f"SELECT neon_dsg_set_admission('{self.pg.quote(raw)}')"

    def call(self, request):
        return self.value(self.statement(request))

    def test_formal_upgrade_receipt_full_context_and_administrator_isolation(self):
        self.assertEqual(self.pg.sql("SELECT extversion FROM pg_extension WHERE extname='neon'").stdout.strip(), "1.9")
        self.assertEqual(self.pg.sql("SELECT has_function_privilege('dsg_app','neon_dsg_set_admission(text)','EXECUTE')").stdout.strip(), "f")
        request = self.request()
        fence = {"protocol_version": 1, "instance": request["instance"], "issuer_epoch": "2"}
        self.value(f"SELECT neon_dsg_fence_issuer('{self.pg.quote(json.dumps(fence))}')")
        reply = self.call(request)
        self.assertEqual(reply["disposition"], "applied")
        self.assertEqual(reply["request"], request)
        self.assertTrue(reply["observed"]["admission_open"])
        self.assertFalse(reply["vm_shrink_permit"])
        canonical = self.pg.sql(f"SELECT '{self.pg.quote(json.dumps(request))}'::jsonb::text").stdout.strip()
        self.assertEqual(reply["context_sha256"], hashlib.sha256(canonical.encode()).hexdigest())
        # 不同字段顺序/空白不改原意图，预算引用不同即使open相同也不能冒充重放。
        replay = self.call(json.dumps(dict(reversed(list(request.items()))), separators=(",", ":")))
        self.assertEqual(replay["disposition"], "replayed")
        self.assertEqual(self.call(self.request(budget_commitment_id="different-budget"))["disposition"], "conflict")
        self.pg.sql("GRANT EXECUTE ON FUNCTION neon_dsg_set_admission(text) TO dsg_app")
        direct = self.pg.sql("SET SESSION AUTHORIZATION dsg_app; " + self.statement(request), check=False)
        self.assertNotEqual(direct.returncode, 0)
        self.assertIn("superuser session", direct.stderr)
        self.pg.sql("CREATE FUNCTION bad_admission_definer(text) RETURNS jsonb LANGUAGE SQL SECURITY DEFINER AS $$ SELECT neon_dsg_set_admission($1) $$")
        indirect = self.pg.sql("SET SESSION AUTHORIZATION dsg_app; SELECT bad_admission_definer('" + self.pg.quote(json.dumps(request)) + "')", check=False)
        self.assertNotEqual(indirect.returncode, 0)
        self.assertIn("superuser session", indirect.stderr)

    def test_formal_strict_request_identity_and_reversible_sql_upgrade(self):
        request = self.request()
        raw = json.dumps(request)
        invalid = [raw[:-1] + ',"open":false}', {**request, "extra": True},
                   {key: value for key, value in request.items() if key != "budget_commitment_id"},
                   self.request(open="true"), self.request(revision="01"), self.request(issuer_epoch="0")]
        for value in invalid:
            with self.subTest(value=str(value)[:60]):
                self.assertNotEqual(self.pg.sql(self.statement(value), check=False).returncode, 0)
        wrong = copy.deepcopy(request)
        wrong["instance"]["vm_uid"] = "foreign-vm"
        reply = self.call(wrong)
        self.assertEqual(reply["disposition"], "stale")
        self.assertFalse(reply["observed"]["admission_open"])
        self.pg.sql("ALTER EXTENSION neon UPDATE TO '1.8'")
        self.assertEqual(self.pg.sql("SELECT to_regprocedure('neon_dsg_set_admission(text)') IS NULL").stdout.strip(), "t")
        self.pg.sql("ALTER EXTENSION neon UPDATE TO '1.9'")
        self.assertEqual(self.pg.sql("SELECT has_function_privilege('dsg_app','neon_dsg_set_admission(text)','EXECUTE')").stdout.strip(), "f")
        self.assertFalse(self.value("SELECT neon_dsg_memory_status()")["observed"]["admission_open"])
