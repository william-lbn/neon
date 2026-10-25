"""真实logger/OS映射生命周期；不以RSS=0推断额外共享backing已脱离。

仅探测本次独占PG的Postmaster、持久协议连接和logger。Mac用系统vmmap，Linux
用/proc/maps；先证明父/业务backend确有完整四数组，才接受logger不持有的负证据。
这是进程映射引用验收，不证明Linux物理回收、整个预算或VM缩容。
"""
import json
import os
from pathlib import Path
import platform
import re
import signal
import subprocess
import time
import unittest

import test_dsg_postgres as lifecycle


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要显式真实PG及配套测试库")
class DsgNonShmemChildLifecycle(unittest.TestCase):
    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        self.pg.config("logging_collector=on\nlog_directory='owned-log'\n"
                       "log_filename='postgres.log'\n")
        self.pg.start()
        self.postmaster = int((self.pg.data / "postmaster.pid").read_text().splitlines()[0])
        lib = self.pg.quote(self.pg.library)
        self.pg.sql(
            f"CREATE FUNCTION dsg_array_mappings() RETURNS text AS '{lib}', "
            "'test_dsg_array_mappings' LANGUAGE C VOLATILE PARALLEL UNSAFE;"
            f"CREATE FUNCTION dsg_grow(bigint,bigint,bigint,int,int,boolean) RETURNS text "
            f"AS '{lib}', 'test_dsg_grow' LANGUAGE C STRICT;"
            "CREATE TABLE logger_data AS SELECT i,repeat(md5(i::text),4) AS v "
            "FROM generate_series(1,100) i; CHECKPOINT;")
        self.peer = self.connect()
        self.expected_data = self.pg.sql(
            "SELECT count(*),sum(i),md5(string_agg(v,',' ORDER BY i)) FROM logger_data").stdout
        base = os.environ.get("NEON_DSG_MAP_ARTIFACT_DIR")
        self.artifacts = Path(base) / self._testMethodName if base else self.pg.root / "maps"
        self.artifacts.mkdir(parents=True, exist_ok=True)

    def connect(self):
        peer = lifecycle.PgWireConnection(self.pg.socket, os.environ.get("USER", "williamlee"), 55432)
        self.addCleanup(peer.close)
        return peer

    def fact(self):
        messages = self.peer.exchange([(b"Q", b"SELECT dsg_array_mappings()\0")])
        self.assertFalse(any(k == b"E" for k, _ in messages), messages)
        rows = [b for k, b in messages if k == b"D"]
        self.assertEqual(len(rows), 1)
        fact = json.loads(rows[0][6:])
        self.assertEqual(fact["pid"], self.peer.backend_pid)
        self.assertEqual([a["array"] for a in fact["arrays"]], list(range(4)))
        self.assertTrue(all(int(a["address"]) > 0 and int(a["mapped_bytes"]) > 0
                            for a in fact["arrays"]))
        return fact

    def logger(self, *, other=None):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            for line in subprocess.check_output(["ps", "-axo", "pid=,ppid=,command="], text=True).splitlines():
                fields = line.strip().split(None, 2)
                if (len(fields) == 3 and fields[1] == str(self.postmaster)
                        and fields[2].startswith("postgres: logger") and int(fields[0]) != other):
                    return int(fields[0])
            time.sleep(0.01)
        self.fail("owned Postmaster has no live logger")

    def maps(self, pid, label):
        # 调用者只传自己创建且已确认出生关系的三种PID，不扫描其他应用映射。
        if platform.system() == "Darwin":
            result = subprocess.run(["/usr/bin/vmmap", "-interleaved", "-noCoalesce", "-w", str(pid)],
                                    text=True, capture_output=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            raw = result.stdout
            rows = []
            for line in raw.splitlines():
                span = re.search(r"([0-9a-fA-F]{8,16})-([0-9a-fA-F]{8,16})", line)
                if span:
                    # ALI是本次MAP_SHARED匿名对象的真实共享别名；PRV/COW重用
                    # 已空VA不能误判为仍持旧共享对象。先用父/业务进程正向覆盖验证。
                    shared = any(mode in line for mode in ("SM=ALI", "SM=SHM", "SM=TRUESHARED"))
                    rows.append((int(span[1], 16), int(span[2], 16), shared))
        elif platform.system() == "Linux":
            raw = Path(f"/proc/{pid}/maps").read_text()
            self.assertLess(len(raw), 16 * 1024 * 1024)
            rows = []
            for line in raw.splitlines():
                span = re.match(r"([0-9a-f]+)-([0-9a-f]+)\s+([rwxps-]{4})\s", line)
                self.assertIsNotNone(span, "unrecognized owned process map")
                rows.append((int(span[1], 16), int(span[2], 16), span[3][3] == "s"))
        else:
            self.fail("native mapping gate requires Mac vmmap or Linux procfs")
        self.assertGreater(len(rows), 10, "empty/unreadable map sample cannot prove absence")
        (self.artifacts / f"{label}-{pid}.maps.txt").write_text(raw)
        return rows

    @staticmethod
    def covered(rows, start, end):
        # 求共享区间并集而非RSS或重复相加；允许内核分裂/合并VMA。
        cursor = start
        for left, right, shared in sorted(rows):
            if shared and left <= cursor < right:
                cursor = min(end, right)
                if cursor == end:
                    break
        return cursor == end

    def verify(self, fact, logger, label):
        parent = self.maps(self.postmaster, label + "-parent")
        backend = self.maps(self.peer.backend_pid, label + "-backend")
        logs = self.maps(logger, label + "-logger")
        (self.artifacts / f"{label}-directory.json").write_text(json.dumps(fact, indent=2) + "\n")
        for a in fact["arrays"]:
            start, size = int(a["address"]), int(a["mapped_bytes"])
            end = start + size
            self.assertTrue(self.covered(parent, start, end), ("parent", a))
            self.assertTrue(self.covered(backend, start, end), ("backend", a))
            self.assertFalse(any(shared and left < end and right > start
                                 for left, right, shared in logs), ("logger still inherits DSG array", a))
        self.assertEqual(self.pg.sql(
            "SELECT count(*),sum(i),md5(string_agg(v,',' ORDER BY i)) FROM logger_data").stdout,
            self.expected_data, "local-copy detach must not clear parent/backend directory or data")

    def test_initial_logger_detaches_only_its_arrays(self):
        self.verify(self.fact(), self.logger(), "initial")

    def test_restarted_logger_after_real_grow_does_not_inherit_arrays(self):
        before = self.fact()
        old_logger = self.logger()
        self.assertIn("committed high=512 generation=2", self.pg.sql(
            "SELECT dsg_grow(1,701,1,512,2000,false)").stdout)
        after = self.fact()
        self.assertEqual(before["arrays"], after["arrays"], "Grow must preserve actual VA geometry")
        # 只终止此独占Postmaster当前仍拥有的logger，验证真实自动重建的fork路径。
        self.assertEqual(old_logger, self.logger())
        os.kill(old_logger, signal.SIGKILL)
        new_logger = self.logger(other=old_logger)
        self.assertEqual(before["boot_id"], self.fact()["boot_id"])
        self.verify(after, new_logger, "logger-restarted")

    def test_logger_survives_internal_pg_boot_without_stale_array_backing(self):
        before = self.fact()
        logger = self.logger()
        self.verify(before, logger, "before-crash")
        # 杀本次持久连接的真实BackendKeyData PID，触发同一Postmaster内部恢复。
        os.kill(self.peer.backend_pid, signal.SIGKILL)
        deadline = time.monotonic() + 12
        after = None
        while time.monotonic() < deadline:
            try:
                self.peer = self.connect()
                after = self.fact()
                if after["boot_id"] != before["boot_id"]:
                    break
            except (OSError, RuntimeError, StopIteration):
                pass
            time.sleep(0.01)
        self.assertIsNotNone(after)
        self.assertNotEqual(before["boot_id"], after["boot_id"])
        self.assertEqual(int((self.pg.data / "postmaster.pid").read_text().splitlines()[0]), self.postmaster)
        self.assertEqual(self.logger(), logger, "logger must really survive this shared boot change")
        # 旧、新目录的共享VA均不得由同一长寿命logger持有；不以新boot补零旧引用。
        logs = self.maps(logger, "surviving-logger-old-source")
        for a in before["arrays"]:
            start, end = int(a["address"]), int(a["address"]) + int(a["mapped_bytes"])
            self.assertFalse(any(shared and left < end and right > start for left, right, shared in logs))
        self.verify(after, logger, "after-crash")
