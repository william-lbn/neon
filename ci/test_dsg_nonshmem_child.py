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
            f"CREATE FUNCTION dsg_region_recycle(text) RETURNS text AS '{lib}', "
            "'test_dsg_region_recycle' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;"
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
        for cycle in range(3):
            # 只杀此独占集群持久连接的BackendKeyData PID。三个真实新boot
            # 保持整个Postmaster寿命的VA/span，仍必须换boot和全新backing。
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
            self.assertEqual(before["arrays"], after["arrays"], "internal recovery must retain owned VA geometry")
            self.assertEqual(int((self.pg.data / "postmaster.pid").read_text().splitlines()[0]), self.postmaster)
            self.assertEqual(self.logger(), logger, "logger must really survive each shared boot change")
            self.verify(after, logger, f"after-crash-{cycle}")
            before = after

    def recycle(self, command):
        messages = self.peer.exchange([(b"Q", f"SELECT dsg_region_recycle('{self.pg.quote(command)}')".encode() + b"\0")])
        self.assertFalse(any(k == b"E" for k, _ in messages), messages)
        rows = [b for k, b in messages if k == b"D"]
        self.assertEqual(len(rows), 1)
        return json.loads(rows[0][6:])

    def test_owned_region_parks_without_access_then_rearms_fresh_at_same_va(self):
        first = self.recycle("reserve")
        (self.artifacts / "local-first.json").write_text(json.dumps(first, indent=2) + "\n")
        self.assertEqual(first["state"], "shared")
        self.assertIs(first["all_zero"], False, "test must really write old backing")
        start, size = int(first["address"]), int(first["mapped_bytes"])
        self.assertGreater(start, 0)
        self.assertGreater(size, 0)
        for cycle in range(3):
            live = self.maps(self.peer.backend_pid, f"local-live-{cycle}")
            # Mac单进程MAP_SHARED实际显示SM=PRV（原始行已留证），不能用
            # ALI/SHM推断未fork的单映射。这里核VA仍完整存在，真实C读写/清零
            # 证明访问性；四数组在父/backend两进程的共享覆盖仍由verify严格核验。
            present = [(left, right, True) for left, right, _ in live]
            self.assertTrue(self.covered(present, start, start + size), first)
            parked = self.recycle("park")
            (self.artifacts / f"local-parked-{cycle}.json").write_text(json.dumps(parked, indent=2) + "\n")
            self.assertEqual((parked["state"], parked["address"], parked["mapped_bytes"]),
                             ("parked", first["address"], first["mapped_bytes"]))
            self.assertIsNone(parked["all_zero"], "parked backing must never be read")
            label = f"local-parked-{cycle}"
            rows = self.maps(self.peer.backend_pid, label)
            self.assertFalse(any(shared and left < start + size and right > start
                                 for left, right, shared in rows))
            raw = (self.artifacts / f"{label}-{self.peer.backend_pid}.maps.txt").read_text()
            protected = []
            for line in raw.splitlines():
                if platform.system() == "Darwin":
                    match = re.search(r"([0-9a-fA-F]{8,16})-([0-9a-fA-F]{8,16}).*?\s([rwx-]{3})/[rwx-]{3}", line)
                else:
                    match = re.match(r"([0-9a-f]+)-([0-9a-f]+)\s+([rwx-]{3})[ps]", line)
                if match:
                    protected.append((int(match[1], 16), int(match[2], 16), match[3] == "---"))
            self.assertTrue(self.covered(protected, start, start + size), "real OS must report entire PROT_NONE span")
            fresh = self.recycle("rearm")
            (self.artifacts / f"local-fresh-{cycle}.json").write_text(json.dumps(fresh, indent=2) + "\n")
            self.assertEqual((fresh["state"], fresh["address"], fresh["mapped_bytes"]),
                             ("shared", first["address"], first["mapped_bytes"]))
            self.assertIs(fresh["all_zero"], True, "new anonymous shared backing must not retain old marker")
            self.assertIs(self.recycle("mark")["all_zero"], False)
        # 直接销毁不可访问占位也必须完整脱离；对应非共享子进程继承Guard的底层清理。
        self.assertEqual(self.recycle("park")["state"], "parked")
        self.assertEqual(self.recycle("destroy")["state"], "empty")
        self.verify(self.fact(), self.logger(), "after-local-lifecycle")

    def test_invalid_owned_mapping_geometry_is_fatal_before_any_replacement(self):
        before = self.fact()
        failed = self.pg.sql("SELECT dsg_region_recycle('invalid')", check=False)
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("FATAL", failed.stderr)
        self.assertIn("invalid owned DSGA mapping geometry", failed.stderr)
        self.assertEqual(before, self.fact(), "invalid local geometry must not alter parent/directory/boot")
        self.verify(before, self.logger(), "after-invalid-local-geometry")
