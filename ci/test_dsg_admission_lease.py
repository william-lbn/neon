"""真实PG17失联关闭与deadline CAS；不把准入租约当作预算或VM许可。"""
import copy
import json
import os
import signal
import time
import unittest
from pathlib import Path

import test_dsg_postgres as lifecycle
import test_dsg_neon_admission as admission
import test_dsg_neon_control as control


@unittest.skipUnless(all(os.environ.get(n) for n in admission._ENV), "需真实PG17和配套Neon扩展")
class DsgAdmissionLease(unittest.TestCase):
    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        neon = self.pg.quote(Path(os.environ["NEON_TEST_NEON_LIBRARY"]).resolve())
        self.pg.config(
            f"shared_preload_libraries='{self.pg.quote(self.pg.library)},{neon}'\n"
            "neon.last_written_lsn_cache_size=1024\n"
            "neon.dsg_vm_uid='dsg-vm-1'\nneon.dsg_compute_instance_id='dsg-compute-1'\n"
            "dynamic_shared_buffers_require_admission=on\n"
            "dynamic_shared_buffers_startup_budget='64MB'\n"
            "dynamic_shared_buffers_admission_lease_timeout='1s'\nbgwriter_delay='20ms'\n"
        )
        self.pg.start()
        self.pg.sql("CREATE EXTENSION neon; CREATE ROLE dsg_app LOGIN")
        self.instance = self.value("SELECT neon_dsg_capabilities()")["instance"]
        lib = self.pg.quote(self.pg.library)
        self.pg.sql(f"CREATE FUNCTION dsg_raw_admission() RETURNS boolean AS '{lib}','test_dsg_admission_raw' LANGUAGE C")

    def value(self, sql):
        return json.loads(self.pg.sql(sql).stdout)

    def call(self, function, request, check=True):
        sql = f"SELECT {function}('{self.pg.quote(json.dumps(request))}')"
        result = self.pg.sql(sql, check=check)
        return json.loads(result.stdout) if check else result

    def fence(self, epoch=1):
        return self.call("neon_dsg_fence_issuer", {"protocol_version": 1, "instance": self.instance, "issuer_epoch": str(epoch)})

    def opened(self, epoch=1, revision=1, opened=True):
        return {"protocol_version": 1, "instance": self.instance, "issuer_epoch": str(epoch),
                "revision": str(revision), "expected_capacity_generation": "1", "open": opened,
                "budget_commitment_id": "original-lease-budget"}

    def lease_request(self, fact=None, action="observe", epoch=1):
        return {"protocol_version": 1, "instance": self.instance, "issuer_epoch": str(epoch),
                "expected_capacity_generation": "1", "expected_admission_revision": fact["observed"]["admission_revision"] if fact else "0",
                "expected_deadline_ns": fact["deadline_ns"] if fact else "0", "action": action}

    def lease(self, request=None):
        return self.call("neon_dsg_admission_lease", request or self.lease_request())

    def open(self, **kwargs):
        return self.call("neon_dsg_set_admission", self.opened(**kwargs))

    def wait_expired(self):
        # SU在该消息开始时仍未过期；sleep后raw读取不触发任何原生expire。
        self.assertEqual(self.pg.sql("SELECT pg_sleep(2.2); SELECT dsg_raw_admission()").stdout.splitlines()[-1], "f")
        result = self.lease()
        self.assertTrue(result["expired"])
        self.assertFalse(result["observed"]["admission_open"])
        return result

    def test_background_loss_closes_without_external_coordinator_or_new_queries(self):
        self.fence()
        self.open()
        wire = lifecycle.PgWireConnection(self.pg.socket, "dsg_app")
        self.addCleanup(wire.close)
        fact = self.wait_expired()
        errors = [body for kind, body in wire.exchange([(b"Q", b"SELECT 1\0")]) if kind == b"E"]
        self.assertEqual(len(errors), 1)
        self.assertIn(b"C55000\0", errors[0])
        for revision in (1, 2, 99):
            result = self.call("neon_dsg_set_admission", self.opened(revision=revision), check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("lease expired", result.stderr)
        self.assertFalse(self.lease(self.lease_request(fact, "renew"))["renewed"])
        self.assertEqual(self.open(revision=2, opened=False)["disposition"], "applied")
        self.fence(2)
        self.assertNotEqual(self.call("neon_dsg_set_admission", self.opened(epoch=2, revision=3), check=False).returncode, 0)

    def test_renewal_keeps_service_and_replayed_or_wrong_context_never_extends(self):
        self.fence()
        self.open()
        before = self.lease()
        request = self.lease_request(before, "renew")
        after = self.lease(request)
        self.assertTrue(after["renewed"])
        self.assertGreater(int(after["deadline_ns"]), int(before["deadline_ns"]))
        replay = self.lease(request)
        self.assertFalse(replay["renewed"])
        self.assertEqual(replay["deadline_ns"], after["deadline_ns"])
        for field in ("issuer_epoch", "expected_capacity_generation", "expected_admission_revision"):
            bad = self.lease_request(after, "renew")
            bad[field] = "999"
            reply = self.lease(bad)
            self.assertFalse(reply["renewed"])
            self.assertEqual(reply["deadline_ns"], after["deadline_ns"])
        for _ in range(7):
            time.sleep(0.2)
            after = self.lease(self.lease_request(after, "renew"))
            self.assertTrue(after["renewed"])
        self.assertTrue(after["observed"]["admission_open"])
        self.wait_expired()

    def test_delayed_first_open_and_same_owner_rearm_are_fenced(self):
        self.assertFalse(self.lease()["armed"])
        self.fence()
        time.sleep(1.1)
        expired = self.lease()
        self.assertTrue(expired["expired"])
        self.assertNotEqual(self.call("neon_dsg_set_admission", self.opened(), check=False).returncode, 0)
        self.assertFalse(self.lease(self.lease_request(expired, "rearm"))["renewed"])
        self.assertFalse(self.lease(self.lease_request(expired, "rearm", 2))["renewed"])
        self.fence(2)
        rearmed = self.lease(self.lease_request(expired, "rearm", 2))
        self.assertTrue(rearmed["renewed"])
        self.assertFalse(rearmed["expired"])
        self.assertFalse(rearmed["observed"]["admission_open"], "rearm不开放业务")
        self.assertEqual(self.open()["disposition"], "stale")
        self.assertEqual(self.open(epoch=2)["disposition"], "applied")

    def test_strict_sql_identity_permissions_and_downgrade_retain_native_fence(self):
        self.fence()
        self.open()
        request = self.lease_request(self.lease(), "renew")
        invalid = [{**request, "extra": True}, {**request, "action": "heartbeat"},
                   {**request, "expected_deadline_ns": 1}, {**request, "issuer_epoch": "01"},
                   {k: v for k, v in request.items() if k != "expected_deadline_ns"}]
        bad = copy.deepcopy(request)
        bad["instance"]["vm_uid"] = "other-vm"
        invalid.append(bad)
        for item in invalid:
            with self.subTest(item=item):
                self.assertNotEqual(self.call("neon_dsg_admission_lease", item, check=False).returncode, 0)
        self.assertEqual(self.pg.sql("SELECT has_function_privilege('dsg_app','neon_dsg_admission_lease(text)','EXECUTE')").stdout.strip(), "f")
        self.pg.sql("GRANT EXECUTE ON FUNCTION neon_dsg_admission_lease(text) TO dsg_app")
        raw = self.pg.quote(json.dumps(request))
        result = self.pg.sql(f"SET SESSION AUTHORIZATION dsg_app; SELECT neon_dsg_admission_lease('{raw}')", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.pg.sql("ALTER EXTENSION neon UPDATE TO '1.15'")
        self.assertEqual(self.pg.sql("SELECT to_regprocedure('neon_dsg_admission_lease(text)') IS NULL").stdout.strip(), "t")
        time.sleep(1.1)
        self.pg.sql("ALTER EXTENSION neon UPDATE TO '1.16'")
        self.assertTrue(self.lease()["expired"])
        self.assertFalse(self.lease(request)["renewed"])

    def test_expiry_does_not_wait_for_inflight_native_resize(self):
        self.fence()
        self.open()
        lib = self.pg.quote(self.pg.library)
        self.pg.sql(f"CREATE FUNCTION dsg_grow_control(text) RETURNS text AS '{lib}','test_dsg_grow_control' LANGUAGE C STRICT; SELECT dsg_grow_control('attach')")
        resize = control.DsgNeonControl("runTest")
        resize.pg, resize.capabilities = self.pg, {"instance": self.instance}
        process = resize.paused(resize.request())
        try:
            expired = self.wait_expired()
            self.assertEqual(expired["observed"]["operation"]["phase"], "preparing")
            self.assertFalse(self.lease(self.lease_request(expired, "renew"))["renewed"])
        finally:
            resize.finish(process)
        after = self.lease()
        self.assertEqual(after["observed"]["capacity_generation"], "2")
        self.assertTrue(after["expired"])
        self.assertFalse(after["observed"]["admission_open"])

    def test_internal_recovery_discards_old_lease_and_deadline(self):
        self.fence()
        self.open()
        old = self.lease(self.lease_request(self.lease(), "renew"))
        old_request = self.lease_request(old, "renew")
        postmaster = (self.pg.data / "postmaster.pid").read_text().splitlines()[0]
        sleeper = self.pg.background_sql("SELECT pg_sleep(60)", "dsg-lease-crash")
        self.pg.wait_sql("SELECT count(*) FROM pg_stat_activity WHERE application_name='dsg-lease-crash' AND state='active'", "1")
        pid = int(self.pg.sql("SELECT pid FROM pg_stat_activity WHERE application_name='dsg-lease-crash' AND state='active'").stdout)
        os.kill(pid, signal.SIGKILL)
        sleeper.communicate(timeout=15)
        limit = time.monotonic() + 15
        previous = self.instance
        while time.monotonic() < limit:
            response = self.pg.sql("SELECT neon_dsg_capabilities()", check=False)
            if response.returncode == 0:
                current = json.loads(response.stdout)["instance"]
                if current["pg_boot_id"] != previous["pg_boot_id"]:
                    self.instance = current
                    break
            time.sleep(0.02)
        self.assertNotEqual(self.instance["pg_boot_id"], previous["pg_boot_id"])
        self.assertEqual((self.pg.data / "postmaster.pid").read_text().splitlines()[0], postmaster)
        new = self.lease()
        self.assertFalse(new["armed"])
        self.assertEqual(new["deadline_ns"], "0")
        self.assertFalse(new["observed"]["admission_open"])
        self.assertNotEqual(self.call("neon_dsg_admission_lease", old_request, check=False).returncode, 0)
        self.fence()
        self.assertTrue(self.lease()["armed"])
