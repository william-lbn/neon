"""在真实 PG17 中检查 DSGA 启动、共享内存重建和数据恢复。

NEON_TEST_PG_BIN 指向待测二进制；NEON_TEST_DSG_LIBRARY 指向 PG 测试模块
test_dsg_region，NEON_TEST_RMGR_LIBRARY 指向配套 neon_rmgr 的完整库路径。
仅创建独立临时集群，不连接已有开发数据库。
这里验证 Mac 可执行的生命周期，不声称验证 Linux 物理回收。
"""

import os
import signal
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

_REQUIRED_ENV = ("NEON_TEST_PG_BIN", "NEON_TEST_DSG_LIBRARY", "NEON_TEST_RMGR_LIBRARY")
_CONFIGURED_ENV = [name for name in _REQUIRED_ENV if os.environ.get(name)]
if _CONFIGURED_ENV and len(_CONFIGURED_ENV) != len(_REQUIRED_ENV):
    raise RuntimeError("生命周期测试环境不完整，必须同时提供 " + ", ".join(_REQUIRED_ENV))


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

    def start(self, *, check=True):
        result = self.command("pg_ctl", "-D", self.data, "-l", self.log, "-w", "start", check=check)
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
