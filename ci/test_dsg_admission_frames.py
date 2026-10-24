"""真实PG关闭准入时的帧读取上界；不是完整quiesce或VM物理验收。

复用私有临时集群和真实协议连接。内存断言来自目标backend实际的
pg_log_backend_memory_contexts，在大帧尚未结束时读取，不能用ERROR后的清理
或RSS采样假装消息体从未分配。每个测试只创建/清理自己拥有的PG。
"""
import contextlib
import re
import socket
import struct
import time
import unittest

import test_dsg_postgres as lifecycle


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要显式真实PG及配套测试库")
class DsgAdmissionFrameLifecycle(unittest.TestCase):
    large_types = (b"Q", b"P", b"B", b"F", b"d")

    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        self.pg.config("log_line_prefix='[%p] '\n")
        self.boot = self.pg.prepare_admission_fixture()
        self.revision = 0

    def gate(self, opened):
        self.revision += 1
        self.pg.sql(f"SELECT dsg_admission({self.boot},{self.revision},1,{str(opened).lower()})")

    def peer(self):
        peer = lifecycle.PgWireConnection(self.pg.socket, "dsg_app")
        self.addCleanup(peer.close)
        return peer, peer.backend_pid

    def allocated(self, pid):
        offset = self.pg.log.stat().st_size
        self.assertEqual(self.pg.sql(f"SELECT pg_log_backend_memory_contexts({pid})").stdout.strip(), "t")
        pattern = re.compile(rf"\[{pid}\].*Grand total: (\d+) bytes")
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            match = pattern.search(self.pg.log.read_bytes()[offset:].decode("utf8", errors="replace"))
            if match:
                return int(match.group(1))
            time.sleep(0.01)
        self.fail("目标backend没有按请求输出实际memory context统计")

    @staticmethod
    def stream(peer, size, byte=b"x"):
        block = byte * 8192
        while size:
            part = block[:min(size, len(block))]
            peer.connection.sendall(part)
            size -= len(part)

    def error(self, messages):
        errors = [body for kind, body in messages if kind == b"E"]
        self.assertEqual(len(errors), 1, messages)
        self.assertIn(b"C55000\0", errors[0])
        self.assertFalse(any(kind in (b"D", b"1", b"2") for kind, _ in messages), messages)

    def test_closed_large_query_parse_bind_fastpath_and_copy_are_bounded(self):
        for kind in self.large_types:
            with self.subTest(kind=kind.decode()), contextlib.closing(self.peer()[0]) as peer:
                pid = peer.backend_pid
                baseline = self.allocated(pid)
                size = 16 * 1024 * 1024
                peer.connection.sendall(kind + struct.pack("!I", size + 4))
                # 发过socket缓冲容量再停在半帧，确认服务端确实已读取正文。
                self.stream(peer, size // 2)
                during = self.allocated(pid)
                print(f"frame={kind.decode()} declared={size} buffered={size // 2} "
                      f"backend_allocated_before={baseline} during={during}", flush=True)
                self.assertLessEqual(during, baseline + 512 * 1024,
                                     "关闭期间仍按声明长度扩张了目标backend消息缓冲")
                self.stream(peer, size - size // 2)
                if kind in (b"P", b"B", b"d"):
                    peer.connection.sendall(b"S" + struct.pack("!I", 4))
                received = peer.receive_ready()
                if kind == b"d":
                    self.assertFalse(any(k in (b"E", b"D") for k, _ in received), received)
                else:
                    self.error(received)
                self.gate(True)
                reused = peer.exchange([(b"Q", b"SELECT 42\0")])
                self.assertTrue(any(k == b"D" and b.endswith(b"42") for k, b in reused), reused)
                self.gate(False)

    def test_reopen_during_discard_does_not_execute_discarded_original(self):
        # 让DDL确实有权限成功，避免把原语句的权限拒绝误判为“没有执行”。
        self.pg.sql("GRANT CREATE ON SCHEMA public TO dsg_app")
        peer, _ = self.peer()
        prefix, tail = b"CREATE TABLE frame_must_not_execute(i int); /*", b"*/\0"
        size = 16 * 1024 * 1024
        peer.connection.sendall(b"Q" + struct.pack("!I", size + 4) + prefix)
        self.stream(peer, size // 2 - len(prefix), b" ")
        self.gate(True)
        self.stream(peer, size - size // 2 - len(tail), b" ")
        peer.connection.sendall(tail)
        self.error(peer.receive_ready())
        self.assertEqual(self.pg.sql("SELECT to_regclass('frame_must_not_execute') IS NULL").stdout.strip(), "t")
        self.assertTrue(any(k == b"D" for k, _ in peer.exchange([(b"Q", b"SELECT 42\0")])))
        accepted = peer.exchange([(b"Q", b"CREATE TABLE accepted_after_frame(i int)\0")])
        self.assertFalse(any(k == b"E" for k, _ in accepted), accepted)
        self.assertEqual(self.pg.sql("SELECT to_regclass('accepted_after_frame') IS NOT NULL").stdout.strip(), "t")

    def test_ignored_pipeline_stays_bounded_after_reopen_until_sync(self):
        self.pg.sql("GRANT CREATE ON SCHEMA public TO dsg_app")
        peer, pid = self.peer()
        parse = b"\0SELECT 42\0" + struct.pack("!H", 0)
        peer.connection.sendall(b"P" + struct.pack("!I", len(parse) + 4) + parse)
        kind, length = peer.receive_exact(1), struct.unpack("!I", peer.receive_exact(4))[0]
        self.assertEqual(kind, b"E")
        self.assertIn(b"C55000\0", peer.receive_exact(length - 4))
        self.gate(True)
        baseline, size = self.allocated(pid), 16 * 1024 * 1024
        peer.connection.sendall(b"B" + struct.pack("!I", size + 4))
        self.stream(peer, size // 2)
        self.assertLessEqual(self.allocated(pid), baseline + 512 * 1024,
                             "ignore-till-Sync重开后仍分配了不会执行的大帧")
        self.stream(peer, size - size // 2)
        ignored = b"CREATE TABLE ignored_until_sync(i int)\0"
        peer.connection.sendall(b"Q" + struct.pack("!I", len(ignored) + 4) + ignored)
        peer.connection.sendall(b"S" + struct.pack("!I", 4))
        messages = peer.receive_ready()
        self.assertFalse(any(k in (b"D", b"E") for k, _ in messages), messages)
        self.assertEqual(self.pg.sql("SELECT to_regclass('ignored_until_sync') IS NULL").stdout.strip(), "t")
        self.assertTrue(any(k == b"D" for k, _ in peer.exchange([(b"Q", b"SELECT 42\0")])))
        accepted = peer.exchange([(b"Q", b"CREATE TABLE accepted_after_sync(i int)\0")])
        self.assertFalse(any(k == b"E" for k, _ in accepted), accepted)
        self.assertEqual(self.pg.sql("SELECT to_regclass('accepted_after_sync') IS NOT NULL").stdout.strip(), "t")

    def test_invalid_lengths_and_truncated_frames_disconnect_without_fake_ready(self):
        for name, frame in [("short-length", b"Q" + struct.pack("!I", 3)),
                            ("oversize-small", b"E" + struct.pack("!I", 0x7fffffff)),
                            ("oversize-large", b"Q" + struct.pack("!I", 0x7fffffff)),
                            ("partial-length", b"Q\0"),
                            ("partial-body", b"Q" + struct.pack("!I", 100) + b"xx")]:
            with self.subTest(name=name), contextlib.closing(self.peer()[0]) as peer:
                peer.connection.sendall(frame)
                peer.connection.shutdown(socket.SHUT_WR)
                received = bytearray()
                try:
                    while chunk := peer.connection.recv(4096):
                        received.extend(chunk)
                except ConnectionResetError:
                    pass
                self.assertNotIn(b"Z\0\0\0\x05", received,
                                 "非法或半帧不能冒充已经恢复同步的ReadyForQuery")
        self.assertEqual(self.pg.sql("SELECT 1").stdout.strip(), "1")

    def test_real_copy_failure_leftovers_remain_silent_while_admission_closed(self):
        self.pg.sql("CREATE TABLE copy_cleanup(i int); GRANT INSERT ON copy_cleanup TO dsg_app")
        self.gate(True)
        peer, _ = self.peer()

        def frame():
            kind = peer.receive_exact(1)
            length = struct.unpack("!I", peer.receive_exact(4))[0]
            self.assertTrue(4 <= length <= 16 * 1024 * 1024)
            return kind, peer.receive_exact(length - 4)

        query = b"COPY copy_cleanup FROM STDIN\0"
        peer.connection.sendall(b"Q" + struct.pack("!I", len(query) + 4) + query)
        kind, _ = frame()
        self.assertEqual(kind, b"G", "必须先进入实际COPY IN，不能仅模拟残留帧")
        bad = b"not-an-integer\n"
        peer.connection.sendall(b"d" + struct.pack("!I", len(bad) + 4) + bad)
        kind, body = frame()
        self.assertEqual(kind, b"E")
        self.assertIn(b"C22P02\0", body, "必须是实际数据转换失败，不是权限/准入拒绝")
        peer.receive_ready()
        self.gate(False)
        # 先收完原COPY错误和Ready，再在关闭期间发残留数据和清理帧，确保
        # 这里读到的是本次Sync结果，不能把此前排队的Ready冒充清理成功。
        received = peer.exchange([(b"d", b"23\n"), (b"c", b""), (b"f", b"cancelled\0"), (b"S", b"")])
        self.assertFalse(any(k in (b"E", b"D") for k, _ in received), received)
        self.error(peer.exchange([(b"Q", b"SELECT 42\0")]))
        self.gate(True)
        self.assertTrue(any(k == b"D" for k, _ in peer.exchange([(b"Q", b"SELECT 42\0")])))
        # 真实在途COPY不经过SocketBackend主循环：关闭后仍允许原工作
        # 正常完成，不能把有界丢弃错误应用到它而静默丢数据；后续SQL仍被关门。
        peer.connection.sendall(b"Q" + struct.pack("!I", len(query) + 4) + query)
        self.assertEqual(frame()[0], b"G")
        self.gate(False)
        rows = b"1\n2\n"
        peer.connection.sendall(b"d" + struct.pack("!I", len(rows) + 4) + rows)
        peer.connection.sendall(b"c" + struct.pack("!I", 4))
        completed = peer.receive_ready()
        self.assertFalse(any(k == b"E" for k, _ in completed), completed)
        self.assertTrue(any(k == b"C" and body == b"COPY 2\0" for k, body in completed), completed)
        self.assertEqual(self.pg.sql("SELECT count(*),sum(i) FROM copy_cleanup").stdout.strip(), "2|3")
        self.error(peer.exchange([(b"Q", b"SELECT 42\0")]))
