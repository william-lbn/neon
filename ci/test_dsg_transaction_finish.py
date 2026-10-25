"""真实关门事务结束：原事务/回调持续登记，不把小型结束命令当排空许可。

仅用自有临时PG、固定私有管理socket和真实wire/原生同点观察。复用夹具而不继承
旧测试，避免把同一功能重复执行。Linux物理/完整Quiesce与可信预算另行完成。
"""
import os
import socket
import struct
import unittest

import test_dsg_admission_frames as frames
import test_dsg_connection_gate as connections
import test_dsg_postgres as lifecycle


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要显式准确PG及配套测试库")
class DsgTransactionFinishLifecycle(unittest.TestCase):
    allocated = frames.DsgAdmissionFrameLifecycle.allocated
    stream = staticmethod(frames.DsgAdmissionFrameLifecycle.stream)

    def setUp(self):
        self.control = connections.DsgConnectionGateLifecycle("runTest")
        self.addCleanup(self.control.doCleanups)
        self.control.setUp()
        self.pg = self.control.pg
        self.gate = self.control.gate
        self.wait = self.control.wait
        self.pg.sql("CREATE TABLE finish_data(i int); GRANT INSERT,SELECT ON finish_data TO dsg_app; "
                    "GRANT CREATE ON SCHEMA public TO dsg_app")
        self.gate(True)
        self.peer = self.control.peer()

    def success(self, messages, status=b"I"):
        self.assertFalse(any(k == b"E" for k, _ in messages), messages)
        self.assertEqual(messages[-1], (b"Z", status))

    def error(self, messages, code="55000", status=b"E"):
        errors = [b for k, b in messages if k == b"E"]
        self.assertEqual(len(errors), 1, messages)
        self.assertIn(b"C" + code.encode() + b"\0", errors[0])
        self.assertFalse(any(k in (b"D", b"1", b"2") for k, _ in messages), messages)
        self.assertEqual(messages[-1], (b"Z", status))

    def query(self, body):
        return self.peer.exchange([(b"Q", body)])

    def begin(self, value=1):
        self.gate(True)
        self.success(self.query(f"BEGIN; INSERT INTO finish_data VALUES({value}); SAVEPOINT held\0".encode()), b"T")
        self.gate(False)
        self.wait(acknowledged=True, public_connections_blocked=True,
                  retained_transactions=1, foreground_idle=False)

    def ended(self):
        self.wait(retained_transactions=0, frontend_executes=0, foreground_idle=True)

    @staticmethod
    def parse(query, name=b""):
        return name + b"\0" + query + b"\0\0\0"

    @staticmethod
    def bind(statement=b"", portal=b"", binary=False):
        formats = b"\0\1\0\1" if binary else b"\0\0"
        return portal + b"\0" + statement + b"\0" + formats + b"\0\0" + formats

    def test_simple_end_forms_preserve_commit_or_rollback_and_connection(self):
        committed = 0
        for query, commits in [(b"COMMIT\0", True), (b"END WORK AND NO CHAIN;\0", True),
                               (b" /* bounded */ cOmMiT TRANSACTION; \0", True),
                               (b"ROLLBACK\0", False), (b"ABORT TRANSACTION;\0", False),
                               (b"ROLLBACK WORK AND NO CHAIN;\0", False)]:
            with self.subTest(query=query):
                self.begin()
                self.success(self.query(query))
                self.ended()
                committed += commits
                self.assertEqual(int(self.pg.sql("SELECT count(*) FROM finish_data").stdout), committed)
                # 已结束不等于重新准入；同一应用连接只能在显式重开后开始新工作。
                self.error(self.query(b"SELECT 42\0"), status=b"I")
                self.gate(True)
                self.success(self.query(b"SELECT 42\0"))

    def test_failed_transactions_can_end_while_closed(self):
        for query in (b"COMMIT\0", b"ROLLBACK\0"):
            with self.subTest(query=query):
                self.gate(True)
                self.error(self.query(b"BEGIN; INSERT INTO finish_data VALUES(1); SELECT 1/0\0"), "22012")
                self.gate(False)
                self.wait(retained_transactions=1, foreground_idle=False)
                self.success(self.query(query))
                self.ended()
                self.assertEqual(self.pg.sql("SELECT count(*) FROM finish_data").stdout.strip(), "0")

    def test_extended_unnamed_finish_describe_and_binary_formats(self):
        self.begin()
        messages = self.peer.exchange([
            (b"P", self.parse(b"COMMIT")), (b"D", b"S\0"),
            (b"B", self.bind(binary=True)), (b"D", b"P\0"),
            (b"E", b"\0\0\0\0\0"), (b"S", b"")])
        self.success(messages)
        self.assertTrue(any(k == b"1" for k, _ in messages), messages)
        self.assertTrue(any(k == b"2" for k, _ in messages), messages)
        self.assertIn((b"C", b"COMMIT\0"), messages)
        self.ended()
        self.assertEqual(self.pg.sql("SELECT count(*) FROM finish_data").stdout.strip(), "1")

    def test_preexisting_named_statement_and_portal_can_finish(self):
        for prepared_portal in (False, True):
            with self.subTest(prepared_portal=prepared_portal):
                name = b"finish_portal_stmt" if prepared_portal else b"finish_stmt"
                self.gate(True)
                self.success(self.peer.exchange([(b"P", self.parse(b"ROLLBACK", name)), (b"S", b"")]))
                self.success(self.query(b"BEGIN; INSERT INTO finish_data VALUES(1)\0"), b"T")
                if prepared_portal:
                    self.success(self.peer.exchange([(b"B", self.bind(name, b"existing_portal")), (b"S", b"")]), b"T")
                self.gate(False)
                self.wait(retained_transactions=1, foreground_idle=False)
                if prepared_portal:
                    messages = [(b"D", b"Pexisting_portal\0"), (b"E", b"existing_portal\0\0\0\0\0")]
                else:
                    messages = [(b"D", b"S" + name + b"\0"), (b"B", self.bind(name)), (b"E", b"\0\0\0\0\0")]
                self.success(self.peer.exchange(messages + [(b"S", b"")]))
                self.ended()
                self.assertEqual(self.pg.sql("SELECT count(*) FROM finish_data").stdout.strip(), "0")

    def test_refused_sql_cannot_escape_or_leak_to_next_message(self):
        for body in (b"COMMIT; CREATE TABLE finish_escape(i int)\0", b"COMMIT AND CHAIN\0",
                     b"ROLLBACK AND CHAIN\0", b"ROLLBACK TO SAVEPOINT held\0",
                     b"PREPARE TRANSACTION 'finish_escape'\0", b"COMMIT PREPARED 'finish_escape'\0",
                     b"COMMIT\0; CREATE TABLE finish_escape(i int)\0", b"\0",
                     "COMMIT /*非ASCII*/\0".encode()):
            with self.subTest(body=body):
                self.begin()
                self.error(self.query(body))
                self.wait(retained_transactions=1, foreground_idle=False)
                self.success(self.query(b"ROLLBACK\0"))
                self.ended()
                self.assertEqual(self.pg.sql("SELECT to_regclass('finish_escape') IS NULL").stdout.strip(), "t")
                self.assertEqual(self.pg.sql("SELECT count(*) FROM finish_data").stdout.strip(), "0")

    def test_extended_rejects_new_named_objects_and_nonfinish_portal(self):
        self.success(self.peer.exchange([(b"P", self.parse(b"COMMIT", b"existing_commit")), (b"S", b"")]))
        for kind in ("named_parse", "named_bind", "ordinary_portal", "parameter_bind"):
            with self.subTest(kind=kind):
                self.begin()
                if kind == "ordinary_portal":
                    self.gate(True)
                    self.success(self.peer.exchange([(b"P", self.parse(b"SELECT 42")),
                                                     (b"B", self.bind(portal=b"ordinary")), (b"S", b"")]), b"T")
                    self.gate(False)
                    command = (b"E", b"ordinary\0\0\0\0\0")
                elif kind == "named_parse":
                    command = (b"P", self.parse(b"COMMIT", b"new_named"))
                elif kind == "named_bind":
                    command = (b"B", self.bind(b"existing_commit", b"new_named"))
                else:
                    command = (b"B", b"\0existing_commit\0\0\0\0\1\0\0\0\0\0\0")
                # ERROR之后直到Sync，夹带的Q也必须忽略，不能因其是ROLLBACK而逃逸。
                messages = self.peer.exchange([command, (b"Q", b"ROLLBACK\0"), (b"S", b"")])
                self.error(messages)
                self.wait(retained_transactions=1, foreground_idle=False)
                self.success(self.query(b"ABORT\0"))
                self.ended()
        self.assertEqual(self.pg.sql("SELECT count(*) FROM finish_data").stdout.strip(), "0")

    def test_oversize_discard_remains_bounded_and_reopen_cannot_execute_original(self):
        self.begin()
        baseline = self.allocated(self.peer.backend_pid)
        prefix = b"COMMIT; CREATE TABLE finish_escape(i int); /*"
        tail = b"*/\0"
        size = 16 * 1024 * 1024
        self.peer.connection.sendall(b"Q" + struct.pack("!I", size + 4) + prefix)
        self.stream(self.peer, size // 2 - len(prefix), b" ")
        self.wait(bounded_inputs=1, retained_transactions=1, foreground_idle=False)
        during = self.allocated(self.peer.backend_pid)
        print(f"retained large Q declared={size} backend_allocated_before={baseline} during={during}", flush=True)
        self.assertLessEqual(during, baseline + 512 * 1024)
        self.gate(True)
        self.stream(self.peer, size - size // 2 - len(tail), b" ")
        self.peer.connection.sendall(tail)
        self.error(self.peer.receive_ready())
        self.assertEqual(self.pg.sql("SELECT to_regclass('finish_escape') IS NULL").stdout.strip(), "t")
        self.gate(False)
        self.success(self.query(b"ROLLBACK\0"))
        self.ended()
        self.assertEqual(self.pg.sql("SELECT count(*) FROM finish_data").stdout.strip(), "0")

    def test_deferred_trigger_is_inflight_until_actual_commit_or_cancel(self):
        self.pg.sql("CREATE FUNCTION finish_wait() RETURNS trigger LANGUAGE plpgsql AS $$BEGIN "
                    "PERFORM pg_advisory_xact_lock(1945012); PERFORM repeat('x',1048576); RETURN NEW; END$$; "
                    "CREATE CONSTRAINT TRIGGER finish_wait AFTER INSERT ON finish_data "
                    "DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION finish_wait()")
        blocker = self.control.peer(self.control.private, os.environ.get("USER", "williamlee"))
        for cancel in (False, True):
            with self.subTest(cancel=cancel):
                self.success(blocker.exchange([(b"Q", b"SELECT pg_advisory_lock(1945012)\0")]))
                self.begin()
                self.peer.connection.sendall(b"Q" + struct.pack("!I", 11) + b"COMMIT\0")
                self.pg.wait_sql(f"SELECT wait_event FROM pg_stat_activity WHERE pid={self.peer.backend_pid}", "advisory")
                self.wait(frontend_executes=1, retained_transactions=1, foreground_idle=False,
                          acknowledged=True, public_connections_blocked=True)
                if cancel:
                    self.assertEqual(self.pg.sql(f"SELECT pg_cancel_backend({self.peer.backend_pid})").stdout.strip(), "t")
                    self.error(self.peer.receive_ready(), "57014", b"I")
                    self.success(blocker.exchange([(b"Q", b"SELECT pg_advisory_unlock(1945012)\0")]))
                else:
                    self.success(blocker.exchange([(b"Q", b"SELECT pg_advisory_unlock(1945012)\0")]))
                    self.success(self.peer.receive_ready())
                self.ended()
                self.assertEqual(self.pg.sql("SELECT count(*) FROM finish_data").stdout.strip(), "1")
        self.gate(True)
        self.success(self.query(b"SELECT 42\0"))

    def test_truncated_cleanup_cannot_fake_ready_or_hide_retained_work(self):
        self.begin()
        self.peer.connection.sendall(b"Q" + struct.pack("!I", 13) + b"ROLL")
        self.wait(bounded_inputs=1, retained_transactions=1, foreground_idle=False)
        self.peer.connection.shutdown(socket.SHUT_WR)
        received = bytearray()
        try:
            while chunk := self.peer.connection.recv(4096):
                received.extend(chunk)
        except ConnectionResetError:
            pass
        self.assertNotIn(b"Z\0\0\0\x05", received)
        self.peer.close()
        self.wait(client_processes=0, retained_transactions=0, foreground_idle=True)
        self.assertEqual(self.pg.sql("SELECT count(*) FROM finish_data").stdout.strip(), "0")
