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
