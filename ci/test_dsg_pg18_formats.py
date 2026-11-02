#!/usr/bin/env python3
"""仅本批PG18真实control/WAL底层格式；不声称正式storage服务或时间线通过。"""
import hashlib, json, os, subprocess, tempfile, unittest
from pathlib import Path

class NativeFormats(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.bin = Path(os.environ["NEON_TEST_PG_BIN"])
        cls.reader = Path(os.environ["NEON_PG18_FORMAT_READER"])
        cls.evidence = Path(os.environ["NEON_PG18_EVIDENCE"]); cls.evidence.mkdir(parents=True, exist_ok=True)
        cls.temp = tempfile.TemporaryDirectory(prefix="pg18-formats-", dir="/tmp")
        cls.root = Path(cls.temp.name); cls.data = cls.root / "data"; cls.sock = cls.root / "s"; cls.sock.mkdir()
        cls.env = {k: v for k, v in os.environ.items() if not k.startswith("NEON_DSGA_")}
        def call(tool, *args):
            result = subprocess.run([str(cls.bin / tool), *map(str,args)], env=cls.env, capture_output=True, text=True, timeout=30)
            with (cls.evidence / (tool + ".log")).open("a") as log:
                log.write(result.stdout + result.stderr)
            if result.returncode: raise AssertionError((tool, result.returncode, result.stderr))
            return result.stdout.strip()
        cls.call = staticmethod(call)
        call("initdb", "-D", cls.data, "-U", "format_admin", "-A", "trust", "--no-locale", "--encoding=UTF8", "--no-sync")
        with (cls.data / "postgresql.conf").open("a") as f:
            f.write("listen_addresses=''\nport=55439\nunix_socket_directories='" + str(cls.sock) + "'\nshared_buffers='4MB'\nwal_level=logical\nmax_wal_senders=2\nhuge_pages=off\n")
        try:
            call("pg_ctl", "-D", cls.data, "-l", cls.root / "pg.log", "-w", "start")
            def sql(query): return call("psql", "-h", cls.sock, "-p", 55439, "-U", "format_admin", "-d", "postgres", "-At", "-v", "ON_ERROR_STOP=1", "-c", query)
            cls.version = sql("SHOW server_version_num")
            if not cls.version.startswith("18"): raise AssertionError(cls.version)
            sql("CREATE TABLE format_probe(i integer PRIMARY KEY,v text); INSERT INTO format_probe SELECT i,repeat('v18',40) FROM generate_series(1,400) i; SELECT pg_logical_emit_message(true,'pg18_formats','actual-v18-format'); CHECKPOINT")
            cls.end = sql("SELECT pg_current_wal_flush_lsn()")
            segment = sql("SELECT pg_walfile_name(pg_current_wal_flush_lsn())")
            cls.wal = cls.data / "pg_wal" / segment
            segment_no = int(segment[8:16],16) * 256 + int(segment[16:24],16)
            start = segment_no * 16 * 1024 * 1024
            cls.start = f"{start >> 32:X}/{start & 0xffffffff:X}"
        finally:
            call("pg_ctl", "-D", cls.data, "-m", "fast", "-w", "stop")
            if (cls.root / "pg.log").exists(): (cls.evidence / "postgres.log").write_bytes((cls.root / "pg.log").read_bytes())
        cls.control = cls.data / "global/pg_control"
        files = {str(p.relative_to(cls.data)): {"sha256": hashlib.sha256(p.read_bytes()).hexdigest(), "bytes":p.stat().st_size} for p in [cls.control,cls.wal]}
        original_control, original_wal = cls.control, cls.wal
        # 只保留本轮读者消费的真实前缀，避免归档整个数据目录/未消费WAL尾部。
        hi, lo = cls.end.split("/"); finish = (int(hi,16) << 32) | int(lo,16)
        cls.control = cls.evidence / "original-pg_control.bin"
        cls.wal = cls.evidence / "original-wal-interval.bin"
        cls.control.write_bytes(original_control.read_bytes())
        cls.wal.write_bytes(original_wal.read_bytes()[:finish-start])
        retained = {p.name:{"sha256":hashlib.sha256(p.read_bytes()).hexdigest(),"bytes":p.stat().st_size} for p in [cls.control,cls.wal]}
        (cls.evidence / "actual-PG-inputs.json").write_text(json.dumps({"server_version_num":cls.version,"start_lsn":cls.start,"flush_lsn":cls.end,"original_isolated_root":str(cls.root),"files":files,"retained_exact_inputs":retained,"formal_Pageserver":False,"Source":False,"E":0},indent=2)+"\n")

    @classmethod
    def tearDownClass(cls): cls.temp.cleanup()

    def read(self, label, *args):
        p = subprocess.run([str(self.reader),*map(str,args)],capture_output=True,text=True,timeout=30)
        (self.evidence / (label + ".stdout")).write_text(p.stdout)
        (self.evidence / (label + ".stderr")).write_text(p.stderr)
        self.assertEqual(p.returncode,0,p.stderr)
        return json.loads(p.stdout)

    def test_PG18_S_NATIVE_CONTROL_001_real_CRC_roundtrip_and_foreign_version_reject(self):
        out = self.read("control", "control", self.control)
        self.assertTrue(out["foreign_version_with_valid_CRC_negative"])
        self.assertEqual(out["control_version"],1800)

    def test_PG18_S_NATIVE_WAL_001_real_SQL_marker_decode_and_CRC_magic_reject(self):
        out = self.read("wal", "wal", self.wal, self.start, self.end)
        self.assertTrue(out["marker"])
        self.assertGreater(out["records"],0)

if __name__ == "__main__": unittest.main(verbosity=2)
