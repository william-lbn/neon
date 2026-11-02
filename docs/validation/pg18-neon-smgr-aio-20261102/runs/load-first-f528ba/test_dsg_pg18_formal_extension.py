#!/usr/bin/env python3
"""PG18正式Neon扩展首次组合：同一SQL合同，无PG版本业务分叉。

显式test-v1 native数组，不是Root出生/预算/normal/KVM/K8s资格。
仅运行这个新增组件目标，不导入或重复PG18已有核心测试矩阵。
"""
import json, os, signal, subprocess, tempfile, unittest
from pathlib import Path

BIN = Path(os.environ["NEON_TEST_PG_BIN"])
EVIDENCE = Path(os.environ["NEON_PG18_EVIDENCE"])
SQL = '\\set ON_ERROR_STOP on\nCREATE EXTENSION neon VERSION \'1.17\';\nCREATE TABLE dsg_data(i integer PRIMARY KEY,pad text);\nALTER TABLE dsg_data ALTER pad SET STORAGE PLAIN;\nINSERT INTO dsg_data SELECT i,repeat(md5(i::text),150) FROM generate_series(1,600) i;\nCREATE TEMP TABLE dsg_requests(rev integer PRIMARY KEY, request jsonb);\nCREATE TEMP TABLE dsg_evidence(label text PRIMARY KEY, value jsonb);\nINSERT INTO dsg_evidence VALUES(\'capabilities\',neon_dsg_capabilities()::jsonb);\nINSERT INTO dsg_evidence SELECT \'initial\',neon_dsg_memory_status()::jsonb;\nINSERT INTO dsg_evidence SELECT \'data_before\',jsonb_build_object(\'rows\',count(*),\'bytes\',pg_relation_size(\'dsg_data\'),\'checksum\',md5(string_agg(i::text||\':\'||pad,\',\' ORDER BY i))) FROM dsg_data;\nCREATE FUNCTION pg_temp.req(rev integer,gen integer,target integer,kind text) RETURNS jsonb LANGUAGE SQL AS $$\n SELECT jsonb_build_object(\'protocol_version\',1,\'key\',jsonb_build_object(\'instance\',neon_dsg_capabilities()::jsonb->\'instance\',\'issuer_epoch\',\'1\',\'revision\',rev::text,\'operation_id\',jsonb_build_object(\'high\',\'18364758544493064720\',\'low\',rev::text)),\'kind\',kind,\'expected_capacity_generation\',gen::text,\'target_buffers\',target,\'target_vm_memory_bytes\',\'1073741824\',\'budget_commitment_id\',\'linux-formal-sql-component-only\',\'deadline_unix_ms\',\'18446744073709551615\')\n$$;\nINSERT INTO dsg_requests VALUES(1,pg_temp.req(1,1,1024,\'grow\'));\nINSERT INTO dsg_evidence SELECT \'grow\',neon_dsg_request_resize(request::text)::jsonb FROM dsg_requests WHERE rev=1;\nINSERT INTO dsg_evidence SELECT \'data_after_grow\',jsonb_build_object(\'rows\',count(*),\'checksum\',md5(string_agg(i::text||\':\'||pad,\',\' ORDER BY i))) FROM dsg_data;\nINSERT INTO dsg_requests VALUES(2,pg_temp.req(2,2,256,\'shrink\'));\nINSERT INTO dsg_evidence SELECT \'shrink\',neon_dsg_request_resize(request::text)::jsonb FROM dsg_requests WHERE rev=2;\nINSERT INTO dsg_evidence SELECT \'data_after_shrink\',jsonb_build_object(\'rows\',count(*),\'checksum\',md5(string_agg(i::text||\':\'||pad,\',\' ORDER BY i))) FROM dsg_data;\nINSERT INTO dsg_requests VALUES(3,pg_temp.req(3,3,2048,\'grow\'));\nINSERT INTO dsg_evidence SELECT \'regrow\',neon_dsg_request_resize(request::text)::jsonb FROM dsg_requests WHERE rev=3;\nINSERT INTO dsg_evidence SELECT \'data_after_regrow\',jsonb_build_object(\'rows\',count(*),\'checksum\',md5(string_agg(i::text||\':\'||pad,\',\' ORDER BY i))) FROM dsg_data;\nINSERT INTO dsg_evidence SELECT \'replay\',neon_dsg_request_resize(request::text)::jsonb FROM dsg_requests WHERE rev=1;\nINSERT INTO dsg_evidence SELECT \'historical_status\',neon_dsg_resize_status(jsonb_build_object(\'protocol_version\',1,\'key\',request->\'key\')::text)::jsonb FROM dsg_requests WHERE rev=1;\nINSERT INTO dsg_evidence SELECT \'same_key_conflict\',neon_dsg_request_resize(jsonb_set(request,\'{target_buffers}\',\'512\'::jsonb)::text)::jsonb FROM dsg_requests WHERE rev=1;\nINSERT INTO dsg_evidence VALUES(\'expired\',neon_dsg_request_resize(jsonb_set(pg_temp.req(4,4,4096,\'grow\'),\'{deadline_unix_ms}\',\'"1"\'::jsonb)::text)::jsonb);\nINSERT INTO dsg_evidence SELECT \'final\',neon_dsg_memory_status()::jsonb;\nINSERT INTO dsg_evidence SELECT \'version\',jsonb_build_object(\'server_version_num\',current_setting(\'server_version_num\'),\'extension_version\',(SELECT extversion FROM pg_extension WHERE extname=\'neon\'));\nINSERT INTO dsg_evidence SELECT \'shared_cost\',neon_dsg_shared_cost()::jsonb;\nINSERT INTO dsg_evidence SELECT \'work\',neon_dsg_work_state()::jsonb;\nINSERT INTO dsg_evidence SELECT \'connections\',neon_dsg_connection_state()::jsonb;\nINSERT INTO dsg_evidence SELECT \'prepared\',neon_dsg_prepared_state()::jsonb;\nINSERT INTO dsg_evidence SELECT \'dsm\',neon_dsg_dsm_state()::jsonb;\nSELECT \'DSG_SQL_EVIDENCE \'||jsonb_build_object(\'case\',label,\'value\',value)::text FROM dsg_evidence ORDER BY label;\n'


class FormalExtension(unittest.TestCase):
    def test_PG18_S_NEON_FORMAL_SQL_001_shared_contract_resize_and_observers(self):
        EVIDENCE.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="pg18-neon-", dir="/tmp") as temp:
            root = Path(temp)
            data, sock = root / "data", root / "s"
            sock.mkdir()
            env = {k: v for k, v in os.environ.items() if not k.startswith("NEON_DSGA_")}
            def call(tool, *args, check=True, **kw):
                result = subprocess.run([str(BIN / tool), *map(str, args)], env=env,
                                        capture_output=True, text=True, timeout=30, **kw)
                if check and result.returncode:
                    raise AssertionError(result.stderr)
                return result
            init = call("initdb", "-D", data, "-U", "dsg_admin", "-A", "trust", "--no-locale", "--encoding=UTF8", "--data-checksums", "--no-sync")
            (EVIDENCE / "initdb.log").write_text(init.stdout + init.stderr)
            with (data / "postgresql.conf").open("a") as f:
                f.write("listen_addresses=''\nport=55438\nunix_socket_directories='" + str(sock) + "'\n"
                        "shared_preload_libraries='neon,neon_rmgr'\n"
                        "neon.last_written_lsn_cache_size=1024\n"
                        "neon.dsg_vm_uid='linux-pg18-formal-sql-component'\n"
                        "neon.dsg_compute_instance_id='isolated-functional-case'\n"
                        "enable_dynamic_shared_buffers=on\nshared_buffers='2MB'\nmax_shared_buffers='64MB'\n"
                        "io_method='worker'\nio_workers=2\nhuge_pages=off\nmax_prepared_transactions=2\n"
                        "bgwriter_lru_maxpages=0\ncheckpoint_timeout='1h'\n")
            # 此明示测试来源只用于内核/扩展组合；不修改或冒充原生产birth policy。
            launch_env = dict(env, NEON_DSGA_PG18_NATIVE_ARRAYS="test-v1:8192")
            with (EVIDENCE / "postgres.log").open("w") as stream:
                process = subprocess.Popen([str(BIN / "postgres"), "-D", str(data)], env=launch_env,
                                           stdout=stream, stderr=subprocess.STDOUT)
                try:
                    import time
                    deadline = time.monotonic() + 20
                    while time.monotonic() < deadline:
                        if process.poll() is not None:
                            self.fail((EVIDENCE / "postgres.log").read_text())
                        ready = call("pg_isready", "-h", sock, "-p", "55438", "-U", "dsg_admin", check=False)
                        if ready.returncode == 0:
                            break
                        time.sleep(.03)
                    else:
                        self.fail("PG18 formal extension startup timeout")
                    (EVIDENCE / "functional.sql").write_text(SQL)
                    result = call("psql", "-h", sock, "-p", "55438", "-U", "dsg_admin", "-d", "postgres",
                                  "-XqAt", "-v", "ON_ERROR_STOP=1", check=False, input=SQL)
                    (EVIDENCE / "sql.log").write_text(result.stdout)
                    (EVIDENCE / "sql.stderr").write_text(result.stderr)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    ev = {}
                    for line in result.stdout.splitlines():
                        if line.startswith("DSG_SQL_EVIDENCE "):
                            item = json.loads(line[len("DSG_SQL_EVIDENCE "):])
                            self.assertNotIn(item["case"], ev)
                            ev[item["case"]] = item["value"]
                    (EVIDENCE / "actual-formal-SQL-evidence.json").write_text(json.dumps(ev, indent=2) + "\n")
                    self.assertEqual(int(ev["version"]["server_version_num"]) // 10000, 18)
                    self.assertEqual(ev["version"]["extension_version"], "1.17")
                    cap = ev["capabilities"]
                    self.assertTrue(cap["supported"] and cap["enabled"] and cap["instance_bound"])
                    self.assertEqual(cap["buffer_api_version"], 6)
                    self.assertFalse(cap["production_ready"] or cap["vm_shrink_permit"])
                    self.assertEqual((ev["initial"]["observed"]["high_buffers"], ev["initial"]["observed"]["capacity_generation"]), (256, "1"))
                    checksum = ev["data_before"]["checksum"]
                    self.assertEqual(ev["data_before"]["rows"], 600)
                    self.assertGreater(ev["data_before"]["bytes"], 2 << 20)
                    for label, high, gen in [("grow", 1024, "2"), ("shrink", 256, "3"), ("regrow", 2048, "4")]:
                        value = ev[label]
                        self.assertEqual(value["receipt"]["disposition"], "accepted", label)
                        self.assertEqual((value["observed"]["high_buffers"], value["observed"]["capacity_generation"]), (high, gen))
                        self.assertEqual(len(value["observed"]["arrays"]), 4)
                        self.assertFalse(value["vm_shrink_permit"])
                        self.assertEqual(ev["data_after_" + label]["checksum"], checksum)
                        self.assertEqual(ev["data_after_" + label]["rows"], 600)
                    for label in ["replay", "historical_status"]:
                        value = ev[label]
                        self.assertEqual(value["receipt"]["disposition"], "replayed")
                        self.assertEqual(value["receipt"]["result"]["high_buffers"], 1024)
                        self.assertEqual(value["observed"]["high_buffers"], 2048)
                    self.assertEqual(ev["same_key_conflict"]["receipt"]["disposition"], "conflict")
                    self.assertEqual(ev["expired"]["receipt"]["disposition"], "rejected")
                    self.assertFalse(ev["expired"]["receipt"]["has_result"])
                    self.assertEqual(ev["work"]["work_api_version"], 2)
                    self.assertEqual(ev["connections"]["connection_api_version"], 1)
                    self.assertEqual(ev["prepared"]["prepared_api_version"], 1)
                    self.assertEqual(ev["dsm"]["dsm_api_version"], 1)
                    self.assertEqual(ev["prepared"]["prepared_slots"], 0)
                    self.assertEqual(ev["prepared"]["maximum_prepared_slots"], 2)
                    self.assertTrue(ev["dsm"]["accounting_valid"])
                    self.assertFalse(ev["dsm"]["limit_enforced"])
                    self.assertEqual(ev["dsm"]["external_budget_bytes"], "0")
                    self.assertEqual(ev["dsm"]["unknown_segments"], 0)
                    self.assertEqual(ev["dsm"]["pending_creations"], 0)
                    for label in ["shared_cost", "work", "connections", "prepared", "dsm"]:
                        self.assertFalse(ev[label]["vm_shrink_permit"], label)
                finally:
                    if process.poll() is None:
                        process.send_signal(signal.SIGINT)
                    try:
                        code = process.wait(timeout=15)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        code = process.wait(timeout=5)
                    (EVIDENCE / "postmaster-Wait.json").write_text(json.dumps({"PID": process.pid, "exit": code}) + "\n")
                    self.assertEqual(code, 0, (EVIDENCE / "postgres.log").read_text()[-8000:])


if __name__ == "__main__":
    unittest.main(verbosity=2)
