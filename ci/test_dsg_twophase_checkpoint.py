"""真实2PC文件短写/崩溃、检查点与备份恢复；只操作本测试的临时集群。

故障库按ci/fixtures/twophase_short_write.c单独编译，显式用
NEON_TEST_TWOPHASE_FAULT_LIBRARY传入；不污染全局DYLD/LD_PRELOAD或生产配置。
NEON_TEST_TWOPHASE_EXPECT_OLD_FAILURE=1只供已保留旧准确构建的反向复现，
新功能分支默认必须恢复成功。物理断电/存储持久性仍由Linux目标环境验收。
"""
import json
import os
import platform
import subprocess
import time
import unittest
from pathlib import Path

import test_dsg_postgres as lifecycle


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要显式PG及两个配套库")
class DsgTwoPhaseCheckpoint(unittest.TestCase):
    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        self.pg.config("max_prepared_transactions=4\ncheckpoint_timeout='1h'\n")
        self.process = None

    def start_injected(self):
        fault = Path(os.environ["NEON_TEST_TWOPHASE_FAULT_LIBRARY"]).resolve(strict=True)
        self.marker = self.pg.root / "short-write.txt"
        env = dict(os.environ)
        env.update(NEON_TEST_TWOPHASE_FAULT_DIR=str((self.pg.data / "pg_twophase").resolve()),
                   NEON_TEST_TWOPHASE_FAULT_MARKER=str(self.marker))
        if platform.system() == "Darwin":
            env["DYLD_INSERT_LIBRARIES"] = str(fault)
        elif platform.system() == "Linux":
            env["LD_PRELOAD"] = str(fault)
        else:
            self.fail("故障库仅支持明确的Darwin/Linux，不将未运行伪装为通过")
        # pg_ctl可能通过受系统保护的shell启动并清理DYLD变量；直接exec测试PG，
        # 显式保存真实Postmaster句柄，析构只停止本临时data对应进程。
        with self.pg.log.open("ab") as log:
            self.process = subprocess.Popen([str(self.pg.pg_bin / "postgres"), "-D", str(self.pg.data)],
                                            env=env, stdout=log, stderr=subprocess.STDOUT)
        self.addCleanup(self.stop_injected)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            self.assertIsNone(self.process.poll(), self.pg.log.read_text()[-12000:])
            if self.pg.sql("SELECT 1", check=False).returncode == 0:
                self.pg.running = True
                return
            time.sleep(0.02)
        self.fail("本测试注入集群未就绪\n" + self.pg.log.read_text()[-12000:])

    def stop_injected(self):
        if self.process is not None and self.process.poll() is None:
            self.pg.command("pg_ctl", "-D", self.pg.data, "-m", "immediate", "-w", "stop", check=False)
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        self.pg.running = False

    def prepare(self):
        self.pg.sql("CREATE TABLE two_phase_checkpoint_data(i int)")
        self.pg.sql("BEGIN; INSERT INTO two_phase_checkpoint_data VALUES(42); "
                    "PREPARE TRANSACTION 'checkpoint-original'")
        self.assertEqual(self.pg.sql("SELECT count(*) FROM pg_prepared_xacts").stdout.strip(), "1")

    def check_recovered(self, commit):
        self.assertEqual(self.pg.sql("SELECT count(*) FROM pg_prepared_xacts WHERE gid='checkpoint-original'").stdout.strip(), "1")
        self.assertEqual(self.pg.sql("SELECT count(*) FROM two_phase_checkpoint_data").stdout.strip(), "0")
        self.pg.install_functions()
        self.assertEqual(self.pg.sql("SELECT dsg_layout()").stdout.strip(), "t")
        self.pg.sql(f"{'COMMIT' if commit else 'ROLLBACK'} PREPARED 'checkpoint-original'")
        self.assertEqual(self.pg.sql("SELECT count(*) FROM pg_prepared_xacts").stdout.strip(), "0")
        self.assertEqual(self.pg.sql("SELECT count(*) FROM two_phase_checkpoint_data").stdout.strip(), "1" if commit else "0")

    @unittest.skipUnless(os.environ.get("NEON_TEST_TWOPHASE_FAULT_LIBRARY"),
                         "真实短写故障场景还需显式编译并指定独立故障库；不能将跳过计为通过")
    def test_real_short_write_then_immediate_crash_keeps_original_prepared(self):
        self.assertTrue(os.environ.get("NEON_TEST_TWOPHASE_FAULT_LIBRARY"), "必须真实装载故障库，不能省略注入仍算通过")
        self.start_injected()
        self.prepare()
        result = self.pg.sql("CHECKPOINT", check=False)
        self.assertNotEqual(result.returncode, 0, "必须真的触发2PC文件短写")
        self.assertTrue(self.marker.exists(), self.pg.log.read_text()[-12000:])
        pid, path, requested, actual = self.marker.read_text().splitlines()
        path = Path(path)
        self.assertGreater(int(pid), 1)
        self.assertEqual(path.parent, (self.pg.data / "pg_twophase").resolve())
        self.assertEqual(int(actual), int(requested) // 2)
        self.assertEqual(path.stat().st_size, int(actual), "须核实真实半文件，而非仅看注入器标记")
        old = os.environ.get("NEON_TEST_TWOPHASE_EXPECT_OLD_FAILURE") == "1"
        self.assertEqual(path.name.endswith(".tmp"), not old)
        self.stop_injected()
        restart = self.pg.start(check=False)
        evidence = os.environ.get("NEON_TEST_TWOPHASE_EVIDENCE")
        if evidence:
            Path(evidence).write_text(json.dumps({"old_expected_failure": old, "file": path.name,
                "requested": int(requested), "written": int(actual), "restart_returncode": restart.returncode,
                "postgres_log": self.pg.log.read_text()}, ensure_ascii=False, indent=2) + "\n")
        if old:
            self.assertNotEqual(restart.returncode, 0, "旧准确构建应真实复现半文件阻止启动")
            log = self.pg.log.read_text()
            # 旧debug/assert构建也可能先在“未来2PC文件”删除路径触发XID断言，
            # 不一定先走incorrect-size。必须是确定的2PC恢复路径，不能把任意
            # 启动失败都算反向证明；原短写文件及大小已在上面独立核验。
            corrupt_file = "incorrect size of file" in log and "pg_twophase/" + path.name in log
            future_assert = all(s in log for s in ["removing future two-phase state file", "TransactionIdPrecedesOrEquals", "restoreTwoPhaseData", "startup process failure"])
            self.assertTrue(corrupt_file or future_assert, log[-12000:])
        else:
            self.assertEqual(restart.returncode, 0, self.pg.log.read_text()[-12000:])
            self.assertFalse(path.exists(), "启动须移除本次失败.tmp，不能永久积压")
            self.check_recovered(commit=True)

    def test_checkpoint_file_survives_immediate_crash_and_explicit_rollback(self):
        self.pg.start()
        self.prepare()
        self.pg.sql("CHECKPOINT")
        names = [p.name for p in (self.pg.data / "pg_twophase").iterdir()]
        self.assertEqual(len(names), 1)
        self.assertRegex(names[0], r"^[0-9A-F]{16}$")
        self.pg.command("pg_ctl", "-D", self.pg.data, "-m", "immediate", "-w", "stop")
        self.pg.running = False
        self.pg.start()
        self.check_recovered(commit=False)

    def test_online_basebackup_restores_same_prepared_and_data(self):
        self.pg.start()
        self.prepare()
        backup = self.pg.root / "backup"
        self.pg.command("pg_basebackup", "-D", backup, "-h", self.pg.socket, "-p", "55432",
                        "-X", "fetch", "-c", "fast", "--no-password")
        self.assertEqual(len(list((backup / "pg_twophase").iterdir())), 1)
        self.pg.command("pg_ctl", "-D", self.pg.data, "-m", "fast", "-w", "stop")
        self.pg.running = False
        # 原集群已停止；同一私有socket/配置只用于这份备份，最后由既有cleanup清理。
        self.pg.data = backup
        self.pg.start()
        self.check_recovered(commit=True)


if __name__ == "__main__":
    unittest.main()
