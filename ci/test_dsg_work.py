"""真实PG进程/前台在途登记：不用pg_stat_activity推断空闲或伪造回执。

只观察独立临时集群。关闭准入后仍在读的已接纳帧、真实COPY、取消和退出均走
PG实际协议路径；长期事务独立于BOUNDED_INPUT，观察命令不能隐藏自身显式事务。
这些测试不声称认证前连接、后台/DSM完整quiesce或Linux物理回收已覆盖。
"""
import json
import os
import signal
import socket
import struct
import time
import unittest

import test_dsg_postgres as lifecycle


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要显式真实PG及配套库")
class DsgWorkLifecycle(unittest.TestCase):
    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        self.boot = self.pg.prepare_admission_fixture()
        self.pg.sql(f"CREATE FUNCTION dsg_work() RETURNS text AS '{self.pg.quote(self.pg.library)}', "
                    "'test_dsg_work_snapshot' LANGUAGE C VOLATILE PARALLEL UNSAFE")
        self.revision = 0

    def gate(self, opened):
        self.revision += 1
        self.pg.sql(f"SELECT dsg_admission({self.boot},{self.revision},1,{str(opened).lower()})")

    def peer(self, user="dsg_app"):
        peer = lifecycle.PgWireConnection(self.pg.socket, user)
        self.addCleanup(peer.close)
        return peer

    def snapshot(self):
        fact = json.loads(self.pg.sql("SELECT dsg_work()").stdout)
        # 旧测试桥用signed bigint传输boot位模式；生产wire保留完整uint64。
        self.assertEqual(str(int(self.boot) & ((1 << 64) - 1)), fact["pg_boot_id"])
        phases = ("startup_clients", "idle_clients", "frontend_reads", "frontend_executes", "bounded_inputs")
        self.assertEqual(sum(fact[k] for k in phases), fact["client_processes"])
        self.assertLessEqual(fact["retained_transactions"], fact["client_processes"])
        expected = not fact["admission_open"] and not any(
            fact[k] for k in ("startup_clients", "frontend_reads", "frontend_executes", "retained_transactions"))
        self.assertEqual(expected, fact["foreground_idle"])
        return fact

    def wait(self, **expected):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            fact = self.snapshot()
            if all(fact[k] == v for k, v in expected.items()):
                return fact
            time.sleep(0.01)
        self.fail(f"真实工作登记未达到{expected}: {fact}\n{self.pg.log.read_text()[-6000:]}")

    @staticmethod
    def send(peer, kind, body):
        peer.connection.sendall(kind + struct.pack("!I", len(body) + 4) + body)

    def error(self, messages, code):
        errors = [body for kind, body in messages if kind == b"E"]
        self.assertEqual(len(errors), 1, messages)
        self.assertIn(b"C" + code.encode() + b"\0", errors[0])

    def test_accepted_read_survives_close_and_bounded_reject_is_separate(self):
        self.gate(True)
        peer = self.peer()
        self.wait(client_processes=1, idle_clients=1)
        body = b"SELECT 42 /*" + b" " * 8192 + b"*/\0"
        peer.connection.sendall(b"Q" + struct.pack("!I", len(body) + 4) + body[:64])
        self.wait(frontend_reads=1, bounded_inputs=0)
        self.gate(False)
        self.wait(frontend_reads=1, foreground_idle=False)
        peer.connection.sendall(body[64:])
        self.error(peer.receive_ready(), "55000")
        self.wait(frontend_reads=0, frontend_executes=0, foreground_idle=True)
        peer.connection.sendall(b"Q" + struct.pack("!I", len(body) + 4) + body[:64])
        self.wait(bounded_inputs=1, frontend_reads=0, foreground_idle=True)
        peer.close()  # 半帧EOF不能假造已同步，只能退出并最终注销真实槽。
        self.wait(client_processes=0, bounded_inputs=0, foreground_idle=True)

    def test_retained_transaction_is_not_hidden_by_rejected_frame_or_observer(self):
        self.gate(True)
        peer = self.peer()
        self.assertFalse(any(k == b"E" for k, _ in peer.exchange([(b"Q", b"BEGIN\0")])))
        self.wait(retained_transactions=1, idle_clients=1)
        self.gate(False)
        body = b"SELECT 42 /*" + b" " * 8192 + b"*/\0"
        peer.connection.sendall(b"Q" + struct.pack("!I", len(body) + 4) + body[:64])
        self.wait(bounded_inputs=1, retained_transactions=1, foreground_idle=False)
        peer.connection.sendall(body[64:])
        self.error(peer.receive_ready(), "55000")
        self.wait(idle_clients=1, retained_transactions=1, foreground_idle=False)
        # 旧事务必须在关门期间协作结束；先重开会掩盖真实排空的活性缺陷。
        peer.exchange([(b"Q", b"ROLLBACK\0")])
        self.wait(retained_transactions=0, foreground_idle=True)
        # 显式事务可与观察同在一个Query；不能仅凭上一轮slot的retained=false排除。
        admin = self.peer(os.environ.get("USER", "williamlee"))
        received = admin.exchange([(b"Q", b"BEGIN; SELECT dsg_work()\0")])
        rows = [b for k, b in received if k == b"D"]
        self.assertEqual(len(rows), 1)
        fact = json.loads(rows[0][6:].decode())
        self.assertFalse(fact["observer_excluded"])
        self.assertFalse(fact["foreground_idle"])
        self.assertGreaterEqual(fact["frontend_executes"], 1)
        admin.exchange([(b"Q", b"ROLLBACK\0")])
        self.wait(retained_transactions=0, foreground_idle=True, observer_excluded=True)

    def test_real_copy_is_executing_until_complete_and_error_cleanup(self):
        self.pg.sql("CREATE TABLE work_copy(i int); GRANT INSERT ON work_copy TO dsg_app")
        self.gate(True)
        peer = self.peer()
        self.send(peer, b"Q", b"COPY work_copy FROM STDIN\0")
        self.assertEqual(peer.receive_exact(1), b"G")
        n = struct.unpack("!I", peer.receive_exact(4))[0]
        peer.receive_exact(n - 4)
        self.gate(False)
        self.wait(frontend_executes=1, foreground_idle=False)
        self.send(peer, b"d", b"1\n2\n")
        self.send(peer, b"c", b"")
        received = peer.receive_ready()
        self.assertFalse(any(k == b"E" for k, _ in received), received)
        self.wait(frontend_executes=0, retained_transactions=0, foreground_idle=True)
        self.assertEqual(self.pg.sql("SELECT count(*),sum(i) FROM work_copy").stdout.strip(), "2|3")
        self.gate(True)
        self.send(peer, b"Q", b"COPY work_copy FROM STDIN\0")
        self.assertEqual(peer.receive_exact(1), b"G")
        n = struct.unpack("!I", peer.receive_exact(4))[0]
        peer.receive_exact(n - 4)
        self.gate(False)
        self.wait(frontend_executes=1, foreground_idle=False)
        self.send(peer, b"d", b"bad-int\n")
        self.error(peer.receive_ready(), "22P02")
        self.wait(frontend_executes=0, foreground_idle=True)

    def test_cancel_exit_and_slot_reuse_clear_only_real_finished_work(self):
        self.gate(True)
        peer = self.peer()
        self.send(peer, b"Q", b"SELECT pg_sleep(30)\0")
        self.wait(frontend_executes=1)
        self.gate(False)
        self.assertEqual(self.pg.sql(f"SELECT pg_cancel_backend({peer.backend_pid})").stdout.strip(), "t")
        self.error(peer.receive_ready(), "57014")
        fact = self.wait(frontend_executes=0, foreground_idle=True)
        self.assertGreaterEqual(fact["auxiliary_processes"], 2)
        self.assertGreaterEqual(fact["background_processes"], 1)
        self.gate(True)
        self.send(peer, b"Q", b"SELECT pg_sleep(30)\0")
        self.wait(frontend_executes=1)
        self.gate(False)
        self.assertEqual(self.pg.sql(f"SELECT pg_terminate_backend({peer.backend_pid})").stdout.strip(), "t")
        self.wait(client_processes=0, foreground_idle=True)
        peer.close()
        for _ in range(6):
            fresh = self.peer()
            self.wait(client_processes=1, idle_clients=1)
            fresh.close()
            self.wait(client_processes=0, foreground_idle=True)

    def test_authentication_wait_is_startup_until_authentication_exit(self):
        self.pg.sql("ALTER ROLE dsg_app PASSWORD 'private-fixture-password'")
        (self.pg.data / "pg_hba.conf").write_text(
            "local all dsg_app scram-sha-256\nlocal all all trust\n")
        self.pg.sql("SELECT pg_reload_conf()")
        # pg_hba_file_rules只检查磁盘文件可解析；真实SASL挑战才证明reload已生效。
        self.pg.wait_sql("SELECT count(*) FROM pg_hba_file_rules WHERE auth_method='scram-sha-256'", "1")
        deadline = time.monotonic() + 5
        while True:
            connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.addCleanup(connection.close)
            connection.settimeout(5)
            connection.connect(str(self.pg.socket / ".s.PGSQL.55432"))
            body = struct.pack("!I", 196608) + b"user\0dsg_app\0database\0postgres\0\0"
            connection.sendall(struct.pack("!I", len(body) + 4) + body)
            # 只读固定认证头，不提供口令或进入SQL；旧trust接受仅说明reload尚未生效。
            head = bytearray()
            while len(head) < 9:
                chunk = connection.recv(9 - len(head))
                self.assertTrue(chunk)
                head.extend(chunk)
            self.assertEqual(head[0:1], b"R")
            method = struct.unpack("!I", head[5:9])[0]
            if method == 10:
                break
            self.assertEqual(method, 0)
            connection.close()
            if time.monotonic() >= deadline:
                self.fail("真实认证路径没有加载新的SCRAM规则")
            time.sleep(0.01)
        self.wait(startup_clients=1, frontend_executes=0, foreground_idle=False)
        connection.close()
        self.wait(client_processes=0, startup_clients=0, foreground_idle=True)

    def test_crash_rebuilds_registry_with_new_closed_boot(self):
        self.gate(True)
        peer = self.peer()
        self.send(peer, b"Q", b"SELECT pg_sleep(30)\0")
        self.wait(frontend_executes=1)
        # PID来自该连接的BackendKeyData，且执行状态已确认；不猜PID或终止陌生进程。
        old_boot = int(self.boot) & ((1 << 64) - 1)
        os.kill(peer.backend_pid, signal.SIGKILL)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            attempt = self.pg.sql("SELECT dsg_boot()", check=False)
            if attempt.returncode == 0:
                boot = attempt.stdout.strip()
                if (int(boot) & ((1 << 64) - 1)) != old_boot:
                    self.boot = boot
                    break
            time.sleep(0.02)
        else:
            self.fail("真实Postmaster没有完成共享内存重建")
        self.wait(client_processes=0, frontend_executes=0, startup_clients=0,
                  retained_transactions=0, admission_open=False, foreground_idle=True)

    def test_unsynced_extended_protocol_retains_implicit_transaction(self):
        self.gate(True)
        peer = self.peer()
        self.send(peer, b"P", b"\0SELECT 42\0" + struct.pack("!H", 0))
        self.send(peer, b"B", b"\0\0" + struct.pack("!HHH", 0, 0, 0))
        self.send(peer, b"H", b"")  # Flush真实Parse/Bind应答，不发送结束隐式事务的Sync。
        for expected in (b"1", b"2"):
            kind = peer.receive_exact(1)
            size = struct.unpack("!I", peer.receive_exact(4))[0]
            peer.receive_exact(size - 4)
            self.assertEqual(kind, expected)
        self.gate(False)
        self.wait(idle_clients=1, retained_transactions=1, foreground_idle=False)
        peer.exchange([(b"S", b"")])
        self.wait(retained_transactions=0, foreground_idle=True)

    def test_describe_cannot_revalidate_plan_after_admission_closed(self):
        self.gate(True)
        peer = self.peer()
        parse = b"prepared_work\0SELECT 42\0" + struct.pack("!H", 0)
        received = peer.exchange([(b"P", parse), (b"S", b"")])
        self.assertTrue(any(k == b"1" for k, _ in received), received)
        self.gate(False)
        received = peer.exchange([(b"D", b"Sprepared_work\0"), (b"S", b"")])
        self.error(received, "55000")
        self.assertFalse(any(k in (b"T", b"t", b"n") for k, _ in received), received)
        self.wait(frontend_reads=0, frontend_executes=0, retained_transactions=0, foreground_idle=True)
