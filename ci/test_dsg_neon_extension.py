"""真实Neon扩展/Neon PG的本机集成，不替代Pageserver/VM或物理回收认证。

使用已安装、同一版本的neon/neon_test_utils和实际test_dsg_region；独立临时
集群测试文件发布、进程取消/死亡、共享LSN缓存、真实Buffer扩缩及过期提示。
不会连接已有数据库，也不运行镜像或部署。
"""

import os
import signal
import socket
import struct
import threading
import time
import unittest
from pathlib import Path

import test_dsg_postgres as pgtests

_EXT_ENV = ("NEON_TEST_NEON_LIBRARY", "NEON_TEST_UTILS_LIBRARY")
_FULL_ENV = (*pgtests._REQUIRED_ENV, *_EXT_ENV)
if any(os.environ.get(name) for name in _EXT_ENV) and not all(
    os.environ.get(name) for name in _FULL_ENV
):
    raise RuntimeError("真实Neon扩展测试环境不完整：" + ", ".join(_FULL_ENV))


class SlruProtocolFixture:
    """只提供SLRU请求的本地PG/COPY协议端点，保留生产客户端所有通信/解析行为。

    它没有Pageserver存储/WAL语义；测试结论只覆盖客户端新hook及发布器接线。
    不允许借这个fixture宣称真实存储端到端或物理回收能力已认证。
    """

    def __init__(self, nblocks):
        self.nblocks = nblocks
        self.requests = []
        self.errors = []
        self.stopping = False
        self.connection = None
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.bind(("127.0.0.1", 0))
        self.port = self.listener.getsockname()[1]
        self.listener.listen(1)
        self.listener.settimeout(0.2)
        self.thread = threading.Thread(target=self.serve, name="slru-protocol-fixture", daemon=True)
        self.thread.start()

    @staticmethod
    def read_exact(connection, size):
        result = bytearray()
        while len(result) < size:
            chunk = connection.recv(size - len(result))
            if not chunk:
                raise EOFError("协议连接结束")
            result.extend(chunk)
        return bytes(result)

    def message(self):
        kind = self.read_exact(self.connection, 1)
        size = struct.unpack("!I", self.read_exact(self.connection, 4))[0]
        if not 4 <= size <= 1024 * 1024:
            raise RuntimeError("fixture收到非法PG帧长")
        return kind, self.read_exact(self.connection, size - 4)

    def send(self, kind, body):
        self.connection.sendall(kind + struct.pack("!I", len(body) + 4) + body)

    def serve(self):
        try:
            while not self.stopping:
                try:
                    self.connection, _ = self.listener.accept()
                    break
                except TimeoutError:
                    continue
            if self.stopping:
                return
            self.connection.settimeout(10)
            size = struct.unpack("!I", self.read_exact(self.connection, 4))[0]
            if not 8 <= size <= 65536:
                raise RuntimeError("fixture收到非法startup长度")
            startup = self.read_exact(self.connection, size - 4)
            if struct.unpack("!I", startup[:4])[0] != 196608:
                raise RuntimeError("fixture仅接受明确禁用SSL/GSS的PGv3连接")
            self.send(b"R", struct.pack("!I", 0))
            self.send(b"S", b"server_version\00017.11\0")
            self.send(b"K", struct.pack("!II", 1, 0))
            self.send(b"Z", b"I")
            kind, query = self.message()
            if kind != b"Q" or not query.startswith(b"pagestream_v3 "):
                raise RuntimeError(f"fixture收到非pagestream查询：{kind} {query}")
            self.send(b"W", b"\0\0\0")
            kind, request = self.message()
            if kind != b"d" or len(request) != 30:
                raise RuntimeError("fixture收到非SLRU COPY请求")
            tag, reqid, lsn, since, slru_kind, segno = struct.unpack("!BQQQBI", request)
            if (tag, slru_kind, segno) != (4, 0, 10):
                raise RuntimeError(f"fixture收到错误SLRU身份：{tag}, {slru_kind}, {segno}")
            self.requests.append((reqid, lsn, since, slru_kind, segno))
            response = struct.pack(
                "!BQQQBIi", 105, reqid, lsn, since, slru_kind, segno, self.nblocks
            )
            if 0 < self.nblocks <= 32:
                response += bytes([77]) * self.nblocks * 8192
            self.send(b"d", response)
            while self.connection.recv(4096):
                pass
        except EOFError:
            pass
        except Exception as error:
            if not self.stopping:
                self.errors.append(repr(error))

    def close(self):
        self.stopping = True
        if self.connection is not None:
            try:
                self.connection.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            self.connection.close()
        self.listener.close()
        self.thread.join(timeout=2)
        if self.thread.is_alive() or self.errors:
            raise AssertionError(f"协议fixture未正常退出：{self.errors}")


@unittest.skipUnless(
    all(os.environ.get(name) for name in _FULL_ENV), "真实Neon扩展专项需显式提供全部PG及扩展库"
)
class DsgNeonExtension(unittest.TestCase):
    def setUp(self):
        # 复用临时集群辅助行为，不继承45项测试，避免把重复执行冒充新覆盖。
        self.pg = pgtests.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        neon = self.pg.quote(Path(os.environ[_EXT_ENV[0]]).resolve())
        self.utils = self.pg.quote(Path(os.environ[_EXT_ENV[1]]).resolve())
        self.pg.config(
            f"shared_preload_libraries='{self.pg.quote(self.pg.library)},{neon}'\n"
            "neon.last_written_lsn_cache_size=1024\n"
        )
        self.target = self.pg.data / "pg_xact/000A"
        self.expected = self.pg.prepare_shrink_fixture()
        self.pg.sql(
            "CREATE EXTENSION injection_points;"
            f"CREATE FUNCTION neon_test_publish(text,integer,integer,boolean) RETURNS void AS "
            f"'{self.utils}','test_neon_slru_publish' LANGUAGE C STRICT;"
            f"CREATE FUNCTION neon_test_unpack(bytea) RETURNS integer AS "
            f"'{self.utils}','test_neon_slru_unpack' LANGUAGE C STRICT;"
            f"CREATE FUNCTION neon_test_lsn(regclass,integer,pg_lsn) RETURNS pg_lsn AS "
            f"'{self.utils}','test_neon_lwlsn' LANGUAGE C;"
            f"CREATE FUNCTION neon_test_download(text,integer) RETURNS boolean AS "
            f"'{self.utils}','test_neon_slru_download' LANGUAGE C STRICT;"
            f"CREATE FUNCTION clear_buffer_cache() RETURNS void AS "
            f"'{self.utils}','clear_buffer_cache' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_exclusive_available() RETURNS boolean AS "
            f"'{self.pg.quote(self.pg.library)}','test_dsg_exclusive_available' LANGUAGE C;"
        )

    def temporaries(self):
        return list((self.pg.data / "pg_xact").glob("*.neon-slru.*"))

    def assert_payload(self, fill, blocks=32):
        self.assertEqual(self.target.read_bytes(), bytes([fill]) * (blocks * 8192))
        self.assertEqual(self.temporaries(), [])

    def publish(self, fill=42, blocks=32, short=False, *, check=True):
        return self.pg.sql(
            f"SELECT neon_test_publish('pg_xact/000A',{blocks},{fill},{str(short).lower()})",
            check=check,
        )

    def pause_publisher(self, hook, fill=42):
        self.pg.sql(f"SELECT injection_points_attach('{hook}','wait')")
        process = self.pg.background_sql(
            f"SELECT neon_test_publish('pg_xact/000A',32,{fill},false)", "neon-slru-publisher"
        )
        self.pg.wait_sql(
            "SELECT count(*)=1 FROM pg_stat_activity "
            f"WHERE application_name='neon-slru-publisher' AND wait_event='{hook}'",
            "t",
        )
        self.assertIsNone(process.poll())
        self.assertEqual(len(self.temporaries()), 1)
        return process

    def finish(self, process, *, success):
        _, errors = process.communicate(timeout=10)
        if success:
            self.assertEqual(process.returncode, 0, errors + self.pg.log.read_text()[-12000:])
        else:
            self.assertNotEqual(process.returncode, 0, errors)
        return errors

    def interrupt_publisher(self, terminate=False):
        function = "pg_terminate_backend" if terminate else "pg_cancel_backend"
        self.assertEqual(
            self.pg.sql(
                f"SELECT {function}(pid) FROM pg_stat_activity "
                "WHERE application_name='neon-slru-publisher'"
            ).stdout.strip(),
            "t",
        )

    def test_publish_complete_partial_and_existing_winner(self):
        self.publish(blocks=3)
        self.assert_payload(42, 3)
        self.publish(fill=99)
        self.assert_payload(42, 3)

    def test_publish_rejects_invalid_path_and_size_before_file_creation(self):
        for path, blocks in (
            ("pg_xact/../000A", 32),
            ("pg_xact_bad/000A", 32),
            ("pg_xact/ABC", 32),
            ("postgresql.conf", 32),
            ("pg_xact/000A", -1),
            ("pg_xact/000A", 0),
            ("pg_xact/000A", 33),
            ("pg_xact/000A", 2147483647),
        ):
            with self.subTest(path=path, blocks=blocks):
                result = self.pg.sql(
                    f"SELECT neon_test_publish('{path}',{blocks},42,false)", check=False
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("invalid Neon SLRU segment publication", result.stderr)
                self.assertFalse(self.target.exists())
                self.assertEqual(self.temporaries(), [])

    def test_two_publishers_never_overwrite_newer_existing_file(self):
        hook = "neon-slru-before-publish"
        first = self.pause_publisher(hook, fill=42)
        self.assertFalse(self.target.exists(), "完整文件发布前不能暴露半文件")
        self.pg.sql(f"SELECT injection_points_detach('{hook}')")
        self.publish(fill=99)
        self.assertEqual(self.target.read_bytes(), bytes([99]) * (32 * 8192))
        self.pg.sql(f"SELECT injection_points_wakeup('{hook}')")
        self.finish(first, success=True)
        self.assert_payload(99)

    def test_real_short_write_cleans_temp_and_restores_same_backend_limit(self):
        wire = pgtests.PgWireConnection(
            self.pg.socket, self.pg.sql("SELECT current_user").stdout.strip()
        )
        self.addCleanup(wire.close)
        messages = wire.exchange([(b"Q", b"SELECT neon_test_publish('pg_xact/000A',32,42,true)\0")])
        self.assertTrue(
            any(kind == b"E" and b"could not write Neon SLRU" in body for kind, body in messages)
        )
        self.assertFalse(self.target.exists())
        self.assertEqual(self.temporaries(), [])
        messages = wire.exchange(
            [(b"Q", b"SELECT neon_test_publish('pg_xact/000A',32,99,false)\0")]
        )
        self.assertFalse(any(kind == b"E" for kind, _ in messages), messages)
        self.assert_payload(99)

    def test_error_before_publish_keeps_target_absent(self):
        self.pg.sql("SELECT injection_points_attach('neon-slru-before-publish','error')")
        failed = self.publish(check=False)
        self.assertNotEqual(failed.returncode, 0)
        self.assertFalse(self.target.exists())
        self.assertEqual(self.temporaries(), [])

    def test_error_after_publish_keeps_complete_target(self):
        self.pg.sql("SELECT injection_points_attach('neon-slru-after-publish','error')")
        failed = self.publish(check=False)
        self.assertNotEqual(failed.returncode, 0)
        self.assert_payload(42)

    def test_cancel_before_publish_cleans_temp(self):
        process = self.pause_publisher("neon-slru-before-publish")
        self.interrupt_publisher()
        self.assertIn("canceling statement", self.finish(process, success=False))
        self.assertFalse(self.target.exists())
        self.assertEqual(self.temporaries(), [])

    def test_fatal_before_publish_cleans_temp(self):
        process = self.pause_publisher("neon-slru-before-publish")
        self.interrupt_publisher(terminate=True)
        self.finish(process, success=False)
        self.assertFalse(self.target.exists())
        self.assertEqual(self.temporaries(), [])

    def test_cancel_after_publish_keeps_complete_target(self):
        process = self.pause_publisher("neon-slru-after-publish")
        self.interrupt_publisher()
        self.finish(process, success=False)
        self.assert_payload(42)

    def test_new_boot_cleans_only_recognized_stale_temporaries(self):
        self.pg.command("pg_ctl", "-D", self.pg.data, "-m", "fast", "-w", "stop")
        self.pg.running = False
        directory = self.pg.data / "pg_xact"
        stale = directory / "000A.neon-slru.123.456"
        retained = directory / "000A.neon-slru.123.keep"
        stale.write_bytes(b"incomplete")
        retained.write_bytes(b"unrecognized")
        self.pg.start()
        self.assertFalse(stale.exists())
        self.assertEqual(retained.read_bytes(), b"unrecognized")
        retained.unlink()

    def test_sigkill_download_is_cleaned_after_shared_memory_rebuild(self):
        boot = self.pg.sql("SELECT dsg_boot()").stdout
        process = self.pause_publisher("neon-slru-before-publish")
        pid = int(
            self.pg.sql(
                "SELECT pid FROM pg_stat_activity WHERE application_name='neon-slru-publisher'"
            ).stdout
        )
        os.kill(pid, signal.SIGKILL)
        self.finish(process, success=False)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            result = self.pg.sql("SELECT dsg_boot()", check=False)
            if result.returncode == 0 and result.stdout != boot:
                break
            time.sleep(0.05)
        else:
            self.fail("新boot恢复未完成\n" + self.pg.log.read_text()[-12000:])
        self.assertFalse(self.target.exists())
        self.assertEqual(self.temporaries(), [])
        self.assertEqual(self.pg.shrink_data_checksum(), self.expected)

    def test_slru_parser_accepts_exact_payload_and_zero(self):
        for count in (0, 1, 32):
            body = struct.pack("!BQQQBIi", 105, 1, 2, 2, 0, 10, count)
            payload = body + bytes([42]) * count * 8192
            self.assertEqual(
                self.pg.sql(
                    f"SELECT neon_test_unpack(decode('{payload.hex()}','hex'))"
                ).stdout.strip(),
                str(count),
            )

    def download_from_fixture(self, blocks):
        fixture = SlruProtocolFixture(blocks)
        self.addCleanup(fixture.close)
        conninfo = (
            f"host=127.0.0.1 port={fixture.port} user=neon_fixture dbname=postgres "
            "sslmode=disable gssencmode=disable connect_timeout=3"
        )
        self.pg.sql(f"ALTER SYSTEM SET neon.pageserver_connstring='{conninfo}'")
        self.pg.sql("SELECT pg_reload_conf()")
        self.pg.wait_sql("SHOW neon.pageserver_connstring", conninfo)
        result = self.pg.sql("SELECT neon_test_download('pg_xact/000A',10)", check=False)
        self.assertEqual(len(fixture.requests), 1, fixture.errors)
        request = fixture.requests[0]
        self.assertGreaterEqual(request[1], request[2])
        return result

    def test_download_hook_uses_real_network_parser_and_publishes_file(self):
        result = self.download_from_fixture(3)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "t")
        self.assert_payload(77, 3)

    def test_download_hook_zero_pages_keeps_file_absent(self):
        result = self.download_from_fixture(0)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "f")
        self.assertFalse(self.target.exists())
        self.assertEqual(self.temporaries(), [])

    def test_download_hook_rejects_oversized_network_response(self):
        result = self.download_from_fixture(33)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("invalid Neon SLRU response block count", result.stderr)
        self.assertFalse(self.target.exists())
        self.assertEqual(self.temporaries(), [])

    def test_download_hook_declines_other_slru_without_network(self):
        # 这些核心SLRU不由Neon下载，包括15位长文件名；没有配置网络端点，
        # 必须在尝试通信和Neon文件名校验前返回false。
        for path in ("pg_subtrans/00000000000000A", "pg_commit_ts/000A", "pg_xact/sub/000A"):
            self.assertEqual(
                self.pg.sql(f"SELECT neon_test_download('{path}',10)").stdout.strip(), "f"
            )
        self.assertFalse(self.target.exists())

    def test_download_hook_rejects_mismatched_identity_before_network(self):
        boot = self.pg.sql("SELECT dsg_boot()").stdout
        result = self.pg.sql("SELECT neon_test_download('pg_xact/000B',10)", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("filename does not match segment number", result.stderr)
        self.assertEqual(self.pg.sql("SELECT dsg_boot()").stdout, boot)
        self.assertEqual(self.temporaries(), [])

    def test_slru_parser_rejects_negative_oversized_and_truncated_messages(self):
        boot = self.pg.sql("SELECT dsg_boot()").stdout
        for count in (-1, 33, 2147483647, 1):
            body = struct.pack("!BQQQBIi", 105, 1, 2, 2, 0, 10, count)
            result = self.pg.sql(
                f"SELECT neon_test_unpack(decode('{body.hex()}','hex'))", check=False
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(self.pg.sql("SELECT dsg_boot()").stdout, boot)
        self.assertEqual(self.pg.shrink_data_checksum(), self.expected)

    def test_lsn_cache_is_shared_monotonic_and_survives_lru_eviction(self):
        first = self.pg.sql(
            "SELECT pg_logical_emit_message(false,'neon-test','first')"
        ).stdout.strip()
        second = self.pg.sql(
            "SELECT pg_logical_emit_message(false,'neon-test','second')"
        ).stdout.strip()
        self.assertEqual(
            self.pg.sql(f"SELECT neon_test_lsn('dsg_shrink_data',0,'{second}')").stdout.strip(),
            second,
        )
        # 每次psql是独立Backend；先更新高LSN再写低LSN不能让缓存倒退。
        self.assertEqual(
            self.pg.sql(f"SELECT neon_test_lsn('dsg_shrink_data',0,'{first}')").stdout.strip(),
            second,
        )
        workers = [
            self.pg.background_sql(
                f"SELECT neon_test_lsn('dsg_shrink_data',i,'{second}') "
                f"FROM generate_series({start},{start + 599}) i",
                f"neon-lsn-{start}",
            )
            for start in (1, 601, 1201, 1801)
        ]
        for worker in workers:
            self.finish(worker, success=True)
        self.assertEqual(
            self.pg.sql(
                f"SELECT neon_test_lsn('dsg_shrink_data',0,NULL)>='{second}'::pg_lsn"
            ).stdout.strip(),
            "t",
        )

    def test_clear_cache_covers_grown_tail_and_real_data_can_reload(self):
        self.assertTrue(self.pg.sql("SELECT dsg_tail_block('dsg_shrink_data',256)").stdout.strip())
        self.pg.sql("SELECT clear_buffer_cache()")
        self.assertFalse(self.pg.sql("SELECT dsg_tail_block('dsg_shrink_data',256)").stdout.strip())
        self.assertEqual(self.pg.shrink_data_checksum(), self.expected)
        self.assertIn("committed high=256", self.pg.shrink_call().stdout)

    def test_clear_cache_stale_hint_does_not_hold_guard_across_shrink(self):
        hook = "neon-clear-buffer-cache-after-peek"
        self.pg.sql(f"SELECT injection_points_attach('{hook}','wait')")
        clearing = self.pg.background_sql("SELECT clear_buffer_cache()", "neon-cache-clear")
        self.pg.wait_sql(
            "SELECT count(*)=1 FROM pg_stat_activity "
            f"WHERE application_name='neon-cache-clear' AND wait_event='{hook}'",
            "t",
        )
        self.assertEqual(self.pg.sql("SELECT dsg_exclusive_available()").stdout.strip(), "t")
        self.assertEqual(self.pg.resize_state()["high"], 2048)
        self.assertIn("committed high=256", self.pg.shrink_call().stdout)
        self.pg.sql(
            f"SELECT injection_points_detach('{hook}'); SELECT injection_points_wakeup('{hook}')"
        )
        self.finish(clearing, success=True)
        self.assertEqual(self.pg.shrink_data_checksum(), self.expected)

    def test_internal_storage_test_bridges_recheck_privilege(self):
        self.pg.sql("CREATE ROLE neon_test_ordinary")
        for query in (
            "SELECT neon_test_publish('pg_xact/000A',1,42,false)",
            "SELECT neon_test_unpack(decode('69','hex'))",
            "SELECT neon_test_lsn('dsg_shrink_data',0,NULL)",
            "SELECT neon_test_download('pg_xact/000A',10)",
        ):
            result = self.pg.sql(f"SET ROLE neon_test_ordinary; {query}", check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("require superuser", result.stderr)
        self.assertFalse(self.target.exists())


if __name__ == "__main__":
    unittest.main()
