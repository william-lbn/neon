"""在真实 PG17 中检查 DSGA 启动、共享内存重建和数据恢复。

NEON_TEST_PG_BIN 指向待测二进制；NEON_TEST_DSG_LIBRARY 指向 PG 测试模块
test_dsg_region，NEON_TEST_RMGR_LIBRARY 指向配套 neon_rmgr 的完整库路径。
仅创建独立临时集群，不连接已有开发数据库。
这里验证 Mac 可执行的生命周期，不声称验证 Linux 物理回收。
"""

import json
import os
import signal
import socket
import struct
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

_REQUIRED_ENV = ("NEON_TEST_PG_BIN", "NEON_TEST_DSG_LIBRARY", "NEON_TEST_RMGR_LIBRARY")
_CONFIGURED_ENV = [name for name in _REQUIRED_ENV if os.environ.get(name)]
if _CONFIGURED_ENV and len(_CONFIGURED_ENV) != len(_REQUIRED_ENV):
    raise RuntimeError("生命周期测试环境不完整，必须同时提供 " + ", ".join(_REQUIRED_ENV))


class PgWireConnection:
    """直接发送真实简单/扩展/FastPath消息，验证门禁没有漏掉协议入口。

    仅连接测试临时集群的trust Unix socket；不实现认证或连接产品数据库。
    错误后等到ReadyForQuery，再在同一连接继续，验证没有把SQL拒绝变成断连。
    """

    def __init__(self, directory, user):
        self.connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.connection.settimeout(10)
        self.connection.connect(str(directory / ".s.PGSQL.55432"))
        body = struct.pack("!I", 196608) + f"user\0{user}\0database\0postgres\0\0".encode()
        self.connection.sendall(struct.pack("!I", len(body) + 4) + body)
        self.receive_ready()

    def receive_exact(self, length):
        chunks = bytearray()
        while len(chunks) < length:
            chunk = self.connection.recv(length - len(chunks))
            if not chunk:
                raise RuntimeError("PG协议测试连接提前断开")
            chunks.extend(chunk)
        return bytes(chunks)

    def receive_ready(self):
        messages = []
        while True:
            kind = self.receive_exact(1)
            length = struct.unpack("!I", self.receive_exact(4))[0]
            if not 4 <= length <= 16 * 1024 * 1024:
                raise RuntimeError("PG协议测试收到非法长度")
            body = self.receive_exact(length - 4)
            messages.append((kind, body))
            if kind == b"Z":
                return messages

    def exchange(self, messages):
        for kind, body in messages:
            self.connection.sendall(kind + struct.pack("!I", len(body) + 4) + body)
        return self.receive_ready()

    def close(self):
        self.connection.close()


@unittest.skipUnless(
    bool(_CONFIGURED_ENV),
    "真实 PG 生命周期测试需要显式指定待测 PG 和两个配套库；CI prepare 不运行该集成测试",
)
class DsgPostgresLifecycle(unittest.TestCase):
    def setUp(self):
        self.pg_bin = Path(os.environ["NEON_TEST_PG_BIN"]).resolve()
        self.library = Path(os.environ["NEON_TEST_DSG_LIBRARY"]).resolve()
        self.rmgr = Path(os.environ["NEON_TEST_RMGR_LIBRARY"]).resolve()
        self.temp = tempfile.TemporaryDirectory(prefix="neon-dsg-")
        self.root = Path(self.temp.name)
        self.data = self.root / "data"
        self.socket = self.root / "socket"
        self.socket.mkdir()
        self.log = self.root / "postgres.log"
        self.running = False
        self.addCleanup(self.cleanup_cluster)
        self.command(
            "initdb",
            "-D",
            self.data,
            "-A",
            "trust",
            "--no-locale",
            "--encoding=UTF8",
            "--data-checksums",
            "--no-sync",
        )
        self.config(
            "listen_addresses = ''\nport = 55432\n"
            f"unix_socket_directories = '{self.quote(self.socket)}'\n"
            f"shared_preload_libraries = '{self.quote(self.library)},{self.quote(self.rmgr)}'\n"
            "wal_consistency_checking = 'neon'\n"
            "enable_dynamic_shared_buffers = on\nshared_buffers = '2MB'\n"
            "max_shared_buffers = '64MB'\n"
        )

    @staticmethod
    def quote(value):
        return str(value).replace("'", "''")

    def config(self, contents):
        with (self.data / "postgresql.conf").open("a") as stream:
            stream.write("\n" + contents + "\n")

    def command(self, binary, *arguments, check=True):
        result = subprocess.run(
            [str(self.pg_bin / binary), *map(str, arguments)],
            text=True,
            capture_output=True,
            timeout=120,
        )
        if check and result.returncode:
            log = self.log.read_text() if self.log.exists() else ""
            self.fail(f"{binary}: {result.stdout}\n{result.stderr}\n{log[-12000:]}")
        return result

    def start(self, *, check=True, options=None):
        extra = [] if options is None else ["-o", options]
        result = self.command(
            "pg_ctl", "-D", self.data, "-l", self.log, "-w", *extra, "start", check=check
        )
        self.running = result.returncode == 0
        return result

    def sql(self, query, *, check=True):
        return self.command(
            "psql",
            "-h",
            self.socket,
            "-p",
            "55432",
            "-d",
            "postgres",
            "-X",
            "-qAt",
            "-v",
            "ON_ERROR_STOP=1",
            "-c",
            query,
            check=check,
        )

    def install_functions(self):
        library = self.quote(self.library)
        self.sql(
            f"CREATE FUNCTION dsg_boot() RETURNS bigint AS '{library}', 'test_dsg_boot_id' LANGUAGE C;"
            f"CREATE FUNCTION dsg_layout() RETURNS boolean AS '{library}', 'test_dsg_buffer_layout' LANGUAGE C;"
        )

    def background_sql(self, query, application):
        """启动本临时集群的真实会话，所有子进程都登记清理，避免断言失败后泄漏客户端。"""
        process = subprocess.Popen(
            [
                str(self.pg_bin / "psql"),
                "-h",
                str(self.socket),
                "-p",
                "55432",
                "-d",
                "postgres",
                "-X",
                "-qAt",
                "-v",
                "ON_ERROR_STOP=1",
                "-c",
                "SET statement_timeout = '20s'; " + query,
            ],
            env={**os.environ, "PGAPPNAME": application},
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        self.addCleanup(self.close_background, process)
        return process

    @staticmethod
    def close_background(process):
        if process.poll() is None:
            process.terminate()
        try:
            process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate(timeout=5)

    def wait_sql(self, query, expected):
        """使用新事务读取实际状态；有界轮询只用于等待确定条件，不以 sleep 猜调度。"""
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            result = self.sql(query).stdout.strip()
            if result == expected:
                return
            time.sleep(0.01)
        self.fail(f"状态未达到 {expected!r}: {result!r}\n{self.log.read_text()[-12000:]}")

    def install_io_functions(self):
        library = self.quote(self.library)
        self.sql(
            f"CREATE FUNCTION dsg_io(regclass, text) RETURNS boolean AS '{library}', "
            "'test_dsg_io' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_io_control(text) RETURNS integer AS '{library}', "
            "'test_dsg_io_control' LANGUAGE C STRICT;"
        )

    def prepare_admission_fixture(self):
        self.config(
            "dynamic_shared_buffers_require_admission = on\n"
            "dynamic_shared_buffers_startup_budget = '64MB'\n"
        )
        self.start()
        self.install_functions()
        library = self.quote(self.library)
        self.sql(
            "CREATE ROLE dsg_app LOGIN;"
            f"CREATE FUNCTION dsg_admission(bigint,bigint,bigint,boolean) RETURNS boolean AS '{library}', "
            "'test_dsg_admission' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_admission_snapshot() RETURNS text AS '{library}', "
            "'test_dsg_admission_snapshot' LANGUAGE C;"
            "CREATE FUNCTION dsg_grow(bigint,bigint,bigint,integer,integer,boolean) "
            f"RETURNS text AS '{library}', 'test_dsg_grow' LANGUAGE C STRICT;"
        )
        return self.sql("SELECT dsg_boot()").stdout.strip()

    def test_managed_bootstrap_checks_shared_layout_before_start(self):
        self.config(
            "dynamic_shared_buffers_require_admission = on\n"
            "dynamic_shared_buffers_startup_budget = '1MB'\n"
        )
        self.assertNotEqual(self.start(check=False).returncode, 0)
        self.assertIn("startup layout exceeds the shared-memory budget", self.log.read_text())

    def test_managed_bootstrap_requires_explicit_budget(self):
        self.config("dynamic_shared_buffers_require_admission = on\n")
        self.assertNotEqual(self.start(check=False).returncode, 0)
        self.assertIn("require an explicit startup shared-memory budget", self.log.read_text())

    def test_managed_command_line_survives_auto_conf_and_include_override(self):
        # 模拟恢复备份留下的大池auto.conf和官方末尾override。Compute必须用-c
        # 固定启动契约；仅在postgresql.conf追加安全值不能保证内部恢复的小池。
        (self.data / "postgresql.auto.conf").write_text(
            "shared_buffers = '32MB'\nmax_shared_buffers = '32MB'\n"
            "dynamic_shared_buffers_require_admission = off\n"
        )
        (self.data / "compute_ctl_temp_override.conf").write_text(
            "shared_buffers = '16MB'\ndynamic_shared_buffers_require_admission = off\n"
        )
        self.config("include_if_exists = 'compute_ctl_temp_override.conf'\n")
        self.start(
            options="-c shared_buffers=2048kB -c max_shared_buffers=65536kB "
            "-c dynamic_shared_buffers_require_admission=on "
            "-c dynamic_shared_buffers_startup_budget=65536kB"
        )
        self.assertEqual(
            self.sql(
                "SELECT current_setting('shared_buffers'),current_setting('max_shared_buffers'),"
                "current_setting('dynamic_shared_buffers_require_admission')"
            ).stdout.strip(),
            "2MB|64MB|on",
        )

    def test_closed_admission_rechecks_role_inside_multi_statement_query(self):
        self.prepare_admission_fixture()
        # 管理身份可执行SET ROLE；同一个Query里的下一条语句必须以新角色再检查。
        attempt = self.sql("SET ROLE dsg_app; SELECT 42", check=False)
        self.assertNotEqual(attempt.returncode, 0)
        self.assertIn("admission is closed", attempt.stderr)
        self.assertNotIn("42", attempt.stdout)

    def test_admission_covers_simple_extended_and_fastpath_on_same_connection(self):
        boot = self.prepare_admission_fixture()
        wire = PgWireConnection(self.socket, "dsg_app")
        self.addCleanup(wire.close)

        def denied(messages):
            errors = [body for kind, body in messages if kind == b"E"]
            self.assertTrue(errors, messages)
            self.assertIn(b"C55000\0", errors[0])
            self.assertIn(b"admission is closed", errors[0])
            self.assertFalse(any(kind == b"D" for kind, _ in messages))

        denied(wire.exchange([(b"Q", b"SELECT 42\0")]))
        parse = (b"P", b"dsg_statement\0SELECT 42\0" + struct.pack("!H", 0))
        bind = (b"B", b"dsg_portal\0dsg_statement\0" + struct.pack("!HHH", 0, 0, 0))
        execute = (b"E", b"dsg_portal\0" + struct.pack("!I", 0))
        sync = (b"S", b"")
        denied(wire.exchange([parse, sync]))
        oid = int(self.sql("SELECT 'pg_catalog.int4pl(integer,integer)'::regprocedure::oid").stdout)
        fastpath = struct.pack("!IHHH", oid, 1, 1, 2)
        fastpath += struct.pack("!IiIiH", 4, 20, 4, 22, 1)
        denied(wire.exchange([(b"F", fastpath)]))
        self.sql(f"SELECT dsg_admission({boot},1,1,true)")
        prepared = wire.exchange([parse, bind, sync])
        self.assertTrue(any(kind == b"1" for kind, _ in prepared), prepared)
        self.assertTrue(any(kind == b"2" for kind, _ in prepared), prepared)
        self.sql(f"SELECT dsg_admission({boot},2,1,false)")
        denied(wire.exchange([execute, sync]))
        self.sql(f"SELECT dsg_admission({boot},3,1,true)")
        completed = wire.exchange([bind, execute, sync])
        self.assertTrue(
            any(kind == b"D" and body.endswith(b"42") for kind, body in completed), completed
        )
        # 已连接会话同样受控，且普通角色在开放后也不能调用管理函数越权改门禁。
        unauthorized = wire.exchange([(b"Q", f"SELECT dsg_admission({boot},4,1,false)\0".encode())])
        self.assertTrue(any(kind == b"E" and b"C42501\0" in body for kind, body in unauthorized))
        stale = self.sql(f"SELECT dsg_admission({boot},2,1,true)", check=False)
        self.assertNotEqual(stale.returncode, 0)
        self.assertIn("invalid or stale", stale.stderr)

    def test_new_boot_closes_admission_and_returns_to_safe_bootstrap(self):
        boot = self.prepare_admission_fixture()
        original_postmaster = (self.data / "postmaster.pid").read_text().splitlines()[0]
        self.sql(
            f"SELECT dsg_admission({boot},1,1,true);"
            "SELECT dsg_grow(1,1,1,1024,1000,false);"
            "CREATE TABLE dsg_boot_data AS SELECT i FROM generate_series(1,1000) i; CHECKPOINT;"
        )
        # 必须取得确定仍在运行的Backend，不能向已经退出、可能复用的短会话PID发信号。
        crash = self.background_sql("SELECT pg_sleep(20)", "dsg-bootstrap-crash")
        self.wait_sql(
            "SELECT count(*) FROM pg_stat_activity WHERE application_name='dsg-bootstrap-crash' AND wait_event='PgSleep'",
            "1",
        )
        victim = int(
            self.sql(
                "SELECT pid FROM pg_stat_activity WHERE application_name='dsg-bootstrap-crash'"
            ).stdout
        )
        os.kill(victim, signal.SIGKILL)
        crash.communicate(timeout=10)
        deadline = time.monotonic() + 15
        new_boot = boot
        while time.monotonic() < deadline:
            observed = self.sql("SELECT dsg_boot()", check=False)
            if observed.returncode == 0 and observed.stdout.strip() != boot:
                new_boot = observed.stdout.strip()
                break
            time.sleep(0.02)
        self.assertNotEqual(new_boot, boot)
        self.assertEqual(
            (self.data / "postmaster.pid").read_text().splitlines()[0], original_postmaster
        )
        snapshot = self.sql("SELECT dsg_admission_snapshot()").stdout.strip()
        self.assertTrue(snapshot.startswith("open=no revision=0 high=256 shared="), snapshot)
        rejected = self.sql(f"SELECT dsg_admission({boot},2,1,true)", check=False)
        self.assertNotEqual(rejected.returncode, 0)
        wire = PgWireConnection(self.socket, "dsg_app")
        self.addCleanup(wire.close)
        self.assertTrue(any(kind == b"E" for kind, _ in wire.exchange([(b"Q", b"SELECT 42\0")])))
        self.sql(f"SELECT dsg_admission({new_boot},1,1,true)")
        self.assertEqual(
            self.sql("SELECT count(*),sum(i) FROM dsg_boot_data").stdout.strip(), "1000|500500"
        )

    def prepare_grow_exit_fixture(self, *, after_publish=False):
        self.start()
        library = self.quote(self.library)
        self.sql(
            "CREATE FUNCTION dsg_grow(bigint, bigint, bigint, integer, integer, boolean) "
            f"RETURNS text AS '{library}', 'test_dsg_grow' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_grow_control(text) RETURNS text AS '{library}', "
            "'test_dsg_grow_control' LANGUAGE C STRICT;"
        )
        self.sql(
            "SELECT dsg_grow_control('"
            + ("attach_after_publish" if after_publish else "attach")
            + "')"
        )

    def prepare_shrink_fixture(self):
        # 禁止后台提前写净，确保Shrink真实走脏页WAL/smgr写回。PLAIN禁止
        # TOAST/压缩把测试工作集缩成只落在bootstrap内，实际表必须超过小池。
        self.config("bgwriter_lru_maxpages=0\ncheckpoint_timeout='1h'\n")
        self.start()
        self.install_functions()
        library = self.quote(self.library)
        self.sql(
            "CREATE FUNCTION dsg_grow(bigint,bigint,bigint,integer,integer,boolean) "
            f"RETURNS text AS '{library}', 'test_dsg_grow' LANGUAGE C STRICT;"
            "CREATE FUNCTION dsg_shrink(bigint,bigint,bigint,integer,integer,boolean) "
            f"RETURNS text AS '{library}', 'test_dsg_shrink' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_resize_state() RETURNS text AS '{library}', "
            "'test_dsg_shrink_snapshot' LANGUAGE C;"
            f"CREATE FUNCTION dsg_grow_control(text) RETURNS text AS '{library}', "
            "'test_dsg_grow_control' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_tail_block(regclass,integer) RETURNS bigint AS '{library}', "
            "'test_dsg_tail_block' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_invalid_tail(regclass,integer) RETURNS integer AS '{library}', "
            "'test_dsg_invalid_tail' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_collision_unpin(regclass) RETURNS boolean AS '{library}', "
            "'test_dsg_collision_unpin' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_allocation_limit(integer) RETURNS text AS '{library}', "
            "'test_dsg_allocation_limit' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_relation_below(regclass,integer) RETURNS boolean AS '{library}', "
            "'test_dsg_relation_below' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_pin(regclass,integer,text) RETURNS boolean AS '{library}', "
            "'test_dsg_pin' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_pin_control(boolean) RETURNS boolean AS '{library}', "
            "'test_dsg_pin_control' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_checkpoint_control(text) RETURNS boolean AS '{library}', "
            "'test_dsg_checkpoint_control' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_grow_verify() RETURNS boolean AS '{library}', "
            "'test_dsg_grow_verify' LANGUAGE C;"
            f"CREATE FUNCTION dsg_stale_hint() RETURNS boolean AS '{library}', "
            "'test_dsg_stale_hint' LANGUAGE C;"
            "SELECT dsg_grow(1,1,1,2048,10000,false);"
            "CREATE TABLE dsg_shrink_data(i integer,pad text);"
            "ALTER TABLE dsg_shrink_data ALTER pad SET STORAGE PLAIN;"
            "INSERT INTO dsg_shrink_data SELECT i,repeat(md5(i::text),150) "
            "FROM generate_series(1,600) i;"
        )
        self.assertEqual(self.sql("SELECT dsg_grow_verify()").stdout.strip(), "t")
        self.assertGreater(
            int(self.sql("SELECT pg_relation_size('dsg_shrink_data')").stdout), 2 * 1024 * 1024
        )
        return self.shrink_data_checksum()

    def shrink_data_checksum(self):
        return self.sql(
            "SELECT count(*),sum(i),sum(octet_length(pad)),sum(hashtextextended(pad,0)) "
            "FROM dsg_shrink_data"
        ).stdout.strip()

    def resize_state(self):
        return json.loads(self.sql("SELECT dsg_resize_state()").stdout)

    def shrink_call(self, revision=2, generation=2, target=256, timeout=10000, *, check=True):
        return self.sql(
            f"SELECT dsg_shrink({revision},{revision},{generation},{target},{timeout},false)",
            check=check,
        )

    def hold_shrink_tail(self):
        block = self.sql("SELECT dsg_tail_block('dsg_shrink_data',256)").stdout.strip()
        self.assertTrue(block, "真实数据页没有驻留在待排水尾部")
        holder = self.background_sql(
            f"SELECT dsg_pin('dsg_shrink_data',{block},'tail_hold')", "dsg-shrink-holder"
        )
        self.wait_sql("SELECT dsg_pin_control(false)", "t")
        return holder

    def release_shrink_tail(self, holder):
        self.sql("SELECT dsg_pin_control(true)")
        _, errors = holder.communicate(timeout=10)
        self.assertEqual(holder.returncode, 0, errors)

    def test_shrink_writes_dirty_tail_and_regrow_preserves_real_data(self):
        expected = self.prepare_shrink_fixture()
        before = self.resize_state()
        self.assertIn("committed high=256 generation=3", self.shrink_call().stdout)
        after = self.resize_state()
        self.assertEqual((after["low"], after["high"], after["generation"]), (256, 256, 3))
        self.assertEqual(after["drain"][0], 2048 - 256)
        self.assertEqual(self.shrink_data_checksum(), expected)
        if os.uname().sysname == "Darwin":
            self.assertTrue(any(a["state"] == 4 for a in after["arrays"]))
            for old, new in zip(before["arrays"], after["arrays"], strict=True):
                self.assertEqual(new["upper"], old["upper"])
                self.assertFalse(new["verified"])
        else:
            self.assertTrue(all(a["state"] == 3 for a in after["arrays"]))
        self.assertIn("replay=true", self.shrink_call().stdout)
        wrong_kind = self.sql("SELECT dsg_grow(2,2,2,256,1000,false)", check=False)
        self.assertNotEqual(wrong_kind.returncode, 0)
        self.assertIn("identity was reused", wrong_kind.stderr)
        self.assertIn(
            "committed high=3072 generation=4",
            self.sql("SELECT dsg_grow(3,3,3,3072,10000,false)").stdout,
        )
        self.sql(
            "INSERT INTO dsg_shrink_data SELECT i,repeat(md5(i::text),150) "
            "FROM generate_series(601,1200) i"
        )
        self.assertEqual(self.sql("SELECT dsg_grow_verify()").stdout.strip(), "t")
        self.assertEqual(
            self.sql("SELECT count(*),sum(i) FROM dsg_shrink_data").stdout.strip(), "1200|720600"
        )
        self.assertIn("committed", self.shrink_call(4, 4).stdout)
        self.assertEqual(
            self.sql("SELECT count(*),sum(i) FROM dsg_shrink_data").stdout.strip(), "1200|720600"
        )
        # 初始布局测试要求预算恰等于bootstrap，不适用于Mac保守保留backing。
        # 此处检查真正收缩后的inactive tail提示拒绝，不削弱原启动预算断言。
        self.assertEqual(self.sql("SELECT dsg_stale_hint()").stdout.strip(), "t")

    def test_shrink_pin_timeout_restores_allocation_then_retry_succeeds(self):
        expected = self.prepare_shrink_fixture()
        holder = self.hold_shrink_tail()
        denied = self.shrink_call(timeout=100)
        self.assertIn("denied high=2048 generation=2", denied.stdout)
        state = self.resize_state()
        self.assertEqual(
            (state["low"], state["high"], state["phase"], state["reason"]), (2048, 2048, 3, 1)
        )
        self.assertGreater(state["drain"][1], 0)
        self.release_shrink_tail(holder)
        self.assertIn("committed high=256", self.shrink_call(3, 2).stdout)
        self.assertEqual(self.shrink_data_checksum(), expected)

    def test_shrink_waits_for_real_pin_and_completes_after_release(self):
        expected = self.prepare_shrink_fixture()
        holder = self.hold_shrink_tail()
        shrinking = self.background_sql("SELECT dsg_shrink(2,2,2,256,10000,false)", "dsg-shrinker")
        self.wait_sql("SELECT dsg_resize_state()::json->>'low'", "256")
        state = self.resize_state()
        self.assertEqual((state["high"], state["phase"]), (2048, 1))
        self.assertIsNone(shrinking.poll())
        self.release_shrink_tail(holder)
        output, errors = shrinking.communicate(timeout=10)
        self.assertEqual(shrinking.returncode, 0, errors)
        self.assertIn("committed high=256", output)
        self.assertEqual(self.shrink_data_checksum(), expected)

    def test_shrink_cancel_restores_fence_without_changing_high(self):
        self.prepare_shrink_fixture()
        holder = self.hold_shrink_tail()
        shrinking = self.background_sql(
            "SELECT dsg_shrink(2,2,2,256,10000,false)", "dsg-cancel-shrinker"
        )
        self.wait_sql("SELECT dsg_resize_state()::json->>'low'", "256")
        self.assertEqual(
            self.sql(
                "SELECT pg_cancel_backend(pid) FROM pg_stat_activity "
                "WHERE application_name='dsg-cancel-shrinker'"
            ).stdout.strip(),
            "t",
        )
        _, errors = shrinking.communicate(timeout=10)
        self.assertNotEqual(shrinking.returncode, 0)
        self.assertIn("canceling statement due to user request", errors)
        state = self.resize_state()
        self.assertEqual(
            (state["low"], state["high"], state["generation"], state["reason"]), (2048, 2048, 2, 3)
        )
        self.release_shrink_tail(holder)
        self.assertIn("committed", self.shrink_call(3, 2).stdout)

    def test_shrink_committed_error_keeps_high_and_pending_budget(self):
        expected = self.prepare_shrink_fixture()
        before = self.resize_state()
        self.sql("SELECT dsg_grow_control('attach_shrink_publish')")
        failed = self.sql(
            "SELECT dsg_grow_control('arm_error'); SELECT dsg_shrink(2,2,2,256,10000,false)",
            check=False,
        )
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("DSGA test resize ERROR", failed.stderr)
        state = self.resize_state()
        self.assertEqual((state["low"], state["high"], state["phase"]), (256, 256, 2))
        for old, new in zip(before["arrays"], state["arrays"], strict=True):
            self.assertEqual(new["upper"], old["upper"])
            self.assertEqual(new["state"], 1)
            self.assertTrue(new["partial"])
        self.assertIn("replay=true", self.shrink_call().stdout)
        self.sql("SELECT dsg_grow_control('detach_shrink')")
        self.assertIn("committed high=256 generation=3", self.shrink_call(3, 3).stdout)
        self.assertEqual(self.shrink_data_checksum(), expected)

    def exercise_shrink_fatal_competitor(self, *, after_publish, during_filter=False):
        expected = self.prepare_shrink_fixture()
        self.sql(
            "SELECT dsg_grow_control('"
            + (
                "attach_shrink_filter"
                if during_filter
                else ("attach_shrink_publish" if after_publish else "attach_shrink_fence")
            )
            + "')"
        )
        old = self.background_sql(
            "SELECT dsg_grow_control('arm_fatal'); SELECT dsg_shrink(2,2,2,256,10000,false)",
            "dsg-old-fatal-shrink",
        )
        self.wait_sql("SELECT dsg_grow_control('exit_waiting')", "yes")
        generation = 3 if after_publish else 2
        self.assertIn(
            "committed high=3072",
            self.sql(f"SELECT dsg_grow(3,3,{generation},3072,10000,false)").stdout,
        )
        before = self.resize_state()
        self.finish_fatal_grow(old)
        self.assertEqual(self.resize_state(), before)
        self.sql("SELECT dsg_grow_control('detach_shrink')")
        self.assertEqual(self.shrink_data_checksum(), expected)

    def test_shrink_fatal_before_publish_is_reconciled_before_new_grow(self):
        self.exercise_shrink_fatal_competitor(after_publish=False)

    def test_shrink_fatal_mid_freelist_filter_reconciles_shared_cursor(self):
        self.exercise_shrink_fatal_competitor(after_publish=False, during_filter=True)

    def test_shrink_fatal_after_publish_never_releases_new_grow_tail(self):
        self.exercise_shrink_fatal_competitor(after_publish=True)

    def test_shrink_keeps_checkpoint_workspace_while_regrowing(self):
        expected = self.prepare_shrink_fixture()
        self.sql("SELECT dsg_checkpoint_control('attach')")
        checkpoint = self.background_sql("CHECKPOINT", "dsg-shrink-checkpoint")
        self.wait_sql("SELECT dsg_checkpoint_control('waiting')", "t")
        self.assertIn("committed high=256", self.shrink_call().stdout)
        state = self.resize_state()
        self.assertEqual(state["arrays"][3]["state"], 2)
        self.assertFalse(state["arrays"][3]["verified"])
        self.assertIn(
            "committed high=1024",
            self.sql("SELECT dsg_grow(3,3,3,1024,10000,false)").stdout,
        )
        self.sql("SELECT dsg_checkpoint_control('release')")
        _, errors = checkpoint.communicate(timeout=10)
        self.assertEqual(checkpoint.returncode, 0, errors)
        self.sql("SELECT dsg_checkpoint_control('detach')")
        self.assertIn("committed high=256", self.shrink_call(4, 4).stdout)
        self.assertEqual(self.shrink_data_checksum(), expected)

    def test_shrink_hash_miss_reloads_below_fence_before_high_publication(self):
        expected = self.prepare_shrink_fixture()
        self.sql("SELECT dsg_grow_control('attach_shrink_prepare')")
        shrinking = self.background_sql(
            "SELECT dsg_grow_control('arm_pause'); SELECT dsg_shrink(2,2,2,256,10000,false)",
            "dsg-shrink-untagged",
        )
        self.wait_sql("SELECT dsg_grow_control('prepare_waiting')", "yes")
        self.assertEqual(self.resize_state()["high"], 2048)
        self.assertEqual(self.shrink_data_checksum(), expected)
        self.assertEqual(
            self.sql("SELECT dsg_relation_below('dsg_shrink_data',256)").stdout.strip(), "t"
        )
        self.sql("SELECT dsg_grow_control('release_prepare')")
        output, errors = shrinking.communicate(timeout=10)
        self.assertEqual(shrinking.returncode, 0, errors)
        self.assertIn("committed high=256", output)
        self.sql("SELECT dsg_grow_control('detach_shrink')")

    def test_shrink_removes_real_failed_read_tagged_invalid_tail(self):
        expected = self.prepare_shrink_fixture()
        # 600正好越过现有文件EOF，仍在已存在的segment0内，确保走真实读失败
        # 而非在打开不存在segment1时提前报错。
        failed = self.sql("SELECT dsg_pin('dsg_shrink_data',600,'read')", check=False)
        self.assertNotEqual(failed.returncode, 0)
        self.assertRegex(failed.stderr, r"could not read (?:block|blocks).*600")
        self.assertGreater(
            int(self.sql("SELECT dsg_invalid_tail('dsg_shrink_data',256)").stdout), 0
        )
        self.assertIn("committed high=256", self.shrink_call().stdout)
        self.assertEqual(self.shrink_data_checksum(), expected)

    def test_shrink_second_array_release_error_retains_unproven_budget(self):
        expected = self.prepare_shrink_fixture()
        before = self.resize_state()
        self.sql("SELECT dsg_grow_control('attach_release')")
        failed = self.sql(
            "SELECT dsg_grow_control('arm_second_release_error'); "
            "SELECT dsg_shrink(2,2,2,256,10000,false)",
            check=False,
        )
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("second array release ERROR", failed.stderr)
        state = self.resize_state()
        self.assertEqual((state["high"], state["phase"]), (256, 2))
        self.assertIn(state["arrays"][0]["state"], (3, 4))
        for old, new in zip(before["arrays"][1:], state["arrays"][1:], strict=True):
            self.assertEqual(new["upper"], old["upper"])
            self.assertEqual(new["state"], 1)
            self.assertTrue(new["partial"])
        self.sql("SELECT dsg_grow_control('detach_shrink')")
        self.assertIn("committed", self.shrink_call(3, 3).stdout)
        self.assertEqual(self.shrink_data_checksum(), expected)

    def test_shrink_repeated_cross_granule_resize_and_invalid_requests(self):
        expected = self.prepare_shrink_fixture()
        generation, high = 2, 2048
        for revision, target in enumerate((256, 3072, 512, 4096, 768, 2048, 256), start=2):
            command = "dsg_shrink" if target < high else "dsg_grow"
            result = self.sql(
                f"SELECT {command}({revision},{revision},{generation},{target},10000,false)"
            )
            generation += 1
            high = target
            self.assertIn(f"committed high={high} generation={generation}", result.stdout)
            self.assertEqual(self.shrink_data_checksum(), expected)
        for args in (
            f"9,9,{generation},255,1000,false",
            f"9,9,{generation},512,1000,false",
            f"9,9,{generation - 1},256,1000,false",
            f"9,9,{generation},256,1000,true",
            f"8,8,{generation},256,1000,false",
        ):
            self.assertNotEqual(self.sql(f"SELECT dsg_shrink({args})", check=False).returncode, 0)
        self.assertEqual(self.resize_state()["generation"], generation)

    def test_shrink_collision_victim_is_not_accessed_after_last_unpin(self):
        expected = self.prepare_shrink_fixture()
        boot = self.sql("SELECT dsg_boot()").stdout
        self.install_io_functions()
        # 先持久化，再仅丢弃缓存，使两个真实ReadBuffer都从hash miss开始。
        self.sql("CHECKPOINT; SELECT dsg_io('dsg_shrink_data','invalidate')")
        collision = self.background_sql(
            "SELECT dsg_collision_unpin('dsg_shrink_data')", "dsg-real-hash-collision"
        )
        self.wait_sql("SELECT dsg_grow_control('prepare_waiting')", "yes")
        # 原victim已在尾部持pin；新读者只能选bootstrap前缀，避免两者恰在
        # 同一张Descriptor页而使页保护探针对合法胜者访问产生误报。
        self.sql("SELECT dsg_allocation_limit(256); SELECT dsg_pin('dsg_shrink_data',0,'read')")
        self.sql("SELECT dsg_grow_control('release_prepare')")
        output, errors = collision.communicate(timeout=10)
        self.assertEqual(collision.returncode, 0, errors + self.log.read_text()[-12000:])
        self.assertIn("t", output)
        self.assertEqual(self.sql("SELECT dsg_boot()").stdout, boot)
        self.sql("SELECT dsg_allocation_limit(2048)")
        self.assertIn("committed high=256", self.shrink_call().stdout)
        self.assertEqual(self.shrink_data_checksum(), expected)

    def test_shrink_skips_real_tail_io_without_holding_access_guard(self):
        expected = self.prepare_shrink_fixture()
        self.install_io_functions()
        library = self.quote(self.library)
        self.sql(
            f"CREATE FUNCTION dsg_io_at(regclass,text,integer) RETURNS boolean AS '{library}', "
            "'test_dsg_io' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_exclusive_available() RETURNS boolean AS '{library}', "
            "'test_dsg_exclusive_available' LANGUAGE C;"
            "SELECT dsg_io_control('attach')"
        )
        block = self.sql("SELECT dsg_tail_block('dsg_shrink_data',256)").stdout.strip()
        self.assertTrue(block)
        writer = self.background_sql(
            f"SELECT dsg_io_at('dsg_shrink_data','writer',{block})", "dsg-tail-io-owner"
        )
        self.wait_sql("SELECT dsg_io_control('writer_waiting')", "1")
        shrinking = self.background_sql(
            "SELECT dsg_shrink(2,2,2,256,10000,false)", "dsg-tail-io-shrinker"
        )
        self.wait_sql("SELECT (dsg_resize_state()::json->'drain'->>2)::integer>0", "t")
        self.assertEqual(self.resize_state()["high"], 2048)
        self.assertEqual(self.sql("SELECT dsg_exclusive_available()").stdout.strip(), "t")
        self.sql("SELECT dsg_io_control('release_writer')")
        for process in (writer, shrinking):
            output, errors = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 0, errors)
        self.assertIn("committed high=256", output)
        self.sql("SELECT dsg_io_control('detach')")
        self.assertEqual(self.shrink_data_checksum(), expected)

    def test_shrink_cancel_during_provider_keeps_committed_capacity_and_budget(self):
        expected = self.prepare_shrink_fixture()
        before = self.resize_state()
        self.sql("SELECT dsg_grow_control('attach_release')")
        shrinking = self.background_sql(
            "SELECT dsg_grow_control('arm_pause'); SELECT dsg_shrink(2,2,2,256,10000,false)",
            "dsg-cancel-release",
        )
        self.wait_sql("SELECT dsg_grow_control('prepare_waiting')", "yes")
        self.assertEqual(
            self.sql(
                "SELECT pg_cancel_backend(pid) FROM pg_stat_activity "
                "WHERE application_name='dsg-cancel-release'"
            ).stdout.strip(),
            "t",
        )
        _, errors = shrinking.communicate(timeout=10)
        self.assertNotEqual(shrinking.returncode, 0)
        self.assertIn("canceling statement due to user request", errors)
        state = self.resize_state()
        self.assertEqual((state["low"], state["high"], state["phase"]), (256, 256, 2))
        for old, new in zip(before["arrays"], state["arrays"], strict=True):
            self.assertEqual(new["upper"], old["upper"])
            self.assertEqual(new["state"], 1)
        self.sql("SELECT dsg_grow_control('detach_shrink')")
        self.assertIn(
            "committed high=3072", self.sql("SELECT dsg_grow(3,3,3,3072,10000,false)").stdout
        )
        self.assertEqual(self.shrink_data_checksum(), expected)

    def start_fatal_grow(self):
        old = self.background_sql(
            "SELECT dsg_grow_control('arm_fatal'); SELECT dsg_grow(1, 1, 1, 1024, 10000, false)",
            "dsg-old-fatal-grow",
        )
        self.wait_sql("SELECT dsg_grow_control('exit_waiting')", "yes")
        return old

    def finish_fatal_grow(self, old):
        self.sql("SELECT dsg_grow_control('release_exit')")
        _, errors = old.communicate(timeout=10)
        self.assertNotEqual(old.returncode, 0)
        self.assertIn("DSGA test Grow FATAL", errors)

    def exercise_grow_fatal_competitor(self, *, after_publish=False, pause_new=True):
        self.prepare_grow_exit_fixture(after_publish=after_publish)
        old = self.start_fatal_grow()
        if after_publish:
            # 旧操作在发布后退出；下一操作仍必须暂停在发布前，不能把
            # “新H已提交”误当成PREPARING状态。两点回调只消费本地一次武装。
            self.sql("SELECT dsg_grow_control('attach')")
        generation = 2 if after_publish else 1
        high = 1024 if after_publish else 256
        prefix = "SELECT dsg_grow_control('arm_pause'); " if pause_new else ""
        new = self.background_sql(
            prefix + f"SELECT dsg_grow(2, 2, {generation}, 2048, 10000, false)",
            "dsg-new-grow",
        )
        if pause_new:
            self.wait_sql("SELECT dsg_grow_control('prepare_waiting')", "yes")
            before = self.sql("SELECT dsg_grow_control('operation')").stdout.strip()
            self.assertEqual(before, f"op=2 phase=1 reason=0 high={high} generation={generation}")
        else:
            _, errors = new.communicate(timeout=10)
            self.assertEqual(new.returncode, 0, errors)
            before = self.sql("SELECT dsg_grow_control('operation')").stdout.strip()
        self.finish_fatal_grow(old)
        # 新协调者持锁时旧回调应放弃清理；新协调者已提交并放锁时，旧回调
        # 即便重获锁也必须按身份拒绝写入。两条路径都不能把新操作标为DENIED。
        self.assertEqual(self.sql("SELECT dsg_grow_control('operation')").stdout.strip(), before)
        if pause_new:
            self.sql("SELECT dsg_grow_control('release_prepare')")
            _, errors = new.communicate(timeout=10)
            self.assertEqual(new.returncode, 0, errors)
        self.assertEqual(
            self.sql("SELECT dsg_grow_control('operation')").stdout.strip(),
            f"op=2 phase=2 reason=0 high=2048 generation={generation + 1}",
        )
        self.sql("SELECT dsg_grow_control('detach')")

    def test_grow_fatal_cleanup_does_not_deny_new_preparing_operation(self):
        self.exercise_grow_fatal_competitor()

    def test_grow_committed_fatal_cleanup_does_not_deny_next_operation(self):
        self.exercise_grow_fatal_competitor(after_publish=True)

    def test_grow_fatal_cleanup_does_not_change_new_committed_operation(self):
        self.exercise_grow_fatal_competitor(pause_new=False)

    def test_grow_fatal_cleanup_reclaims_own_operation(self):
        self.prepare_grow_exit_fixture()
        failed = self.sql(
            "SELECT dsg_grow_control('arm_fatal_unpaused'); "
            "SELECT dsg_grow(1, 1, 1, 1024, 10000, false)",
            check=False,
        )
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("DSGA test Grow FATAL", failed.stderr)
        self.wait_sql(
            "SELECT dsg_grow_control('operation')", "op=1 phase=3 reason=3 high=256 generation=1"
        )
        self.assertIn("denied", self.sql("SELECT dsg_grow(1,1,1,1024,1000,false)").stdout)
        self.assertIn("committed", self.sql("SELECT dsg_grow(2,2,1,1024,1000,false)").stdout)

    def test_grow_rejected_request_does_not_clean_another_operation(self):
        self.prepare_grow_exit_fixture()
        old = self.start_fatal_grow()
        before = self.sql("SELECT dsg_grow_control('operation')").stdout
        rejected = self.sql("SELECT dsg_grow(2,2,99,2048,1000,false)", check=False)
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn("conflicts with current capacity", rejected.stderr)
        self.assertEqual(self.sql("SELECT dsg_grow_control('operation')").stdout, before)
        self.finish_fatal_grow(old)
        self.assertEqual(
            self.sql("SELECT dsg_grow_control('operation')").stdout.strip(),
            "op=1 phase=3 reason=3 high=256 generation=1",
        )

    def exercise_unpinned_io_wait(self, action, *, owner_finishes=True):
        # 初始数据已持久化；测试仅把同一页面标脏，Drop 只移除缓存而不删除文件。
        # 禁止后台抢先清理，以确保 writer 真正进入原生 StartBufferIO。
        self.config("bgwriter_lru_maxpages = 0\ncheckpoint_timeout = '1h'\n")
        self.start()
        self.install_functions()
        self.install_io_functions()
        self.sql(
            "CREATE TABLE dsg_io_data AS SELECT i FROM generate_series(1, 100) i; "
            "CHECKPOINT; SELECT dsg_io_control('attach');"
        )
        boot = self.sql("SELECT dsg_boot()").stdout
        writer = self.background_sql("SELECT dsg_io('dsg_io_data', 'writer')", "dsg-io-writer")
        self.wait_sql("SELECT dsg_io_control('writer_waiting')", "1")
        waiter = self.background_sql(
            "SELECT dsg_io('dsg_io_data', 'invalidate')", "dsg-io-invalidator"
        )
        self.wait_sql("SELECT dsg_io_control('waiter_waiting')", "1")

        # 先让原 IO owner 完成并放下最后原始 pin。即使它的广播已经把等待者
        # 从队列弹出，等待者的 cv_sleep_target 仍指向 IOCV，必须保留自身 pin。
        if owner_finishes:
            self.sql("SELECT dsg_io_control('release_writer')")
            _, errors = writer.communicate(timeout=10)
            self.assertEqual(writer.returncode, 0, errors)
            self.wait_sql("SELECT dsg_io_control('waiter_alone')", "1")
        if action == "wake":
            self.sql("SELECT dsg_io_control('release_waiter')")
        else:
            function = "pg_cancel_backend" if action == "cancel" else "pg_terminate_backend"
            self.assertEqual(
                self.sql(
                    f"SELECT {function}(pid) FROM pg_stat_activity "
                    "WHERE application_name = 'dsg-io-invalidator'"
                ).stdout.strip(),
                "t",
            )
        _, errors = waiter.communicate(timeout=10)
        if action == "wake":
            self.assertEqual(waiter.returncode, 0, errors)
        else:
            self.assertNotEqual(waiter.returncode, 0)
            self.assertIn(
                "canceling statement due to user request"
                if action == "cancel"
                else "terminating connection due to administrator command",
                errors,
            )
            # ERROR 展开到审核点时仍有 Guard；FATAL 则先 LWLockReleaseAll。
            # 两者在原生资源清理放 pin 前都已退出 CV，并保有自身真实 pin。
            self.assertEqual(
                self.sql("SELECT dsg_io_control('audit')").stdout.strip(),
                "7" if action == "cancel" else "3",
            )
        if not owner_finishes:
            # 保持 owner 尚未广播，等待者原本仍在 CV 队列中。审核的退队位
            # 因而验证显式清理，不能被“广播已弹出但 target 仍在”掩盖。
            self.sql("SELECT dsg_io_control('release_writer')")
            _, errors = writer.communicate(timeout=10)
            self.assertEqual(writer.returncode, 0, errors)
        self.sql("SELECT dsg_io_control('detach'); CHECKPOINT;")
        self.assertEqual(self.sql("SELECT dsg_boot()").stdout, boot)
        self.assertEqual(
            self.sql("SELECT count(*), sum(i) FROM dsg_io_data").stdout.strip(), "100|5050"
        )
        self.assertEqual(self.sql("SELECT dsg_layout()").stdout.strip(), "t")

    def test_unpinned_io_wait_finishes_after_owner_releases_pin(self):
        self.exercise_unpinned_io_wait("wake")

    def test_unpinned_io_wait_cancel_cleans_cv_before_guard(self):
        self.exercise_unpinned_io_wait("cancel")

    def test_unpinned_io_wait_terminate_retains_pin_until_cv_cleanup(self):
        self.exercise_unpinned_io_wait("terminate")

    def test_unpinned_io_wait_cancel_dequeues_before_owner_finishes(self):
        self.exercise_unpinned_io_wait("cancel", owner_finishes=False)

    def test_unpinned_io_wait_terminate_dequeues_before_owner_finishes(self):
        self.exercise_unpinned_io_wait("terminate", owner_finishes=False)

    def test_recovery_fast_drop_holds_array_guard(self):
        self.start()
        self.install_io_functions()
        self.sql("CREATE TABLE dsg_drop_data AS SELECT i FROM generate_series(1, 100) i;")
        standby = self.root / "standby"
        standby_socket = self.root / "standby-socket"
        standby_socket.mkdir()
        standby_log = self.root / "standby.log"
        self.command(
            "pg_basebackup",
            "-h",
            self.socket,
            "-p",
            "55432",
            "-D",
            standby,
            "-X",
            "stream",
            "-R",
            "--checkpoint=fast",
        )
        with (standby / "postgresql.conf").open("a") as stream:
            stream.write(
                f"\nport = 55433\nunix_socket_directories = '{self.quote(standby_socket)}'\n"
            )
        self.addCleanup(
            self.command, "pg_ctl", "-D", standby, "-m", "immediate", "stop", check=False
        )
        self.command("pg_ctl", "-D", standby, "-l", standby_log, "-w", "start")

        def standby_sql(query):
            result = self.command(
                "psql",
                "-h",
                standby_socket,
                "-p",
                "55433",
                "-d",
                "postgres",
                "-X",
                "-qAt",
                "-v",
                "ON_ERROR_STOP=1",
                "-c",
                query,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr + standby_log.read_text()[-12000:])
            return result.stdout.strip()

        def wait_replay(lsn):
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                if standby_sql(f"SELECT pg_last_wal_replay_lsn() >= '{lsn}'::pg_lsn") == "t":
                    return
                time.sleep(0.01)
            self.fail("备库重放未达到目标\n" + standby_log.read_text()[-12000:])

        # 使用真实物理备库和 WAL 重放触发只在恢复中启用的 size-cache 快速路径，
        # 不伪造 InRecovery、缓存大小或 BufferTag。先重放一条 INSERT 形成原生缓存。
        self.assertEqual(standby_sql("SELECT pg_is_in_recovery()"), "t")
        standby_sql("SELECT dsg_io_control('attach_fast_drop')")
        self.sql("INSERT INTO dsg_drop_data VALUES (101)")
        wait_replay(self.sql("SELECT pg_current_wal_insert_lsn()").stdout.strip())
        self.assertEqual(standby_sql("SELECT count(*) FROM dsg_drop_data"), "101")
        self.sql("DROP TABLE dsg_drop_data")
        wait_replay(self.sql("SELECT pg_current_wal_insert_lsn()").stdout.strip())
        self.assertEqual(standby_sql("SELECT to_regclass('dsg_drop_data') IS NULL"), "t")
        self.assertGreater(int(standby_sql("SELECT dsg_io_control('fast_drop_found')")), 0)
        standby_sql("SELECT dsg_io_control('detach')")

    def exercise_cleanup_waiter(self, action):
        self.start()
        self.install_functions()
        library = self.quote(self.library)
        self.sql(
            f"CREATE FUNCTION dsg_pin(regclass, integer, text) RETURNS boolean AS '{library}', "
            "'test_dsg_pin' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_pin_control(boolean) RETURNS boolean AS '{library}', "
            "'test_dsg_pin_control' LANGUAGE C STRICT;"
            "CREATE TABLE dsg_pin_data AS SELECT i FROM generate_series(1, 100) i; CHECKPOINT;"
        )
        boot = self.sql("SELECT dsg_boot()").stdout
        holder = self.background_sql(
            "SELECT dsg_pin('dsg_pin_data', 0, 'hold')", "dsg-native-pin-holder"
        )
        self.wait_sql("SELECT dsg_pin_control(false)", "t")
        waiter = self.background_sql(
            "SELECT dsg_pin('dsg_pin_data', 0, 'cleanup')", "dsg-native-pin-waiter"
        )

        # 原生 BufferPin 不属于 heavyweight lock，isolationtester 无法可靠识别。
        # 必须先观察真实等待，再释放 holder 或发送取消，避免测成一次无竞争调用。
        self.wait_sql(
            "SELECT count(*) FROM pg_stat_activity WHERE "
            "application_name = 'dsg-native-pin-waiter' AND state = 'active' "
            "AND wait_event = 'BufferPin'",
            "1",
        )
        if action != "wake":
            function = "pg_cancel_backend" if action == "cancel" else "pg_terminate_backend"
            self.assertEqual(
                self.sql(
                    f"SELECT {function}(pid) FROM pg_stat_activity "
                    "WHERE application_name = 'dsg-native-pin-waiter'"
                ).stdout.strip(),
                "t",
            )
            _, errors = waiter.communicate(timeout=10)
            self.assertNotEqual(waiter.returncode, 0)
            self.assertIn(
                "canceling statement due to user request"
                if action == "cancel"
                else "terminating connection due to administrator command",
                errors,
            )
            self.assertEqual(self.sql("SELECT dsg_pin_control(false)").stdout.strip(), "t")

        self.assertEqual(self.sql("SELECT dsg_pin_control(true)").stdout.strip(), "t")
        holder_output, holder_errors = holder.communicate(timeout=10)
        self.assertEqual(holder.returncode, 0, holder_errors)
        self.assertEqual(holder_output.strip(), "t")
        if action == "wake":
            waiter_output, waiter_errors = waiter.communicate(timeout=10)
            self.assertEqual(waiter.returncode, 0, waiter_errors)
            self.assertEqual(waiter_output.strip(), "t")

        # 再取 cleanup lock 验证 waiter flag、pin 和内容锁均已清理；FATAL 只终止
        # 指定会话，Postmaster/共享 boot 应继续存活，数据也必须保持一致。
        self.assertEqual(
            self.sql("SELECT dsg_pin('dsg_pin_data', 0, 'cleanup')").stdout.strip(), "t"
        )
        self.assertEqual(
            self.sql("SELECT count(*), sum(i) FROM dsg_pin_data").stdout.strip(), "100|5050"
        )
        self.assertEqual(self.sql("SELECT dsg_boot()").stdout, boot)

    def test_native_pin_waiter_wakes_after_last_other_pin(self):
        self.exercise_cleanup_waiter("wake")

    def test_native_pin_waiter_cancel_cleans_references(self):
        self.exercise_cleanup_waiter("cancel")

    def test_native_pin_waiter_termination_cleans_references(self):
        self.exercise_cleanup_waiter("terminate")

    def test_last_unpin_stops_descriptor_access(self):
        self.start()
        library = self.quote(self.library)
        self.sql(
            "CREATE FUNCTION dsg_grow(bigint, bigint, bigint, integer, integer, boolean) "
            f"RETURNS text AS '{library}', 'test_dsg_grow' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_unpin_probe() RETURNS boolean AS '{library}', "
            "'test_dsg_unpin_probe' LANGUAGE C;"
            "SELECT dsg_grow(1, 1, 1, 1024, 1000, false);"
            "CREATE TABLE dsg_probe_data AS SELECT i, repeat(md5(i::text), 32) AS payload "
            "FROM generate_series(1, 3000) i; CHECKPOINT;"
        )
        checksum = (
            "SELECT count(*), sum(i), md5(string_agg(payload, ',' ORDER BY i)) FROM dsg_probe_data"
        )
        before = self.sql(checksum).stdout
        self.assertEqual(self.sql("SELECT dsg_unpin_probe()").stdout.strip(), "t")
        self.assertEqual(self.sql(checksum).stdout, before)

    def prepare_checkpoint_fixture(self):
        # 工作集小于本测试的启动池，关闭后台清理，确保请求的 checkpoint
        # 确实包含脏页。这里控制测试调度，不模拟 checkpoint 或改共享 flags。
        self.config("shared_buffers = '8MB'\nbgwriter_lru_maxpages = 0")
        self.start()
        self.install_functions()
        library = self.quote(self.library)
        self.sql(
            "CREATE FUNCTION dsg_grow(bigint, bigint, bigint, integer, integer, boolean) "
            f"RETURNS text AS '{library}', 'test_dsg_grow' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_checkpoint(text) RETURNS boolean AS '{library}', "
            "'test_dsg_checkpoint_control' LANGUAGE C STRICT;"
            "CREATE TABLE dsg_checkpoint_data (id int PRIMARY KEY, payload text);"
            "INSERT INTO dsg_checkpoint_data SELECT i, repeat(md5(i::text), 32) "
            "FROM generate_series(1, 1500) i;"
        )
        return (
            "SELECT count(*), sum(id), md5(string_agg(payload, ',' ORDER BY id)) "
            "FROM dsg_checkpoint_data"
        )

    def test_checkpoint_workspace_lease_does_not_block_grow(self):
        checksum = self.prepare_checkpoint_fixture()
        before = self.sql(checksum).stdout
        self.sql("SELECT dsg_checkpoint('attach')")
        checkpoint = self.background_sql("CHECKPOINT", "dsg-checkpoint-request")
        try:
            self.wait_sql("SELECT dsg_checkpoint('waiting')", "t")
            self.assertEqual(self.sql("SELECT dsg_checkpoint('available')").stdout.strip(), "f")
            # checkpointer 仍持工作区租约时，另一个真实 Backend 必须能在
            # 短期限内完成 Grow；旧全程数组 Guard 会使此调用超时拒绝。
            self.assertEqual(
                self.sql("SELECT dsg_grow(1, 1, 1, 2048, 1000, false)").stdout.strip(),
                "committed high=2048 generation=2 replay=false",
            )
        finally:
            self.sql("SELECT dsg_checkpoint('release')")
            checkpoint_output, checkpoint_errors = checkpoint.communicate(timeout=10)
            self.sql("SELECT dsg_checkpoint('detach')")
        self.assertEqual(checkpoint.returncode, 0, checkpoint_errors)
        self.assertEqual(checkpoint_output.strip(), "")
        self.assertEqual(self.sql("SELECT dsg_checkpoint('available')").stdout.strip(), "t")
        self.assertEqual(self.sql(checksum).stdout, before)
        self.sql("CREATE EXTENSION amcheck; SELECT bt_index_check('dsg_checkpoint_data_pkey')")

    def exercise_checkpoint_error(self, action):
        checksum = self.prepare_checkpoint_fixture()
        before = self.sql(checksum).stdout
        boot = self.sql("SELECT dsg_boot()").stdout
        self.sql(f"SELECT dsg_checkpoint('{action}')")
        try:
            result = self.sql("CHECKPOINT", check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("DSGA test checkpoint failure", self.log.read_text())
        finally:
            self.sql("SELECT dsg_checkpoint('detach')")
        self.assertEqual(self.sql("SELECT dsg_checkpoint('available')").stdout.strip(), "t")
        self.assertEqual(
            self.sql("SELECT dsg_grow(1, 1, 1, 2048, 1000, false)").stdout.strip(),
            "committed high=2048 generation=2 replay=false",
        )
        self.sql("CHECKPOINT")
        self.assertEqual(self.sql(checksum).stdout, before)
        self.assertEqual(self.sql("SELECT dsg_boot()").stdout, boot)

    def test_checkpoint_error_releases_workspace_lease(self):
        self.exercise_checkpoint_error("attach_error")

    def test_checkpoint_error_after_pin_cleans_pin_and_workspace(self):
        # 在真实 pin 和 content lock 已建立后抛错，覆盖非末尾 LWLock 的
        # 租约释放与 PG 原生 ResourceOwner 清理，不能仅测试扫描前失败。
        self.exercise_checkpoint_error("attach_write_error")

    def test_grow_preserves_data_and_recovers_after_crash(self):
        self.start()
        self.install_functions()
        library = self.quote(self.library)
        self.sql(
            "CREATE FUNCTION dsg_grow(bigint, bigint, bigint, integer, integer, boolean) "
            f"RETURNS text AS '{library}', 'test_dsg_grow' LANGUAGE C STRICT;"
            f"CREATE FUNCTION dsg_grown_used() RETURNS boolean AS '{library}', "
            "'test_dsg_grow_verify' LANGUAGE C;"
            "CREATE TABLE dsg_grow_data (id int PRIMARY KEY, payload text);"
            "INSERT INTO dsg_grow_data SELECT i, repeat(md5(i::text), 32) "
            "FROM generate_series(1, 12000) i;"
        )
        checksum = (
            "SELECT count(*), sum(id), md5(string_agg(payload, ',' ORDER BY id)) FROM dsg_grow_data"
        )
        before = self.sql(checksum).stdout
        boot = self.sql("SELECT dsg_boot()").stdout

        # 同一真实连接执行扩容、继续查询并回滚 SQL 事务；共享容量应保留，
        # 不能只用多次新建连接模拟“保持连接”。所有 SQL 只作用于本测试集群。
        result = self.sql(
            "BEGIN; CREATE TEMP TABLE dsg_connection AS SELECT pg_backend_pid() AS pid;"
            "SELECT dsg_grow(10, 10, 1, 512, 1000, false);"
            "SELECT pg_backend_pid() = (SELECT pid FROM dsg_connection); ROLLBACK;"
        ).stdout.splitlines()
        self.assertEqual(result, ["committed high=512 generation=2 replay=false", "t"])
        self.assertEqual(
            self.sql("SELECT dsg_grow(10, 10, 1, 512, 1000, false)").stdout.strip(),
            "committed high=512 generation=2 replay=true",
        )
        self.assertEqual(
            self.sql("SELECT dsg_grow(11, 11, 2, 2304, 1000, false)").stdout.strip(),
            "committed high=2304 generation=3 replay=false",
        )
        self.assertEqual(self.sql(checksum).stdout, before)
        self.assertEqual(self.sql("SELECT dsg_grown_used()").stdout.strip(), "t")
        self.assertEqual(self.sql("SHOW shared_buffers").stdout.strip(), "2MB")
        self.sql("VACUUM (FREEZE, ANALYZE) dsg_grow_data")

        # immediate stop 留下真实恢复工作，重启必须换 boot 并使用启动初值；
        # 扩容期间的数据仍由 Neon rmgr/WAL 恢复，不能把容量日志当作持久化数据。
        self.stop()
        self.start()
        self.assertNotEqual(self.sql("SELECT dsg_boot()").stdout, boot)
        self.assertEqual(self.sql(checksum).stdout, before)
        self.assertEqual(self.sql("SELECT dsg_layout()").stdout.strip(), "t")
        self.sql("CREATE EXTENSION amcheck; SELECT bt_index_check('dsg_grow_data_pkey')")

    def stop(self):
        self.command("pg_ctl", "-D", self.data, "-w", "-m", "immediate", "stop")
        self.running = False

    def cleanup_cluster(self):
        try:
            if (
                self.data.exists()
                and self.command("pg_ctl", "-D", self.data, "status", check=False).returncode == 0
            ):
                self.stop()
        finally:
            self.temp.cleanup()

    def test_crash_rebuilds_directory_in_same_postmaster(self):
        self.start()
        self.install_functions()
        self.assertEqual(self.sql("SELECT dsg_layout()").stdout.strip(), "t")
        self.sql(
            "CREATE TABLE dsg_data (id int PRIMARY KEY, payload text);"
            "INSERT INTO dsg_data SELECT i, repeat(md5(i::text), 32) FROM generate_series(1, 20000) i;"
        )
        self.sql("VACUUM (FREEZE, ANALYZE) dsg_data")
        before = self.sql(
            "SELECT count(*), sum(id), md5(string_agg(payload, ',' ORDER BY id)) FROM dsg_data"
        ).stdout
        postmaster = int((self.data / "postmaster.pid").read_text().splitlines()[0])
        # 连续重建，验证 Postmaster 的 on_shmem_exit 清理与新 boot 身份。
        # 只杀本集群中由本测试刚启动并读取 PID 的连接，不触及其他实例。
        for _ in range(2):
            boot = self.sql("SELECT dsg_boot()").stdout.strip()
            with subprocess.Popen(
                [
                    str(self.pg_bin / "psql"),
                    "-h",
                    str(self.socket),
                    "-p",
                    "55432",
                    "-d",
                    "postgres",
                    "-X",
                    "-qAt",
                    "-c",
                    "SELECT pg_sleep(60);",
                ],
                env={**os.environ, "PGAPPNAME": "neon_dsg_crash_probe"},
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            ) as victim:
                try:
                    deadline = time.monotonic() + 10
                    while time.monotonic() < deadline:
                        probe = self.sql(
                            "SELECT pid FROM pg_stat_activity WHERE "
                            "application_name = 'neon_dsg_crash_probe' AND state = 'active'"
                        ).stdout.strip()
                        if probe:
                            backend = int(probe)
                            break
                        time.sleep(0.05)
                    else:
                        self.fail("crash probe backend did not start")
                    os.kill(backend, signal.SIGKILL)
                    victim.communicate(timeout=15)
                finally:
                    if victim.poll() is None:
                        victim.kill()
                        victim.communicate(timeout=15)
            deadline = time.monotonic() + 30
            last = None
            while time.monotonic() < deadline:
                result = self.sql("SELECT dsg_boot()", check=False)
                last = result.stdout.strip()
                if result.returncode == 0 and last and last != boot:
                    break
                time.sleep(0.1)
            else:
                self.fail(
                    f"shared memory boot identity did not change: {last}\n{self.log.read_text()[-12000:]}"
                )
            self.assertEqual(
                int((self.data / "postmaster.pid").read_text().splitlines()[0]), postmaster
            )
            self.assertEqual(self.sql("SELECT dsg_layout()").stdout.strip(), "t")
            self.assertEqual(
                self.sql(
                    "SELECT count(*), sum(id), md5(string_agg(payload, ',' ORDER BY id)) FROM dsg_data"
                ).stdout,
                before,
            )
        self.sql("CREATE EXTENSION amcheck; SELECT bt_index_check('dsg_data_pkey', true)")

    def test_invalid_maximum_is_rejected_before_start(self):
        self.config("max_shared_buffers = '1MB'")
        self.assertNotEqual(self.start(check=False).returncode, 0)
        self.assertIn(
            "max_shared_buffers must not be less than shared_buffers", self.log.read_text()
        )

    def test_explicit_huge_pages_are_rejected(self):
        self.config("huge_pages = on")
        self.assertNotEqual(self.start(check=False).returncode, 0)
        self.assertIn(
            "dynamic shared buffers do not yet support explicit huge pages", self.log.read_text()
        )

    def test_disabled_uses_original_buffer_pool(self):
        self.config("enable_dynamic_shared_buffers = off\nmax_shared_buffers = '1MB'")
        self.start()
        self.install_functions()
        self.assertEqual(self.sql("SELECT dsg_boot() IS NULL").stdout.strip(), "t")
        self.assertEqual(self.sql("SHOW shared_buffers").stdout.strip(), "2MB")


if __name__ == "__main__":
    unittest.main()
