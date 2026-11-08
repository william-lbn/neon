#!/usr/bin/env python3
"""统一候选发行的运行身份、跨仓源码锁和最终发布资产检查。"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

import compute
import release

ROOT = Path(__file__).resolve().parents[1]


def verify_attempt(tag, arch, allow_legacy=False):
    if release.DSGA_VERSION.fullmatch(tag):
        if arch != "amd64":
            raise ValueError("This unified Helm candidate supports amd64 only")
        return release.validate_current_execution(tag)
    if allow_legacy:
        # 旧主线仍可使用原日期标签；失败 job 单独重跑必须拒绝写旧 attempt 标签。
        run_id, attempt = os.environ.get("GITHUB_RUN_ID", ""), os.environ.get("GITHUB_RUN_ATTEMPT", "")
        if re.fullmatch(r"[1-9][0-9]*", run_id) and re.fullmatch(r"[1-9][0-9]*", attempt):
            if re.fullmatch(rf"[0-9]{{4}}\.[0-9]{{2}}\.[0-9]{{2}}-[0-9]{{6}}-[a-f0-9]{{12}}-r{run_id}-a{attempt}", tag):
                return None
    raise ValueError("Immutable version does not belong to this run/attempt; rerun all jobs")


def verify_workflow_pins(lock):
    text = (ROOT / ".github/workflows/dsga-release.yml").read_text()
    required = [
        ("william-lbn/autoscaling", "dockerhub.yml", lock["autoscaling_commit"]),
        ("william-lbn/neon-helm", "dsga-package.yml", lock["helm_commit"]),
        ("william-lbn/postgres", "build.yml", lock["postgres"]["v17"]),
        ("william-lbn/postgres", "build.yml", lock["postgres"]["v18"]),
    ]
    for repo, workflow, sha in required:
        expected = f"uses: {repo}/.github/workflows/{workflow}@{sha}"
        if expected not in text:
            raise ValueError(f"Reusable workflow differs from pinned source: {repo}/{workflow}/{sha}")


def prepare(arch):
    if arch != "amd64":
        raise ValueError("Unified DSGA Chart templates currently support amd64 only")
    lock = release.LOCK
    release.validate_lock(lock, "dsga")
    compute.validate()
    verify_workflow_pins(lock)
    version = f"0.2.0-dsga.{os.environ.get('GITHUB_RUN_ID')}.{os.environ.get('GITHUB_RUN_ATTEMPT')}.{arch}"
    verify_attempt(version, arch)
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if actual != os.environ.get("GITHUB_SHA"):
        raise ValueError("Orchestrator checkout differs from the event commit")
    outputs = {
        "tag": version, "source_commit": actual,
        "autoscaling": lock["autoscaling_commit"], "helm": lock["helm_commit"],
        "pg17": lock["postgres"]["v17"], "pg18": lock["postgres"]["v18"],
        "postgres_matrix": json.dumps(compute.postgres_matrix(), separators=(",", ":")),
        "binaries": json.dumps({"binary": lock["core_binaries"]}, separators=(",", ":")),
        "compute_components": json.dumps({"include": [{"name": k, "binary": v} for k, v in lock["compute_components"].items()]}, separators=(",", ":")),
        "distribution_artifact": f"dsga-distribution-{version}",
    }
    with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
        for key, value in outputs.items():
            stream.write(f"{key}={value}\n")
    print(f"Pinned candidate {version}; PostgreSQL profiles: {lock['postgres_profiles']}")


def finalize_assets(directory, tag, source_commit):
    """只在 images、同 run 原生门禁和 Helm 包都完成后形成一次原子 Release 提交。"""
    verify_attempt(tag, "amd64")
    path = Path(directory)
    manifest = json.loads((path / "distribution-manifest.json").read_text())
    if manifest.get("neon_commit") != source_commit or manifest.get("version") != tag:
        raise ValueError("Release assets do not match the exact source/version")
    validated = release.make_manifest(manifest["images"], release.LOCK, tag, "amd64", source_commit, source_commit,
                                      "dsga", manifest.get("autoscaling_capabilities"), manifest.get("postgres_native_validation"))
    for key in ("source_lock", "qualification", "chart_version", "release_tier", "vm_system_mode", "execution"):
        if manifest.get(key) != validated[key]:
            raise ValueError(f"Release asset contract differs: {key}")
    raw_manifest = (path / "distribution-manifest.json").read_bytes()
    manifest_hash = hashlib.sha256(raw_manifest).hexdigest()
    receipt = json.loads((path / "helm-package-receipt.json").read_text())
    if (receipt.get("release_version") != tag or receipt.get("helm_commit") != release.LOCK["helm_commit"]
            or receipt.get("distribution_manifest_sha256") != manifest_hash
            or receipt.get("qualification") != manifest["qualification"] or receipt.get("deployed") is not False):
        raise ValueError("Helm package receipt is not bound to this manifest/source/candidate")
    if (path / "autoscaling-capabilities.json").exists():
        if json.loads((path / "autoscaling-capabilities.json").read_text()) != manifest["autoscaling_capabilities"]:
            raise ValueError("Standalone Autoscaling capabilities differ from the validated manifest")
    charts = sorted(path.glob("*.tgz"))
    if not charts:
        raise ValueError("No packaged Helm Charts; cannot publish a unified release")
    # Helm 工作流已经执行 lint/template 和包内锁检查。此处再次校验包名/版本及
    # 全部资产 hash；最终 Release 在完整资产上传前保持 draft，绝不提升 latest。
    expected_charts = {item["file"]: item for item in receipt.get("charts", [])}
    if set(expected_charts) != {chart.name for chart in charts} or len(expected_charts) != len(receipt.get("charts", [])):
        raise ValueError("Helm chart asset set differs from the successful package receipt")
    for chart in charts:
        if not chart.name.endswith(f"-{tag}.tgz"):
            raise ValueError(f"Chart version differs from the image release: {chart.name}")
        item = expected_charts[chart.name]
        if (item.get("sha256") != hashlib.sha256(chart.read_bytes()).hexdigest()
                or item.get("lint") != "pass" or item.get("offline_render") != "pass"):
            raise ValueError(f"Chart package differs from its validation receipt: {chart.name}")
    notes = f"""# Neon DSGA candidate {tag}

统一源码、镜像与 Helm 候选发行，平台 linux/amd64。

- Neon 提交：{source_commit}
- PostgreSQL 17：{release.LOCK['postgres']['v17']}
- PostgreSQL 18：{release.LOCK['postgres']['v18']}
- Autoscaling：{release.LOCK['autoscaling_commit']}
- Helm：{release.LOCK['helm_commit']}
- 镜像数量：{len(manifest['images'])}；Chart 数量：{len(charts)}。
- 同 run 运行 PG17/18 原生内核回归、镜像源码/digest/架构校验、真实 WAL 装载、SQL/存储恢复 smoke、Helm lint/template/打包门禁。
- PG14–17 保留 full profile；PG18 为 dsga-core，包含 PostgreSQL、contrib 与 Neon 必需扩展，不包含尚未联合适配的第三方全扩展集。

本包使用 legacy VM system mode 与公开默认内核构建，production_ready=false，physical_memory_release_qualified=false。代码中最近完成的伸缩实现已纳入准确 SHA，但本候选物料没有 normal-split-v3 内核、构建计划及实例资格证明；真实物理缩容和三节点验收不由此 CI 自动授予。Chart 保持资格门禁关闭。本发布不修改任何 latest 标签。

部署以 distribution-manifest.json 的 image@sha256 引用及配套 Chart/values 为准。文件完整性见 SHA256SUMS；最终运行证据：{release.execution_identity(tag)['run_url']}。
"""
    (path / "RELEASE_NOTES.md").write_text(notes)
    entries = []
    for item in sorted(path.iterdir()):
        if item.name == "SHA256SUMS":
            continue
        if item.is_symlink() or not item.is_file() or not re.fullmatch(r"[A-Za-z0-9_.-]+", item.name):
            raise ValueError(f"Unexpected release asset path: {item.name}")
        entries.append(f"{hashlib.sha256(item.read_bytes()).hexdigest()}  {item.name}")
    (path / "SHA256SUMS").write_text("\n".join(entries) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    plan = sub.add_parser("prepare")
    plan.add_argument("--arch", required=True)
    attempt = sub.add_parser("verify-attempt")
    attempt.add_argument("--tag", required=True)
    attempt.add_argument("--arch", required=True)
    attempt.add_argument("--allow-legacy", action="store_true")
    assets = sub.add_parser("assets")
    assets.add_argument("--directory", required=True)
    assets.add_argument("--tag", required=True)
    assets.add_argument("--source-commit", required=True)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.arch)
    elif args.command == "verify-attempt":
        verify_attempt(args.tag, args.arch, args.allow_legacy)
    else:
        finalize_assets(args.directory, args.tag, args.source_commit)


if __name__ == "__main__":
    main()
