"""在真实 PostgreSQL 进程中验证 Neon WAL 重放，不依赖 Pageserver/mock。

运行前设置 NEON_TEST_PG_BIN 和 NEON_TEST_RMGR_LIBRARY。使用独立临时集群，
仅监听私有 Unix socket；immediate 停机用于真正触发 crash recovery。
该测试补充 Neon 端到端测试，不替代 Pageserver、RO 或 Linux VM 验收。
"""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path


class NeonRmgrRecovery(unittest.TestCase):
    def setUp(self):
        self.target_pg_bin = Path(os.environ["NEON_TEST_PG_BIN"]).resolve()
        self.target_rmgr = Path(os.environ["NEON_TEST_RMGR_LIBRARY"]).resolve()
        self.pg_bin = Path(os.environ.get("NEON_TEST_LEGACY_PG_BIN", self.target_pg_bin)).resolve()
        self.rmgr = Path(
            os.environ.get("NEON_TEST_LEGACY_RMGR_LIBRARY", self.target_rmgr)
        ).resolve()
        self.temp = tempfile.TemporaryDirectory(prefix="neon-rmgr-")
        self.root = Path(self.temp.name)
        self.data = self.root / "data"
        self.socket = self.root / "socket"
        self.socket.mkdir()
        self.log = self.root / "postgres.log"
        self.running = False
        try:
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
            # 不监听 TCP，避免碰到开发者已有的服务；socket 目录由本测试独占。
            with (self.data / "postgresql.conf").open("a") as config:
                config.write("\nlisten_addresses = ''\n")
                config.write("port = 55432\n")
                config.write(f"unix_socket_directories = '{self.quote(self.socket)}'\n")
                config.write(f"shared_preload_libraries = '{self.quote(self.rmgr)}'\n")
                config.write("wal_consistency_checking = 'neon'\n")
                config.write("max_wal_size = '2GB'\n")
            self.start()
        except BaseException:
            self.cleanup_cluster()
            raise

    @staticmethod
    def quote(value):
        return str(value).replace("'", "''")

    def command(self, binary, *arguments):
        result = subprocess.run(
            [str(self.pg_bin / binary), *map(str, arguments)],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=180,
        )
        if result.returncode:
            log = self.log.read_text() if self.log.exists() else ""
            raise AssertionError(f"{binary} failed: {result.stdout}\n{log[-12000:]}")
        return result.stdout.strip()

    def start(self):
        self.command("pg_ctl", "-D", self.data, "-l", self.log, "-w", "start")
        self.running = True

    def sql(self, query):
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
        )

    def crash_and_restart(self):
        # 不做正常 checkpoint 的停机，确保测试的是 WAL redo 而非干净启动。
        self.command("pg_ctl", "-D", self.data, "-w", "-m", "immediate", "stop")
        self.running = False
        # 升级场景由旧二进制产生 WAL，再由新二进制和新 rmgr 做 crash recovery。
        # 每项测试独立建库，避免第一项升级后其余测试无意中只覆盖新格式。
        self.pg_bin = self.target_pg_bin
        with (self.data / "postgresql.conf").open("a") as config:
            config.write(f"shared_preload_libraries = '{self.quote(self.target_rmgr)}'\n")
        self.start()

    def cleanup_cluster(self):
        try:
            if self.running:
                self.command("pg_ctl", "-D", self.data, "-w", "-m", "immediate", "stop")
        finally:
            self.running = False
            self.temp.cleanup()

    def tearDown(self):
        self.cleanup_cluster()

    def test_01_insert_delete_lock_and_multi_insert_recover(self):
        self.sql(
            "CREATE TABLE small (id int PRIMARY KEY, value text); "
            "INSERT INTO small SELECT i, repeat('x', 100) FROM generate_series(1, 500) i;"
        )
        self.sql("VACUUM (FREEZE, ANALYZE) small")
        self.sql("CHECKPOINT")
        # 分别触发 INSERT、DELETE、LOCK、COPY 的自定义记录；COPY 使用批量插入。
        self.sql(
            "INSERT INTO small VALUES (501, 'insert'); DELETE FROM small WHERE id = 1; "
            "SELECT id FROM small WHERE id = 2 FOR UPDATE;"
        )
        result = subprocess.run(
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
                "COPY small FROM STDIN",
            ],
            input="502\tcopy-a\n503\tcopy-b\n",
            text=True,
            capture_output=True,
            timeout=60,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        before = self.sql(
            "SELECT count(*), sum(id), md5(string_agg(value, ',' ORDER BY id)) FROM small"
        )
        self.crash_and_restart()
        self.assertEqual(
            self.sql(
                "SELECT count(*), sum(id), md5(string_agg(value, ',' ORDER BY id)) FROM small"
            ),
            before,
        )

    def test_02_update_shared_vm_page_recovers_both_clears(self):
        self.sql(
            "CREATE TABLE same_vm (id int PRIMARY KEY, value text); "
            "ALTER TABLE same_vm ALTER COLUMN value SET STORAGE PLAIN; "
            "INSERT INTO same_vm SELECT i, repeat('x',100) FROM generate_series(1,500) i;"
        )
        self.sql("VACUUM (FREEZE, ANALYZE) same_vm")
        self.sql("CHECKPOINT")
        old_block = int(
            self.sql(
                "SELECT split_part(trim(both '()' from ctid::text), ',', 1) FROM same_vm WHERE id=2"
            )
        )
        self.sql("UPDATE same_vm SET value = repeat('y', 3000) WHERE id IN (2, 3)")
        new_block = int(
            self.sql(
                "SELECT split_part(trim(both '()' from ctid::text), ',', 1) FROM same_vm WHERE id=2"
            )
        )
        heap_blocks_per_vm_page = (int(self.sql("SHOW block_size")) - 24) * 4
        self.assertNotEqual(old_block, new_block, "必须触发跨 heap 页 UPDATE")
        self.assertEqual(old_block // heap_blocks_per_vm_page, new_block // heap_blocks_per_vm_page)
        before = self.sql("SELECT md5(string_agg(value, ',' ORDER BY id)) FROM same_vm")
        self.crash_and_restart()
        self.assertEqual(
            self.sql("SELECT md5(string_agg(value, ',' ORDER BY id)) FROM same_vm"), before
        )

    def test_03_update_distinct_vm_pages_recovers(self):
        # 关闭 TOAST 压缩/外置，确保超过一张 VM 页覆盖的 heap 范围。
        self.sql(
            "CREATE TABLE wide (id int PRIMARY KEY, value text); "
            "ALTER TABLE wide ALTER COLUMN value SET STORAGE PLAIN; "
            "INSERT INTO wide SELECT i, repeat('a', 6000) FROM generate_series(1, 40000) i;"
        )
        self.sql("VACUUM (FREEZE, ANALYZE) wide")
        self.sql("CHECKPOINT")
        old_block = int(
            self.sql(
                "SELECT split_part(trim(both '()' from ctid::text), ',', 1) FROM wide WHERE id=1"
            )
        )
        self.sql("UPDATE wide SET value=repeat('b', 6500) WHERE id=1")
        new_block = int(
            self.sql(
                "SELECT split_part(trim(both '()' from ctid::text), ',', 1) FROM wide WHERE id=1"
            )
        )
        block_size = int(self.sql("SHOW block_size"))
        heap_blocks_per_vm_page = (block_size - 24) * 4
        self.assertNotEqual(
            old_block // heap_blocks_per_vm_page,
            new_block // heap_blocks_per_vm_page,
            "测试必须真正跨越两张 VM 页，不能提前跳过关键重放场景",
        )
        before = self.sql("SELECT count(*), sum(id), sum(length(value)) FROM wide")
        self.crash_and_restart()
        self.assertEqual(self.sql("SELECT count(*), sum(id), sum(length(value)) FROM wide"), before)
        self.assertEqual(self.sql("SELECT value=repeat('b',6500) FROM wide WHERE id=1"), "t")
        # 索引路径与堆结果同时检查，避免仅总数相等掩盖索引可见性错误。
        self.assertEqual(
            self.sql(
                "SET enable_seqscan=off; SELECT count(*) FROM wide WHERE id BETWEEN 1 AND 40000"
            ),
            "40000",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
