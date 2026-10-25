"""原生两阶段槽覆盖未valid/退出/失败/恢复，前台idle不能代替保留资源为零。"""
import json
import os
import signal
import struct
import time
import unittest

import test_dsg_connection_gate as connections
import test_dsg_postgres as lifecycle


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要真实PG及配套测试库")
class DsgPreparedLifecycle(unittest.TestCase):
    def setUp(self):
        self.control = connections.DsgConnectionGateLifecycle("runTest")
        self.control.extra_postgres_config = "max_prepared_transactions=4\n"
        self.addCleanup(self.control.doCleanups)
        self.control.setUp()
        self.pg = self.control.pg
        library = self.pg.quote(self.pg.library)
        self.pg.sql(f"CREATE FUNCTION dsg_prepared() RETURNS text AS '{library}', 'test_dsg_prepared_snapshot' LANGUAGE C; "
                    f"CREATE FUNCTION dsg_prepared_control(text) RETURNS int AS '{library}', 'test_dsg_prepared_control' LANGUAGE C STRICT; "
                    "CREATE TABLE prepared_data(i int); GRANT INSERT ON prepared_data TO dsg_app")

    def fact(self):
        messages = self.control.observer.exchange([(b"Q", b"SELECT dsg_prepared()\0")])
        fact = json.loads(self.control.row(messages))
        self.assertEqual(fact["maximum_prepared_slots"], 4)
        self.assertLessEqual(fact["prepared_slots"], 4)
        return fact

    def wait(self, **expected):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            fact = self.fact()
            if all(fact[k] == v for k, v in expected.items()):
                return fact
            time.sleep(0.01)
        self.fail(f"prepared native fact did not reach {expected}: {fact}")

    def peer(self, admin=False):
        if admin:
            return self.control.peer(self.control.private, os.environ.get("USER", "williamlee"))
        return self.control.peer()

    @staticmethod
    def query(peer, query):
        return peer.exchange([(b"Q", query.encode() + b"\0")])

    def success(self, messages, status=b"I"):
        self.assertFalse(any(k == b"E" for k, _ in messages), messages)
        self.assertEqual(messages[-1], (b"Z", status))

    def error(self, messages, code):
        errors = [b for k, b in messages if k == b"E"]
        self.assertEqual(len(errors), 1, messages)
        self.assertIn(b"C" + code.encode() + b"\0", errors[0])

    def arm(self, peer, mode):
        self.pg.sql("SELECT dsg_prepared_control('attach')")
        self.success(self.query(peer, f"SELECT dsg_prepared_control('{mode}')"))

    def test_prepared_survives_backend_exit_and_foreground_idle(self):
        self.control.gate(True)
        peer = self.peer()
        self.success(self.query(peer, "BEGIN; INSERT INTO prepared_data VALUES(42); PREPARE TRANSACTION 'owned'"))
        peer.close()
        self.control.gate(False)
        self.wait(prepared_slots=1, foreground_idle=True, public_connections_blocked=True)
        self.assertEqual(self.pg.sql("SELECT count(*) FROM pg_prepared_xacts").stdout.strip(), "1")
        # 显式测试协调者完成原意图，不由伸缩器擅自选择提交/回滚客户事务。
        self.pg.sql("COMMIT PREPARED 'owned'")
        self.wait(prepared_slots=0, foreground_idle=True)
        self.assertEqual(self.pg.sql("SELECT i FROM prepared_data").stdout.strip(), "42")

    def test_closed_gate_blocks_new_prepare_even_for_management(self):
        self.control.gate(True)
        peer = self.peer(admin=True)
        self.success(self.query(peer, "BEGIN; INSERT INTO prepared_data VALUES(1)"), b"T")
        self.control.gate(False)
        self.error(self.query(peer, "PREPARE TRANSACTION 'blocked'"), "55000")
        self.wait(prepared_slots=0)
        self.success(self.query(peer, "ROLLBACK"))
        self.control.gate(True)
        self.success(self.query(peer, "BEGIN; INSERT INTO prepared_data VALUES(2); PREPARE TRANSACTION 'blocked'"))
        self.control.gate(False)
        self.wait(prepared_slots=1)
        self.pg.sql("ROLLBACK PREPARED 'blocked'")
        self.wait(prepared_slots=0)
        self.assertEqual(self.pg.sql("SELECT count(*) FROM prepared_data").stdout.strip(), "0")

    def test_reserved_before_valid_and_close_race_remains_visible(self):
        self.control.gate(True)
        peer = self.peer(admin=True)
        self.arm(peer, "arm_wait")
        body = b"BEGIN; INSERT INTO prepared_data VALUES(3); PREPARE TRANSACTION 'reserved'\0"
        peer.connection.sendall(b"Q" + struct.pack("!I", len(body) + 4) + body)
        self.pg.wait_sql("SELECT dsg_prepared_control('state')", "1")
        self.control.gate(False)
        self.wait(prepared_slots=1, frontend_executes=1, foreground_idle=False,
                  acknowledged=True, public_connections_blocked=True)
        self.pg.sql("SELECT dsg_prepared_control('release')")
        self.success(peer.receive_ready())
        self.wait(prepared_slots=1, foreground_idle=True)
        self.pg.sql("ROLLBACK PREPARED 'reserved'")
        self.pg.sql("SELECT dsg_prepared_control('detach')")
        self.wait(prepared_slots=0)

    def test_actual_error_after_registration_recovers_slot_without_faking_zero(self):
        self.control.gate(True)
        peer = self.peer(admin=True)
        self.arm(peer, "arm_error")
        self.error(self.query(peer, "BEGIN; INSERT INTO prepared_data VALUES(4); PREPARE TRANSACTION 'retry'"), "XX000")
        self.wait(prepared_slots=0)
        self.success(self.query(peer, "ROLLBACK"))
        self.success(self.query(peer, "BEGIN; INSERT INTO prepared_data VALUES(5); PREPARE TRANSACTION 'retry'"))
        self.control.gate(False)
        self.wait(prepared_slots=1)
        self.pg.sql("ROLLBACK PREPARED 'retry'")
        self.pg.sql("SELECT dsg_prepared_control('detach')")
        self.wait(prepared_slots=0)
        self.assertEqual(self.pg.sql("SELECT count(*) FROM prepared_data").stdout.strip(), "0")

    def test_duplicate_and_full_native_slots_do_not_leak_registration(self):
        self.control.gate(True)
        peer = self.peer()
        for i in range(4):
            self.success(self.query(peer, f"BEGIN; INSERT INTO prepared_data VALUES({i}); PREPARE TRANSACTION 'held{i}'"))
        self.wait(prepared_slots=4)
        self.error(self.query(peer, "BEGIN; PREPARE TRANSACTION 'held0'"), "42710")
        self.success(self.query(peer, "ROLLBACK"))
        self.error(self.query(peer, "BEGIN; PREPARE TRANSACTION 'full'"), "53200")
        self.success(self.query(peer, "ROLLBACK"))
        self.control.gate(False)
        self.wait(prepared_slots=4, foreground_idle=True)
        for i in range(4):
            self.pg.sql(f"ROLLBACK PREPARED 'held{i}'")
        self.wait(prepared_slots=0)

    def test_crash_rebuild_recovers_wal_or_disk_but_not_unlogged_reserved_slot(self):
        for checkpoint in (False, True):
            with self.subTest(checkpoint=checkpoint):
                self.control.gate(True)
                peer = self.peer(admin=True)
                self.success(self.query(peer, "BEGIN; INSERT INTO prepared_data VALUES(42); PREPARE TRANSACTION 'saved'"))
                if checkpoint:
                    self.pg.sql("CHECKPOINT")
                self.assertEqual(bool(list((self.pg.data / "pg_twophase").iterdir())), checkpoint)
                self.arm(peer, "arm_wait")
                body = b"BEGIN; INSERT INTO prepared_data VALUES(99); PREPARE TRANSACTION 'unlogged'\0"
                peer.connection.sendall(b"Q" + struct.pack("!I", len(body) + 4) + body)
                self.pg.wait_sql("SELECT dsg_prepared_control('state')", "1")
                old = self.wait(prepared_slots=2)
                # PID来自本次真实BackendKeyData；此时只杀自己的持TwoPhaseStateLock backend。
                os.kill(peer.backend_pid, signal.SIGKILL)
                peer.close()
                deadline = time.monotonic() + 15
                while time.monotonic() < deadline:
                    try:
                        self.control.observer = self.peer(admin=True)
                        fact = self.fact()
                        if fact["pg_boot_id"] != old["pg_boot_id"]:
                            break
                    except (OSError, RuntimeError, StopIteration):
                        time.sleep(0.02)
                else:
                    self.fail("prepared crash did not rebuild native PG boot")
                self.control.boot = self.pg.sql("SELECT dsg_boot()").stdout.strip()
                self.control.revision = 0
                self.wait(prepared_slots=1, acknowledged=True, public_connections_blocked=True)
                self.assertEqual(self.pg.sql("SELECT gid FROM pg_prepared_xacts").stdout.strip(), "saved")
                self.pg.sql("COMMIT PREPARED 'saved'")
                self.pg.sql("SELECT dsg_prepared_control('detach')")
                self.wait(prepared_slots=0)
        self.assertEqual(self.pg.sql("SELECT count(*) FROM prepared_data WHERE i=42").stdout.strip(), "2")
        self.assertEqual(self.pg.sql("SELECT count(*) FROM prepared_data WHERE i=99").stdout.strip(), "0")
