"""用实际 Make 规则验证 PG 版本锁变更会失效 configure 缓存，不编译模拟数据库。"""

import contextlib
import json
import os
import shutil
import subprocess
import tempfile
import time
import unittest
from pathlib import Path


class PostgresConfigCacheTests(unittest.TestCase):
    @contextlib.contextmanager
    def build_fixture(self):
        with tempfile.TemporaryDirectory(prefix="neon-pg-source-") as directory:
            root = Path(directory)
            source = root / "vendor/postgres-v17"
            source.mkdir(parents=True)
            rule = root / "postgres.mk"
            shutil.copyfile(Path(__file__).resolve().parents[1] / "postgres.mk", rule)
            revision = root / "vendor/revisions.json"
            configure = source / "configure"
            configure.write_text(r"""#!/bin/sh
printf '%s\n' "$@" > config.status
""")
            configure.chmod(0o755)
            build = root / "build"
            status = build / "v17/config.status"
            command = [
                shutil.which("make") or "make",
                "-f", str(rule),
                f"ROOT_PROJECT_DIR={root}",
                f"BUILD_DIR={build}",
                "POSTGRES_VERSIONS=v17",
                str(status),
            ]

            def run_make():
                return subprocess.run(command, cwd=root, text=True, capture_output=True, timeout=20)

            yield root, source, revision, status, run_make

    def init_repository(self, directory):
        def git(*args):
            return subprocess.check_output(
                ["git", "-C", str(directory), *args], text=True, stderr=subprocess.DEVNULL
            ).strip()

        git("init", "-q")
        git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
            "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-qm", "fixture")
        return git

    def test_export_under_neon_git_tree_uses_lock_not_parent_head(self):
        with self.build_fixture() as (root, _, revision, status, run_make):
            parent = self.init_repository(root)("rev-parse", "HEAD")
            pin = "a" * 40
            revision.write_text(json.dumps({"v17": ["17.11", pin]}))
            (root / "vendor/postgres-v17/.neon-source-revision").write_text(pin + "\n")
            result = run_make()
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn(pin, status.read_text())
            self.assertNotIn(parent, status.read_text())

    def test_rejects_wrong_postgres_head_before_reusing_configure_cache(self):
        with self.build_fixture() as (_, source, revision, status, run_make):
            git = self.init_repository(source)
            pin = git("rev-parse", "HEAD")
            revision.write_text(json.dumps({"v17": ["17.11", pin]}))
            result = run_make()
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            before = status.read_bytes()
            git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-qm", "wrong source")
            result = run_make()
            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertIn("does not match pinned revision", result.stderr)
            self.assertEqual(before, status.read_bytes())


    def test_export_receipt_must_match_locked_revision(self):
        with self.build_fixture() as (root, source, revision, status, run_make):
            revision.write_text(json.dumps({"v17": ["17.11", "a" * 40]}))
            (source / ".neon-source-revision").write_text("b" * 40 + "\n")
            result = run_make()
            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertIn("does not match pinned revision", result.stderr)
            self.assertFalse(status.exists())

    def test_rejects_malformed_source_lock(self):
        with self.build_fixture() as (_, _, revision, status, run_make):
            revision.write_text(json.dumps({"v17": ["17.11", "not-a-commit"]}))
            result = run_make()
            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertFalse(status.exists())

    def test_revision_change_reconfigures_and_unchanged_build_reuses_cache(self):
        with tempfile.TemporaryDirectory(prefix="neon-pg-config-") as directory:
            root = Path(directory)
            source = root / "vendor/postgres-v17"
            source.mkdir(parents=True)
            rule = root / "postgres.mk"
            rule.write_text((Path(__file__).resolve().parents[1] / "postgres.mk").read_text())
            revision = root / "vendor/revisions.json"
            revision.write_text(json.dumps({"v17": ["17.11", "1" * 40]}))
            receipt = source / ".neon-source-revision"
            receipt.write_text("1" * 40 + "\n")

            # 只替代高成本的 configure 可执行文件，依赖判断使用被测的完整
            # postgres.mk。输出实际参数和调用次数，能发现旧 SHA 被缓存的问题。
            configure = source / "configure"
            configure.write_text(
                "#!/bin/sh\n"
                'printf "%s\n" "$@" > config.status\n'
                'printf "configured\n" >> configure-runs\n'
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
            receipt.write_text("2" * 40 + "\n")
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
