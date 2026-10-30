"""真实Postmaster门禁/ack：socket身份来自内核，禁止新的业务fork，保留私有管理。

只创建自己的临时PG及随机loopback端口。既有会话保留，先前RAW工作仍登记；
暂停本次Postmaster确认pending不能假ack，恢复/崩溃后原许可不可跨boot沿用。
"""
import json
import os
import signal
import socket
import struct
import subprocess
import time
import unittest

import test_dsg_postgres as lifecycle


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要显式真实PG和配套库")
class DsgConnectionGateLifecycle(unittest.TestCase):
    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        self.public = self.pg.socket
        self.private = self.pg.data / "dsg_socket"
        self.private.mkdir(mode=0o700)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            self.port = reservation.getsockname()[1]
        self.pg.port = self.port
        self.pg.socket = self.private
        self.pg.config(f"port={self.port}\nlisten_addresses='127.0.0.1'\n"
                       f"unix_socket_directories='{self.public},{self.private}'\n"
                       "unix_socket_permissions=0700\ndynamic_shared_buffers_require_connection_gate=on\n"
                       "log_connections=on\nlog_line_prefix='[%p] '\n")
        # 专项夹具在首次出生前固定postmaster-only参数；旧用例默认不变。
        if extra := getattr(self, "extra_postgres_config", ""):
            self.pg.config(extra)
        self.boot = self.pg.prepare_admission_fixture()
        self.pg.sql(f"CREATE FUNCTION dsg_connections() RETURNS text AS '{self.pg.quote(self.pg.library)}', "
                    "'test_dsg_connection_snapshot' LANGUAGE C VOLATILE PARALLEL UNSAFE")
        self.revision = 0
        self.observer = self.peer(self.private, lifecycle.native_database_user())
        self.wait(acknowledged=True, requested_open=False, public_connections_blocked=True)

    def peer(self, directory=None, user="dsg_app"):
        peer = lifecycle.PgWireConnection(directory or self.public, user, self.port)
        self.addCleanup(peer.close)
        return peer

    @staticmethod
    def row(messages):
        rows = [b for k, b in messages if k == b"D"]
        if len(rows) != 1:
            raise AssertionError("expected one native row")
        return rows[0][6:].decode()

    def gate(self, opened):
        self.revision += 1
        # 使用已有私有连接，Postmaster被停止时也不得悄悄新建观察连接。
        body = f"SELECT dsg_admission({self.boot},{self.revision},1,{str(opened).lower()})\0".encode()
        messages = self.observer.exchange([(b"Q", body)])
        self.assertFalse(any(k == b"E" for k, _ in messages), messages)

    def fact(self):
        fact = json.loads(self.row(self.observer.exchange([(b"Q", b"SELECT dsg_connections()\0")])))
        self.assertTrue(fact["enabled"])
        self.assertEqual(fact["requested_open"], fact["admission_open"])
        self.assertEqual(fact["acknowledged"], fact["requested_serial"] > 0 and
                         fact["requested_serial"] == fact["applied_serial"] and
                         fact["requested_open"] == fact["applied_open"])
        self.assertEqual(fact["public_connections_blocked"], fact["acknowledged"] and not fact["requested_open"])
        return fact

    def wait(self, **expected):
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            fact = self.fact()
            if all(fact[k] == v for k, v in expected.items()):
                return fact
            time.sleep(0.01)
        self.fail(f"native gate did not reach {expected}: {fact}")

    def denied(self, *, tcp=False, forged=False):
        peer = socket.socket(socket.AF_INET if tcp else socket.AF_UNIX, socket.SOCK_STREAM)
        self.addCleanup(peer.close)
        peer.settimeout(2)
        peer.connect(("127.0.0.1", self.port) if tcp else str(self.public / f".s.PGSQL.{self.port}"))
        try:
            if forged:
                body = struct.pack("!I", 196608) + b"user\0cloud_admin\0database\0postgres\0\0"
                peer.sendall(struct.pack("!I", len(body) + 4) + body)
            self.assertEqual(peer.recv(1), b"", "closed public socket was not rejected before startup")
        except (ConnectionResetError, BrokenPipeError):
            pass  # fork前丢弃未读startup时内核也可返回RST，不是假PG认证应答。
        peer.close()

    def test_closed_boot_refuses_public_unix_tcp_and_forged_management_name(self):
        before = self.wait(client_processes=0, startup_clients=0)
        self.assertTrue(before["private_management_ready"])
        offset = self.pg.log.stat().st_size
        for tcp, forged in [(False, False), (True, False), (False, True), (True, True)]:
            self.denied(tcp=tcp, forged=forged)
        after = self.wait(client_processes=0, startup_clients=0)
        self.assertEqual(before["requested_serial"], after["requested_serial"])
        self.assertNotIn("connection received:", self.pg.log.read_text()[offset:])
        # 门禁未根据startup用户名放行，也未影响私有管理读取。
        self.assertEqual(self.row(self.observer.exchange([(b"Q", b"SELECT 42\0")])), "42")

    def test_existing_connection_and_prior_raw_survive_close_then_reopen(self):
        self.gate(True)
        opened = self.wait(acknowledged=True, requested_open=True)
        app = self.peer()
        self.assertEqual(self.row(app.exchange([(b"Q", b"SELECT 42\0")])), "42")
        raw = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.addCleanup(raw.close)
        raw.settimeout(3)
        raw.connect(str(self.public / f".s.PGSQL.{self.port}"))
        raw.sendall(struct.pack("!II", 8, 80877104))
        self.assertEqual(raw.recv(1), b"N")
        self.wait(startup_clients=1)
        self.gate(False)
        closed = self.wait(acknowledged=True, public_connections_blocked=True, startup_clients=1)
        self.assertGreater(closed["requested_serial"], opened["requested_serial"])
        self.assertFalse(closed["foreground_idle"])
        self.denied()
        messages = app.exchange([(b"Q", b"SELECT 42\0")])
        self.assertTrue(any(k == b"E" and b"C55000\0" in b for k, b in messages))
        raw.close()
        self.wait(startup_clients=0)
        self.gate(True)
        self.wait(acknowledged=True, requested_open=True)
        self.assertEqual(self.row(app.exchange([(b"Q", b"SELECT 42\0")])), "42")

    def test_stopped_postmaster_cannot_ack_and_queued_accept_uses_new_gate(self):
        self.gate(True)
        self.wait(acknowledged=True, requested_open=True)
        pid = int((self.pg.data / "postmaster.pid").read_text().splitlines()[0])
        os.kill(pid, signal.SIGSTOP)
        queued = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.addCleanup(queued.close)
        queued.settimeout(3)
        try:
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                state = subprocess.run(["ps", "-o", "stat=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
                if state.startswith("T"):
                    break
                time.sleep(0.005)
            self.assertTrue(state.startswith("T"))
            self.gate(False)
            fact = self.fact()
            self.assertFalse(fact["acknowledged"])
            self.assertFalse(fact["public_connections_blocked"])
            self.assertGreater(fact["requested_serial"], fact["applied_serial"])
            queued.connect(str(self.public / f".s.PGSQL.{self.port}"))
        finally:
            os.kill(pid, signal.SIGCONT)
        self.wait(acknowledged=True, public_connections_blocked=True)
        self.assertEqual(queued.recv(1), b"")
        queued.close()
        self.wait(startup_clients=0)

    def test_private_permission_drift_blocks_new_private_and_does_not_claim_ready(self):
        self.private.chmod(0o755)
        try:
            self.assertFalse(self.fact()["private_management_ready"])
            peer = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.addCleanup(peer.close)
            peer.settimeout(2)
            peer.connect(str(self.private / f".s.PGSQL.{self.port}"))
            self.assertEqual(peer.recv(1), b"")
            peer.close()
        finally:
            self.private.chmod(0o700)
        self.assertTrue(self.fact()["private_management_ready"])
        restored = self.peer(self.private, lifecycle.native_database_user())
        self.assertEqual(self.row(restored.exchange([(b"Q", b"SELECT 42\0")])), "42")

    def test_crash_rebuild_does_not_inherit_old_open_ack(self):
        self.gate(True)
        old = self.wait(acknowledged=True, requested_open=True)
        app = self.peer()
        body = b"SELECT pg_sleep(30)\0"
        app.connection.sendall(b"Q" + struct.pack("!I", len(body)+4) + body)
        # 目标PID来自本次连接BackendKeyData，先确认真实查询在执行再杀死。
        self.pg.wait_sql(f"SELECT count(*) FROM pg_stat_activity WHERE pid={app.backend_pid} AND wait_event='PgSleep'", "1")
        os.kill(app.backend_pid, signal.SIGKILL)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            try:
                observer = lifecycle.PgWireConnection(self.private, lifecycle.native_database_user(), self.port)
                self.addCleanup(observer.close)
                self.observer = observer
                fact = self.fact()
                if fact["pg_boot_id"] != old["pg_boot_id"]:
                    break
            except (OSError, RuntimeError, StopIteration):
                time.sleep(0.02)
        else:
            self.fail("new private management connection did not observe rebuilt PG boot")
        self.wait(acknowledged=True, public_connections_blocked=True, requested_serial=1,
                  requested_open=False, startup_clients=0)
        self.denied()


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要显式真实PG和配套库")
class DsgConnectionConfiguration(unittest.TestCase):
    def test_enabled_gate_rejects_missing_or_symlink_private_directory(self):
        for symlink in [False, True]:
            with self.subTest(symlink=symlink):
                pg = lifecycle.DsgPostgresLifecycle("runTest")
                self.addCleanup(pg.doCleanups)
                pg.setUp()
                if symlink:
                    (pg.data / "dsg_socket").symlink_to(pg.socket, target_is_directory=True)
                pg.config("dynamic_shared_buffers_require_connection_gate=on\n"
                          "dynamic_shared_buffers_require_admission=on\n"
                          "dynamic_shared_buffers_startup_budget='64MB'\nunix_socket_permissions=0700\n")
                self.assertNotEqual(pg.start(check=False).returncode, 0)
                self.assertIn("owned private directory", pg.log.read_text())
