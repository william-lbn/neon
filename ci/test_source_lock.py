"""用独立本地 Git 仓库验证源码锁拒绝错误绑定，不访问网络。"""

import json
import subprocess
import tempfile
import unittest
from pathlib import Path

from source_lock import validate_postgres_sources


class SourceLockTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="neon-source-lock-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "ci").mkdir()
        (self.root / "vendor").mkdir()
        self.git("init", "-q")
        self.commit = "a" * 40
        self.lock = {"postgres": {"v17": self.commit}}
        self.revisions = {"v17": ["17.11", self.commit]}
        self.write_manifests()
        self.git("add", "ci/distribution.json", "vendor/revisions.json")
        self.git(
            "update-index", "--add", "--cacheinfo", "160000", self.commit, "vendor/postgres-v17"
        )
        self.git(
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        )

    def git(self, *arguments):
        return subprocess.check_output(["git", *arguments], cwd=self.root, text=True).strip()

    def write_manifests(self):
        (self.root / "ci/distribution.json").write_text(json.dumps(self.lock))
        (self.root / "vendor/revisions.json").write_text(json.dumps(self.revisions))

    def test_head_and_index_accept_matching_source(self):
        self.assertEqual(validate_postgres_sources(self.root), self.lock["postgres"])
        self.assertEqual(validate_postgres_sources(self.root, "index"), self.lock["postgres"])

    def test_changed_index_does_not_silently_use_old_head(self):
        self.git("update-index", "--cacheinfo", "160000", "b" * 40, "vendor/postgres-v17")
        validate_postgres_sources(self.root)
        with self.assertRaisesRegex(ValueError, "differs from gitlink"):
            validate_postgres_sources(self.root, "index")

    def test_stale_revision_manifest_is_rejected(self):
        self.revisions["v17"][1] = "b" * 40
        self.write_manifests()
        with self.assertRaisesRegex(ValueError, "revision manifest differs"):
            validate_postgres_sources(self.root)

    def test_short_sha_is_rejected(self):
        self.lock["postgres"]["v17"] = "a" * 12
        self.write_manifests()
        with self.assertRaisesRegex(ValueError, "full commit SHA"):
            validate_postgres_sources(self.root)

    def test_missing_checkout_does_not_resolve_parent_repo(self):
        (self.root / "vendor/postgres-v17").mkdir()
        with self.assertRaisesRegex(ValueError, "not initialized"):
            validate_postgres_sources(self.root, require_checkout=True)

    def make_checkout(self):
        checkout = self.root / "vendor/postgres-v17"
        checkout.mkdir()
        subprocess.check_call(["git", "init", "-q", str(checkout)])
        (checkout / "configure.ac").write_text("test source\n")
        subprocess.check_call(["git", "-C", str(checkout), "add", "configure.ac"])
        subprocess.check_call(
            [
                "git",
                "-C",
                str(checkout),
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "source",
            ]
        )
        commit = subprocess.check_output(
            ["git", "-C", str(checkout), "rev-parse", "HEAD"], text=True
        ).strip()
        return checkout, commit

    def test_wrong_checkout_is_rejected(self):
        self.make_checkout()
        with self.assertRaisesRegex(ValueError, "checkout differs"):
            validate_postgres_sources(self.root, require_checkout=["v17"])

    def test_actual_checkout_must_match_and_be_clean(self):
        checkout, commit = self.make_checkout()
        self.lock["postgres"]["v17"] = commit
        self.revisions["v17"][1] = commit
        self.write_manifests()
        self.git("update-index", "--cacheinfo", "160000", commit, "vendor/postgres-v17")
        validate_postgres_sources(self.root, "index", ["v17"])
        (checkout / "configure.ac").write_text("uncommitted source\n")
        with self.assertRaisesRegex(ValueError, "uncommitted source"):
            validate_postgres_sources(self.root, "index", ["v17"])


if __name__ == "__main__":
    unittest.main()
