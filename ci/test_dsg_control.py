"""在真实PG进程中验证R09的回执、历史、换代和业务取消。

复用生命周期集群夹具，不继承/重复运行原有48项。测试桥只做参数与结果转换；
状态变化来自真实协调器、内核锁、Provider和原生异常清理，不伪造执行结果。
这些是内核契约测试；生产Neon管理认证、预算承诺和VM执行链须各自集成验证。
"""

import json
import unittest

import test_dsg_postgres as lifecycle

ACCEPTED, REPLAYED, BUSY, STALE, CONFLICT = range(5)
RESULT_EXPIRED, NOT_FOUND, CANCEL_REQUESTED, TOO_LATE, REJECTED = range(5, 10)
GROW, SHRINK = 0, 1
PREPARING, COMMITTED, DENIED = 1, 2, 3
INTERRUPTED, CANCELLED = 3, 4
MAX_U64 = (1 << 64) - 1


@unittest.skipUnless(
    bool(lifecycle._CONFIGURED_ENV),
    "真实控制契约测试需要显式指定PG及两个配套库；prepare阶段不冒充已执行",
)
class DsgControlLifecycle(unittest.TestCase):
    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.pg.setUp()
        self.addCleanup(self.pg.doCleanups)
        self.pg.prepare_grow_exit_fixture()
        library = self.pg.quote(self.pg.library)
        self.signature = "text,text,text,text,text,text,text,integer,integer,integer,integer"
        self.pg.sql(
            f"CREATE FUNCTION dsg_control({self.signature}) RETURNS jsonb "
            f"AS '{library}', 'test_dsg_control' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_parse(text) RETURNS text AS '{library}', "
            "'test_dsg_control_parse' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_hold_guard() RETURNS boolean AS '{library}', "
            "'test_dsg_control_hold_guard' LANGUAGE C;"
        )
        self.boot = self.state()["boot"]

    def statement(
        self,
        action="resize",
        *,
        boot=None,
        epoch=1,
        revision=1,
        hi=0,
        lo=1,
        generation=1,
        target=1024,
        kind=GROW,
        timeout=10000,
        token=0,
    ):
        # 身份一律作为text传递，不经SQL signed bigint隐式转换。
        values = (self.boot if boot is None else boot, epoch, revision, hi, lo, generation)
        strings = ",".join(f"'{self.pg.quote(value)}'" for value in values)
        return f"SELECT dsg_control('{action}',{strings},{target},{kind},{timeout},{token})"

    def call(self, action="resize", **parameters):
        return json.loads(self.pg.sql(self.statement(action, **parameters)).stdout)

    def state(self):
        return self.call("state", boot=0)["observed"]

    def arm(self, action):
        self.pg.sql(f"SELECT dsg_grow_control('{action}')")

    def paused(self, **parameters):
        process = self.pg.background_sql(
            "SELECT dsg_grow_control('arm_pause'); " + self.statement(**parameters),
            "dsg-control-paused",
        )
        self.pg.wait_sql("SELECT dsg_grow_control('prepare_waiting')", "yes")
        return process

    def finish(self, process):
        self.arm("release_prepare")
        output, errors = process.communicate(timeout=10)
        self.assertEqual(process.returncode, 0, errors + self.pg.log.read_text()[-12000:])
        return json.loads(output.strip().splitlines()[-1])

    def fixture_data(self):
        self.call(target=2048)
        self.pg.sql(
            "CREATE TABLE dsg_control_data(i integer,pad text);"
            "ALTER TABLE dsg_control_data ALTER pad SET STORAGE PLAIN;"
            "INSERT INTO dsg_control_data SELECT i,repeat(md5(i::text),150) "
            "FROM generate_series(1,600) i;"
        )
        return self.checksum()

    def checksum(self):
        return self.pg.sql(
            "SELECT count(*),sum(i),sum(octet_length(pad)),sum(hashtextextended(pad,0)) "
            "FROM dsg_control_data"
        ).stdout

    def test_history_result_and_current_state_have_distinct_identities(self):
        first = self.call()
        self.assertEqual(first["disposition"], ACCEPTED)
        self.assertEqual(first["result"]["high"], 1024)
        self.call(revision=2, lo=2, generation=2, target=256, kind=SHRINK)
        self.call(revision=3, lo=3, generation=3, target=2048)
        for action in ("resize", "status"):
            replay = self.call(action)
            self.assertEqual(replay["disposition"], REPLAYED)
            self.assertEqual(replay["key"]["lo"], "1")
            self.assertEqual(
                (replay["result"]["high"], replay["result"]["generation"]), (1024, "2")
            )
            self.assertEqual((replay["observed"]["high"], replay["observed"]["lo"]), (2048, "3"))
            self.assertTrue(replay["result"]["replayed"])

    def test_reusing_operation_id_with_changed_intent_is_conflict(self):
        self.call()
        self.call(revision=2, lo=2, generation=2, target=2048)
        before = self.state()
        for change in (
            {"target": 2048},
            {"generation": 2},
            {"kind": SHRINK},
            {"token": 1},
            {"epoch": 2},
            {"revision": 9},
        ):
            with self.subTest(change=change):
                result = self.call(**change)
                self.assertEqual(result["disposition"], CONFLICT)
                self.assertFalse(result["has_result"])
                self.assertEqual(self.state(), before)

    def test_bounded_history_expiry_never_reexecutes_old_request(self):
        for revision in range(1, 22):
            result = self.call(revision=revision, lo=revision, target=256)
            self.assertEqual(result["disposition"], ACCEPTED)
        before = self.state()
        result = self.call("status", target=256)
        self.assertEqual(result["disposition"], RESULT_EXPIRED)
        self.assertFalse(result["has_result"])
        self.assertEqual(self.call(target=256)["disposition"], STALE)
        # 21是当前操作，16条历史覆盖5..20；4已淘汰，5恰在保留边界。
        self.assertEqual(self.call("status", revision=4, lo=4)["disposition"], RESULT_EXPIRED)
        self.assertEqual(self.call("status", revision=5, lo=5)["disposition"], REPLAYED)
        self.assertEqual(self.state(), before)

    def test_historical_receipt_does_not_cancel_current_preparing_operation(self):
        self.call()
        running = self.paused(revision=2, lo=2, generation=2, target=2048)
        historical = self.call("cancel")
        self.assertEqual(historical["disposition"], TOO_LATE)
        self.assertEqual(
            (historical["result"]["lo"], historical["result"]["phase"]), ("1", COMMITTED)
        )
        self.assertEqual(
            (historical["observed"]["lo"], historical["observed"]["phase"]), ("2", PREPARING)
        )
        self.assertFalse(self.state()["cancel_requested"])
        replay = self.call()
        self.assertEqual(replay["disposition"], REPLAYED)
        self.assertEqual(replay["result"]["high"], 1024)
        self.assertEqual(self.finish(running)["result"]["high"], 2048)

    def test_new_epoch_resets_revision_and_preserves_unsigned_identity(self):
        first = dict(epoch=MAX_U64 - 1, revision=MAX_U64, hi=1 << 63, lo=MAX_U64)
        result = self.call(**first)
        self.assertEqual(result["disposition"], ACCEPTED)
        self.assertEqual(result["result"]["hi"], str(1 << 63))
        self.assertEqual(result["result"]["revision"], str(MAX_U64))
        result = self.call(epoch=MAX_U64, revision=1, hi=MAX_U64, lo=0, generation=2, target=2048)
        self.assertEqual(result["disposition"], ACCEPTED)
        self.assertEqual(result["result"]["epoch"], str(MAX_U64))
        self.assertEqual(self.call(**first)["disposition"], REPLAYED)
        self.assertEqual(
            self.call(epoch=MAX_U64 - 1, revision=MAX_U64, lo=9, generation=3)["disposition"],
            STALE,
        )

    def test_busy_receipt_names_waiting_request_not_running_operation(self):
        running = self.paused()
        result = self.call(revision=2, lo=2, timeout=20)
        self.assertEqual(result["disposition"], BUSY)
        self.assertEqual(result["key"]["lo"], "2")
        self.assertFalse(result["has_result"])
        self.assertIsNone(result["result"])
        self.assertEqual((result["observed"]["lo"], result["observed"]["phase"]), ("1", PREPARING))
        # 同一意图即使剩余等待为零，也必须能查询已被接受的运行事实。
        duplicate = self.call(timeout=0)
        self.assertEqual(duplicate["disposition"], REPLAYED)
        self.assertEqual(duplicate["result"]["phase"], PREPARING)
        self.assertEqual(self.finish(running)["result"]["phase"], COMMITTED)

    def test_cancel_grow_before_commit_preserves_capacity_and_charged_cost(self):
        before = self.state()
        running = self.paused()
        prepared = self.state()
        for _ in range(2):
            cancelled = self.call("cancel")
            self.assertEqual(cancelled["disposition"], CANCEL_REQUESTED)
            self.assertTrue(cancelled["result"]["cancel_requested"])
        result = self.finish(running)
        self.assertEqual(
            (result["result"]["phase"], result["result"]["reason"]), (DENIED, CANCELLED)
        )
        self.assertEqual((result["observed"]["low"], result["observed"]["high"]), (256, 256))
        for old, allocated, final in zip(
            before["arrays"], prepared["arrays"], result["observed"]["arrays"], strict=True
        ):
            self.assertGreaterEqual(int(allocated["upper"]), int(old["upper"]))
            self.assertEqual(final["upper"], allocated["upper"])
        self.assertEqual(self.call()["result"]["reason"], CANCELLED)
        self.assertEqual(self.call(revision=2, lo=2)["result"]["phase"], COMMITTED)
        historical = self.call("status")
        self.assertEqual(historical["result"]["high"], 256)
        self.assertEqual(historical["observed"]["high"], 1024)
        self.assertEqual(self.call("cancel")["disposition"], TOO_LATE)

    def test_cancel_shrink_restores_fence_and_preserves_real_data(self):
        checksum = self.fixture_data()
        self.arm("attach_shrink_fence")
        request = dict(revision=2, lo=2, generation=2, target=256, kind=SHRINK)
        running = self.paused(**request)
        self.assertEqual((self.state()["low"], self.state()["high"]), (256, 2048))
        self.assertEqual(self.call("cancel", **request)["disposition"], CANCEL_REQUESTED)
        result = self.finish(running)
        self.assertEqual(
            (result["result"]["phase"], result["result"]["reason"]), (DENIED, CANCELLED)
        )
        self.assertEqual((self.state()["low"], self.state()["high"]), (2048, 2048))
        self.assertEqual(self.checksum(), checksum)
        self.arm("detach_shrink")
        self.assertEqual(
            self.call(revision=3, lo=3, generation=2, target=256, kind=SHRINK)["result"]["phase"],
            COMMITTED,
        )
        self.assertEqual(self.checksum(), checksum)

    def test_cancel_after_publish_is_too_late_and_never_restores_high(self):
        self.call()
        self.arm("attach_release")
        request = dict(revision=2, lo=2, generation=2, target=256, kind=SHRINK)
        running = self.paused(**request)
        result = self.call("cancel", **request)
        self.assertEqual(result["disposition"], TOO_LATE)
        self.assertTrue(result["result"]["cancel_closed"])
        self.assertFalse(result["result"]["cancel_requested"])
        self.assertEqual(result["observed"]["high"], 256)
        self.assertEqual(self.finish(running)["result"]["phase"], COMMITTED)
        self.assertEqual(self.state()["high"], 256)

    def exercise_cancel_while_waiting_for_guard(self, kind):
        request = {}
        if kind == SHRINK:
            self.call()
            request = dict(revision=2, lo=2, generation=2, target=256, kind=SHRINK)
        self.arm("attach_guard")
        scanner = self.pg.background_sql(
            "SELECT dsg_grow_control('arm_pause'); SELECT dsg_hold_guard()",
            "dsg-control-guard",
        )
        self.pg.wait_sql("SELECT dsg_grow_control('prepare_waiting')", "yes")
        running = self.pg.background_sql(self.statement(**request), "dsg-control-lock-waiter")
        self.pg.wait_sql(
            "SELECT count(*) FROM pg_stat_activity WHERE "
            "application_name='dsg-control-lock-waiter' AND wait_event='DsgBufferResize'",
            "1",
        )
        self.assertEqual(self.call("cancel", **request)["disposition"], CANCEL_REQUESTED)
        # Guard仍被真实持有；业务取消必须自行退出等待，不能靠先放Guard或等10s期限。
        output, errors = running.communicate(timeout=2)
        self.assertEqual(running.returncode, 0, errors)
        result = json.loads(output)
        self.assertEqual(
            (result["result"]["phase"], result["result"]["reason"]), (DENIED, CANCELLED)
        )
        self.assertIsNone(scanner.poll())
        self.arm("release_prepare")
        _, errors = scanner.communicate(timeout=10)
        self.assertEqual(scanner.returncode, 0, errors)

    def test_cancel_grow_waiting_for_access_lock_is_prompt(self):
        self.exercise_cancel_while_waiting_for_guard(GROW)

    def test_cancel_shrink_waiting_for_access_lock_is_prompt(self):
        self.exercise_cancel_while_waiting_for_guard(SHRINK)

    def test_wrong_cancel_key_and_unknown_request_cannot_cancel_running_grow(self):
        running = self.paused()
        self.assertEqual(self.call("cancel", epoch=2)["disposition"], CONFLICT)
        self.assertEqual(self.call("cancel", revision=2, lo=2)["disposition"], NOT_FOUND)
        self.assertFalse(self.state()["cancel_requested"])
        self.assertEqual(self.finish(running)["result"]["phase"], COMMITTED)

    def test_request_deadline_zero_rejects_new_but_allows_terminal_replay(self):
        self.assertEqual(self.call(timeout=0)["disposition"], REJECTED)
        self.assertEqual(self.state()["phase"], 0)
        self.call()
        self.assertEqual(self.call(timeout=0)["disposition"], REPLAYED)
        self.assertEqual(self.call(revision=2, lo=2, timeout=0)["disposition"], REJECTED)
        self.assertEqual(self.state()["lo"], "1")

    def test_sql_rollback_cannot_undo_committed_resize(self):
        self.pg.sql("BEGIN; " + self.statement() + "; ROLLBACK")
        result = self.call("status")
        self.assertEqual((result["result"]["phase"], result["observed"]["high"]), (COMMITTED, 1024))

    def test_sql_error_is_durable_denial_and_does_not_reexecute(self):
        result = self.pg.sql(
            "SELECT dsg_grow_control('arm_error'); " + self.statement(), check=False
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("DSGA test resize ERROR", result.stderr)
        for action in ("status", "resize"):
            result = self.call(action)
            self.assertEqual(
                (result["result"]["phase"], result["result"]["reason"]), (DENIED, INTERRUPTED)
            )
            self.assertEqual(result["observed"]["high"], 256)

    def test_query_reconciles_fatal_without_overwriting_next_operation(self):
        old = self.pg.background_sql(
            "SELECT dsg_grow_control('arm_fatal'); " + self.statement(), "dsg-control-fatal"
        )
        self.pg.wait_sql("SELECT dsg_grow_control('exit_waiting')", "yes")
        self.assertEqual(self.state()["phase"], PREPARING)
        reconciled = self.call("status")
        self.assertEqual(
            (reconciled["result"]["phase"], reconciled["result"]["reason"]), (DENIED, INTERRUPTED)
        )
        self.call(revision=2, lo=2, target=2048)
        before = self.state()
        self.arm("release_exit")
        _, errors = old.communicate(timeout=10)
        self.assertNotEqual(old.returncode, 0)
        self.assertIn("DSGA test Grow FATAL", errors)
        self.assertEqual(self.state(), before)
        self.assertEqual(self.call("status")["result"]["high"], 256)

    def test_new_boot_rejects_old_resize_status_and_cancel(self):
        self.call()
        old_boot = self.boot
        self.pg.command("pg_ctl", "-D", self.pg.data, "-m", "fast", "-w", "stop")
        self.pg.running = False
        self.pg.start()
        self.assertNotEqual(self.state()["boot"], old_boot)
        for action in ("resize", "status", "cancel"):
            result = self.call(action)
            self.assertEqual(result["disposition"], STALE)
            self.assertFalse(result["has_result"])
        self.assertEqual(self.state()["high"], 256)

    def test_release_identity_includes_epoch_and_both_operation_halves(self):
        self.call()
        first = self.call(revision=2, lo=2, generation=2, target=256, kind=SHRINK)
        next_request = dict(
            epoch=2, revision=2, hi=MAX_U64, lo=0, generation=3, target=256, kind=SHRINK
        )
        second = self.call(**next_request)
        for receipt, epoch, hi, lo in ((first, "1", "0", "2"), (second, "2", str(MAX_U64), "0")):
            for array in receipt["result"]["arrays"]:
                self.assertEqual(
                    (array["epoch"], array["revision"], array["hi"], array["lo"]),
                    (epoch, "2", hi, lo),
                )
                self.assertEqual(array["generation"], "3")
        historical = self.call("status", revision=2, lo=2)
        self.assertEqual(historical["result"]["arrays"][0]["epoch"], "1")
        self.assertEqual(historical["observed"]["arrays"][0]["epoch"], "2")

    def test_canonical_uint64_parser_rejects_ambiguous_and_overflowing_text(self):
        for value in ("0", "1", str(1 << 63), str(MAX_U64)):
            self.assertEqual(self.pg.sql(f"SELECT dsg_parse('{value}')").stdout.strip(), value)
        for value in (
            "",
            "00",
            "01",
            "+1",
            "-1",
            " 1",
            "1 ",
            "1.0",
            "1e2",
            "１",
            str(MAX_U64 + 1),
            "9" * 21,
        ):
            with self.subTest(value=value):
                result = self.pg.sql(f"SELECT dsg_parse('{self.pg.quote(value)}')", check=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("canonical decimal string", result.stderr)

    def test_ordinary_and_security_definer_callers_are_rejected(self):
        self.pg.sql(
            "CREATE ROLE dsg_control_app LOGIN;"
            f"GRANT EXECUTE ON FUNCTION dsg_control({self.signature}) TO dsg_control_app;"
            "CREATE FUNCTION dsg_definer() RETURNS text LANGUAGE SQL SECURITY DEFINER "
            "AS $$SELECT dsg_parse('1')$$;"
        )
        for query in (self.statement(), "SELECT dsg_parse('1')", "SELECT dsg_definer()"):
            result = self.pg.sql("SET SESSION AUTHORIZATION dsg_control_app; " + query, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("superuser session", result.stderr)

    def test_control_rejects_reverse_lock_order_even_for_known_receipt(self):
        self.call()
        for action in ("resize", "status", "cancel"):
            result = self.pg.sql(self.statement("guarded_" + action), check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("holding buffer coordination locks", result.stderr)
        # 原生ERROR已释放Guard；下一真实扩容可发布，不留下无法观察的读锁。
        self.assertEqual(
            self.call(revision=2, lo=2, generation=2, target=2048)["result"]["phase"], COMMITTED
        )


if __name__ == "__main__":
    unittest.main()
