"""真实PG17的issuer栅栏准入；只新增受影响范围，不继承重复旧生命周期套件。

C桥只转换参数/快照，所有发布、重放、锁与恢复来自内核。此测试不声称完成
预算、后台排空、Agent或VM链，也不能替代Linux K8s/物理内存验证。
"""
import json
import os
import signal
import time
import unittest

import test_dsg_control as control
import test_dsg_postgres as lifecycle

APPLIED, REPLAYED, BUSY, STALE, CONFLICT = range(5)


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需显式指定真实PG及配套库")
class DsgAdmissionLifecycle(unittest.TestCase):
    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        self.pg.config("dynamic_shared_buffers_require_admission=on\ndynamic_shared_buffers_startup_budget='64MB'\n")
        self.pg.prepare_grow_exit_fixture()
        self.pg.install_functions()
        lib = self.pg.quote(self.pg.library)
        self.pg.sql(
            f"CREATE FUNCTION dsg_admit(text,text,text,text,boolean,integer,boolean DEFAULT false) RETURNS jsonb AS '{lib}','test_dsg_control_admission' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_fence(text,text,boolean DEFAULT false) RETURNS jsonb AS '{lib}','test_dsg_fence_issuer' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_control(text,text,text,text,text,text,text,integer,integer,integer,integer) RETURNS jsonb AS '{lib}','test_dsg_control' LANGUAGE C STRICT;"
            "CREATE ROLE dsg_app LOGIN"
        )
        self.boot = self.read_boot()
        self.control = control.DsgControlLifecycle("runTest")
        self.control.pg, self.control.boot = self.pg, self.boot

    def read_boot(self):
        # 测试桥历史boot(bigint)保留位模式；生产wire/本入口始终为规范uint64文本。
        return str(int(self.pg.sql("SELECT dsg_boot()").stdout) & ((1 << 64) - 1))

    def statement(self, *, boot=None, epoch=1, revision=1, generation=1, opened=True, token=1, guarded=False):
        values = (self.boot if boot is None else boot, epoch, revision, generation)
        ids = ",".join(f"'{self.pg.quote(value)}'" for value in values)
        return f"SELECT dsg_admit({ids},{str(opened).lower()},{token},{str(guarded).lower()})"

    def call(self, **parameters):
        return json.loads(self.pg.sql(self.statement(**parameters)).stdout)

    def fence(self, epoch):
        return json.loads(self.pg.sql(f"SELECT dsg_fence('{self.boot}','{epoch}')").stdout)

    def assert_wire(self, connection, accepted):
        messages = connection.exchange([(b"Q", b"SELECT 1\0")])
        errors = [body for kind, body in messages if kind == b"E"]
        if accepted:
            self.assertFalse(errors, errors)
        else:
            self.assertEqual(len(errors), 1)
            self.assertIn(b"C55000\0", errors[0])

    def test_owner_context_replay_and_same_connection_admission(self):
        wire = lifecycle.PgWireConnection(self.pg.socket, "dsg_app")
        self.addCleanup(wire.close)
        self.assert_wire(wire, False)
        self.assertEqual(self.call()["disposition"], STALE, "开门不能隐式提升floor")
        self.assertEqual(self.fence(1)["disposition"], control.FENCED)
        self.assertEqual(self.call()["disposition"], APPLIED)
        self.assert_wire(wire, True)
        self.assertEqual(self.call()["disposition"], REPLAYED)
        self.assertEqual(self.call(token=2)["disposition"], CONFLICT)
        self.assertEqual(self.call(opened=False)["disposition"], CONFLICT)
        self.assertEqual(self.call(revision=2, opened=False, token=3)["disposition"], APPLIED)
        self.assert_wire(wire, False)
        self.assertEqual(self.call()["disposition"], STALE, "迟到旧开门不能覆盖关门")
        self.fence(2)
        self.assertEqual(self.call(revision=3)["disposition"], STALE)
        self.assertEqual(self.call(epoch=2, revision=3, token=4)["disposition"], APPLIED)
        self.assert_wire(wire, True)
        invalid = self.pg.sql(self.statement(epoch=2, revision=4, guarded=True), check=False)
        self.assertNotEqual(invalid.returncode, 0)
        self.assertIn("coordination locks", invalid.stderr)
        result = self.pg.sql("SET SESSION AUTHORIZATION dsg_app; " + self.statement(epoch=2, revision=4), check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("superuser session", result.stderr)

    def test_close_does_not_wait_for_resize_but_open_requires_settled_generation(self):
        self.fence(1)
        process = self.control.paused(target=1024)
        closed = self.call(opened=False)
        self.assertEqual(closed["disposition"], APPLIED)
        self.assertFalse(closed["admission_open"])
        self.assertEqual(self.call(revision=2, token=2)["disposition"], BUSY)
        self.control.finish(process)
        self.assertEqual(self.call(revision=2, token=2)["disposition"], STALE)
        self.assertEqual(self.call(revision=2, generation=2, token=3)["disposition"], APPLIED)

    def test_internal_recovery_clears_original_admission_identity(self):
        self.fence(1)
        self.assertEqual(self.call()["disposition"], APPLIED)
        postmaster = (self.pg.data / "postmaster.pid").read_text().splitlines()[0]
        sleeper = self.pg.background_sql("SELECT pg_sleep(60)", "dsg-admission-owned-crash")
        self.pg.wait_sql("SELECT count(*) FROM pg_stat_activity WHERE application_name='dsg-admission-owned-crash' AND state='active'", "1")
        pid = int(self.pg.sql("SELECT pid FROM pg_stat_activity WHERE application_name='dsg-admission-owned-crash' AND state='active'").stdout)
        # 只终止本测试刚启动的临时PG会话，真实同Postmaster恢复，而非伪造boot。
        os.kill(pid, signal.SIGKILL)
        sleeper.communicate(timeout=15)
        end = time.monotonic() + 15
        new_boot = self.boot
        while time.monotonic() < end:
            result = self.pg.sql("SELECT dsg_boot()", check=False)
            if result.returncode == 0:
                new_boot = str(int(result.stdout) & ((1 << 64) - 1))
                if new_boot != self.boot:
                    break
            time.sleep(0.02)
        self.assertNotEqual(new_boot, self.boot)
        self.assertEqual((self.pg.data / "postmaster.pid").read_text().splitlines()[0], postmaster)
        old = self.call()
        self.assertEqual(old["disposition"], STALE)
        self.assertFalse(old["admission_open"])
        self.assertEqual(old["admission_revision"], "0")
        self.boot = new_boot
        self.assertEqual(self.call()["disposition"], STALE)
        self.fence(2)
        self.assertEqual(self.call(epoch=2)["disposition"], APPLIED)
