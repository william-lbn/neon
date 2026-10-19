"""用实际 Make 规则验证 PG 版本锁变更会失效 configure 缓存，不编译模拟数据库。"""

import json
import os
import shutil
import subprocess
import tempfile
import time
import unittest
from pathlib import Path


class PostgresConfigCacheTests(unittest.TestCase):
    def test_revision_change_reconfigures_and_unchanged_build_reuses_cache(self):
        with tempfile.TemporaryDirectory(prefix="neon-pg-config-") as directory:
            root = Path(directory)
            source = root / "vendor/postgres-v17"
            source.mkdir(parents=True)
            rule = root / "postgres.mk"
            rule.write_text((Path(__file__).resolve().parents[1] / "postgres.mk").read_text())
            revision = root / "vendor/revisions.json"
            revision.write_text(json.dumps({"v17": ["17.11", "1" * 40]}))

            # 只替代高成本的 configure 可执行文件，依赖判断使用被测的完整
            # postgres.mk。输出实际参数和调用次数，能发现旧 SHA 被缓存的问题。
            configure = source / "configure"
            configure.write_text(
                "#!/bin/sh\n"
                'printf "%s\\n" "$@" > config.status\n'
                'printf "configured\\n" >> configure-runs\n'
            )
            configure.chmod(0o755)
            os.utime(rule, (1, 1))
            os.utime(revision, (1, 1))
            build = root / "build"
            status = build / "v17/config.status"
            command = [
                shutil.which("make") or "make",
                "-f",
                str(rule),
                f"ROOT_PROJECT_DIR={root}/",
                f"BUILD_DIR={build}",
                "POSTGRES_VERSIONS=v17",
                str(status),
            ]

            def run_make():
                result = subprocess.run(
                    command, cwd=root, text=True, capture_output=True, timeout=20
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

            run_make()
            self.assertIn("1" * 40, status.read_text())
            self.assertEqual(
                (status.parent / "configure-runs").read_text().splitlines(), ["configured"]
            )
            run_make()
            self.assertEqual(
                (status.parent / "configure-runs").read_text().splitlines(), ["configured"]
            )

            revision.write_text(json.dumps({"v17": ["17.11", "2" * 40]}))
            # 明确模拟缓存早于新锁；兼容只比较秒级时间戳的旧版 Make。
            earlier = time.time() - 5
            os.utime(status, (earlier, earlier))
            run_make()
            self.assertIn("2" * 40, status.read_text())
            self.assertNotIn("1" * 40, status.read_text())
            self.assertEqual(
                (status.parent / "configure-runs").read_text().splitlines(),
                ["configured", "configured"],
            )


if __name__ == "__main__":
    unittest.main()
