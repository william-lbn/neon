"""真实Postmaster fork前登记，不在尚无PGPROC的child安装共享清理回调。

实际socket、原始startup/SSL/GSS协议、真实超时/退出/reap/崩溃恢复。日志先确认
child真的收到连接，防止把尚未accept误作在途或以等待时长猜测。仅私有临时PG。
"""
import json
import os
import re
import signal
import socket
import struct
import subprocess
import time
import unittest

import test_dsg_work as work
import test_dsg_postgres as lifecycle


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要显式真实PG及配套库")
class DsgPrestartLifecycle(unittest.TestCase):
    # 只复用行为函数，不继承旧test方法，避免把原8项再次收集执行。
    gate = work.DsgWorkLifecycle.gate
    peer = work.DsgWorkLifecycle.peer
    snapshot = work.DsgWorkLifecycle.snapshot
    wait = work.DsgWorkLifecycle.wait
    send = staticmethod(work.DsgWorkLifecycle.send)

    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        self.pg.config("log_connections=on\nlog_line_prefix='[%p] '\n"
                       "max_connections=6\nsuperuser_reserved_connections=1\n"
                       "autovacuum_max_workers=1\nmax_wal_senders=0\nmax_worker_processes=0\n"
                       "authentication_timeout='2s'\n")
        if self._testMethodName == "test_capacity_refuses_before_fork_without_untracked_deadends":
            self.pg.config("authentication_timeout='20s'\n")
        self.boot = self.pg.prepare_admission_fixture()
        self.pg.sql(f"CREATE FUNCTION dsg_work() RETURNS text AS '{self.pg.quote(self.pg.library)}', "
                    "'test_dsg_work_snapshot' LANGUAGE C VOLATILE PARALLEL UNSAFE")
        self.revision = 0

    def raw(self, payload=b""):
        offset = self.pg.log.stat().st_size
        peer = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.addCleanup(peer.close)
        peer.settimeout(5)
        peer.connect(str(self.pg.socket / ".s.PGSQL.55432"))
        if payload:
            peer.sendall(payload)
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            match = re.search(rb"\[(\d+)\].*connection received:", self.pg.log.read_bytes()[offset:])
            if match:
                return peer, int(match.group(1))
            time.sleep(0.005)
        self.fail("没有本次真实fork连接接收日志")

    def test_raw_empty_pending_is_not_idle(self):
        peer, pid = self.raw()
        self.assertEqual(self.pg.sql(f"SELECT count(*) FROM pg_stat_activity WHERE pid={pid}").stdout.strip(), "0")
        fact = self.snapshot()
        self.assertEqual(fact["startup_clients"], 1)
        self.assertEqual(fact["client_processes"], 1)
        self.assertFalse(fact["foreground_idle"])
        peer.close()
        self.wait(startup_clients=0, client_processes=0, foreground_idle=True)

    def test_partial_ssl_gss_startup_counted_before_pgproc(self):
        for payload, reply in [(b"\0\0", None),
                               (struct.pack("!II", 8, 80877103), b"N"),
                               (struct.pack("!II", 8, 80877104), b"N")]:
            with self.subTest(payload=payload.hex()):
                peer, pid = self.raw(payload)
                if reply:
                    self.assertEqual(peer.recv(1), reply)
                # 已真正fork/收到连接，但没有startup包/PGPROC，不能用pg_stat_activity猜。
                self.assertEqual(self.pg.sql(f"SELECT count(*) FROM pg_stat_activity WHERE pid={pid}").stdout.strip(), "0")
                self.wait(startup_clients=1, client_processes=1, foreground_idle=False)
                peer.close()
                self.wait(startup_clients=0, client_processes=0, foreground_idle=True)

    def test_startup_handoff_has_no_duplicate_and_early_timeout_is_reaped(self):
        peer, _ = self.raw()
        self.wait(startup_clients=1, foreground_idle=False)
        # 同一socket从无启动包继续到真正认证/Ready；不能先清旧计数再漏登记PGPROC。
        body = struct.pack("!I", 196608) + b"user\0dsg_app\0database\0postgres\0\0"
        peer.sendall(struct.pack("!I", len(body) + 4) + body)
        wire = object.__new__(lifecycle.PgWireConnection)
        wire.connection = peer
        self.assertTrue(any(k == b"Z" for k, _ in wire.receive_ready()))
        self.wait(client_processes=1, startup_clients=0, idle_clients=1)
        peer.close()
        self.wait(client_processes=0, foreground_idle=True)
        timed, _ = self.raw()
        self.wait(startup_clients=1, foreground_idle=False)
        # 官方启动包定时器真实触发_exit；只有Postmaster reap后才归零，不发送取消代造。
        self.wait(client_processes=0, startup_clients=0, foreground_idle=True)
        timed.close()

    def test_invalid_cancel_and_malformed_startup_clear_only_after_child_exit(self):
        peer, _ = self.raw()
        self.wait(startup_clients=1)
        peer.sendall(struct.pack("!IIII", 16, 80877102, 0, 0))
        self.assertEqual(peer.recv(1), b"")
        self.wait(client_processes=0, startup_clients=0)
        peer.close()
        peer, _ = self.raw(b"\0\0")
        self.wait(startup_clients=1)
        peer.sendall(b"\0\x03")  # 长度3小于官方最小值，真实早期协议失败。
        self.wait(client_processes=0, startup_clients=0, foreground_idle=True)
        peer.close()

    def test_capacity_refuses_before_fork_without_untracked_deadends(self):
        observer = self.peer(os.environ.get("USER", "williamlee"))
        def observed():
            messages = observer.exchange([(b"Q", b"SELECT dsg_work()\0")])
            rows = [b for k, b in messages if k == b"D"]
            self.assertEqual(len(rows), 1)
            return json.loads(rows[0][6:])
        alive = []
        refused = False
        for _ in range(24):  # 夹具MaxLive=16，硬上限只约束测试，不能无限fork等结果。
            peer = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.addCleanup(peer.close)
            peer.settimeout(0.02)
            peer.connect(str(self.pg.socket / ".s.PGSQL.55432"))
            try:
                data = peer.recv(1)
            except socket.timeout:
                alive.append(peer)
                continue
            self.assertEqual(data, b"")
            peer.close()
            refused = True
            break
        self.assertTrue(refused, "受管容量拒绝仍fork了未登记dead-end子进程")
        fact = observed()
        self.assertGreater(fact["startup_clients"], 0)
        self.assertLessEqual(fact["startup_clients"], 16)
        self.assertFalse(fact["foreground_idle"])
        for peer in alive:
            peer.close()
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            fact = observed()
            if fact["client_processes"] == 0:
                break
            time.sleep(0.01)
        self.assertEqual(fact["client_processes"], 0)
        self.assertTrue(fact["foreground_idle"])

    def test_status_lock_owner_crash_does_not_block_postmaster_prestart_cleanup(self):
        self.pg.sql(f"CREATE FUNCTION dsg_work_stop() RETURNS boolean AS '{self.pg.quote(self.pg.library)}', "
                    "'test_dsg_work_stop_with_status_lock' LANGUAGE C VOLATILE PARALLEL UNSAFE")
        active = self.peer(os.environ.get("USER", "williamlee"))
        raw, _ = self.raw()
        self.wait(startup_clients=1)
        old = int(self.boot) & ((1 << 64) - 1)
        self.send(active, b"Q", b"SELECT dsg_work_stop()\0")
        killed = False
        postmaster = int((self.pg.data / "postmaster.pid").read_text().splitlines()[0])
        def stopped_owned():
            state = subprocess.run(["ps", "-o", "ppid=,stat=", "-p", str(active.backend_pid)],
                                   capture_output=True, text=True, check=False).stdout.split()
            return len(state) == 2 and int(state[0]) == postmaster and state[1].startswith("T")
        try:
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and not stopped_owned():
                time.sleep(0.005)
            self.assertTrue(stopped_owned(), "故障夹具没有在真实status_lock内停止自身")
            # 后端此时确实持着status_lock，Postmaster仍必须处理SIGCHLD/旧启动槽。
            os.kill(active.backend_pid, signal.SIGKILL)
            killed = True
        finally:
            if not killed and stopped_owned():
                os.kill(active.backend_pid, signal.SIGKILL)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            r = self.pg.sql("SELECT dsg_boot()", check=False)
            if r.returncode == 0 and (int(r.stdout.strip()) & ((1 << 64)-1)) != old:
                self.boot = r.stdout.strip()
                break
            time.sleep(0.02)
        else:
            self.fail("Postmaster被backend共享锁拖住，未从真实崩溃恢复")
        self.wait(client_processes=0, startup_clients=0, frontend_executes=0,
                  admission_open=False, foreground_idle=True)
        self.assertEqual(raw.recv(1), b"")
        raw.close()
