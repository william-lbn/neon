"""核对 fork 的 PG gitlink、发行锁与实际检出，防止独立源码未接入 Neon 构建。"""

import json
import re
import subprocess
from pathlib import Path


def validate_postgres_sources(root: Path, source_ref="HEAD", require_checkout=False):
    lock = json.loads((root / "ci/distribution.json").read_text())["postgres"]
    revisions = json.loads((root / "vendor/revisions.json").read_text())
    # 并行PG版本可以尚未进入默认发行矩阵，但其已登记gitlink仍须准确。
    # 返回值保持发行矩阵不变，不因新增v18引用自动激活所有版本构建。
    pins = dict(lock)
    for version, revision in revisions.items():
        if not re.fullmatch(r"v[0-9]+", version) or not isinstance(revision, list) or len(revision) != 2:
            raise ValueError("Postgres revision manifest has an invalid parallel source")
        pins.setdefault(version, revision[1])
    paths = [f"vendor/postgres-{version}" for version in pins]
    # 本地提交前检查 index；CI 默认检查已提交的 HEAD，避免读错未提交 gitlink。
    if source_ref == "index":
        command = ["git", "ls-files", "--stage", "--", *paths]
    else:
        command = ["git", "ls-tree", source_ref, "--", *paths]
    tracked = subprocess.check_output(command, cwd=root, text=True)
    entries = {}
    for line in tracked.splitlines():
        metadata, path = line.split("\t", 1)
        fields = metadata.split()
        if source_ref == "index" and fields[2] != "0":
            raise ValueError(f"PG submodule has an unresolved index conflict: {path}")
        if fields[0] == "160000":
            entries[path] = fields[1] if source_ref == "index" else fields[2]

    for version, commit in pins.items():
        path = f"vendor/postgres-{version}"
        if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-f]{40}", commit):
            raise ValueError(f"PG source must use a full commit SHA: {version}")
        if entries.get(path) != commit:
            raise ValueError(f"Postgres lock differs from gitlink: {version}")
        if (
            version not in revisions
            or len(revisions[version]) != 2
            or revisions[version][1] != commit
        ):
            raise ValueError(f"Postgres revision manifest differs from lock: {version}")
        if require_checkout and (require_checkout is True or version in require_checkout):
            checkout = root / path
            # 缺少 .git 时 git -C 会向父目录查找，必须先拒绝，不能误认成 Neon HEAD。
            if not (checkout / ".git").exists():
                raise ValueError(f"Postgres submodule is not initialized: {version}")
            actual = subprocess.check_output(
                ["git", "-C", str(checkout), "rev-parse", "HEAD"], text=True
            ).strip()
            if actual != commit:
                raise ValueError(f"Postgres checkout differs from pinned commit: {version}")
            if subprocess.run(
                ["git", "-C", str(checkout), "diff", "--quiet", "HEAD", "--"]
            ).returncode:
                raise ValueError(f"Postgres checkout has uncommitted source changes: {version}")
    return lock
