#!/usr/bin/env python3
"""记录不可变镜像，并在所有源码、profile 和候选资格一致后生成发行清单。"""
import argparse
import datetime
import json
import os
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
LOCK = json.loads((ROOT / "ci/distribution.json").read_text())
SHA = re.compile(r"[a-f0-9]{40}")
DIGEST = re.compile(r"sha256:[a-f0-9]{64}")
DSGA_VERSION = re.compile(r"0\.2\.0-dsga\.([1-9][0-9]*)\.([1-9][0-9]*)\.amd64")


def execution_identity(tag):
    match = DSGA_VERSION.fullmatch(tag)
    if match is None:
        raise ValueError("Invalid immutable DSGA release version")
    run_id, attempt = match.groups()
    return {"repository": "william-lbn/neon", "run_id": run_id, "run_attempt": attempt,
            "run_url": f"https://github.com/william-lbn/neon/actions/runs/{run_id}/attempts/{attempt}"}


def validate_current_execution(tag):
    expected = execution_identity(tag)
    for key, env in [("repository", "GITHUB_REPOSITORY"), ("run_id", "GITHUB_RUN_ID"), ("run_attempt", "GITHUB_RUN_ATTEMPT")]:
        if os.environ.get(env) != expected[key]:
            raise ValueError("Version belongs to another run/attempt; rerun all jobs")
    return expected


def load_images(directory):
    images = {}
    for path in sorted(pathlib.Path(directory).rglob("*.json")):
        item = json.loads(path.read_text())
        if not isinstance(item, dict) or item.get("kind") != "runtime":
            continue
        name = item["image"].split("/")[-1].split(":")[0]
        if name in images and images[name] != item:
            raise ValueError(f"Conflicting image records: {name}")
        images[name] = item
    return images


def expected_images(lock=None):
    lock = LOCK if lock is None else lock
    profiles = lock.get("postgres_profiles", {v: "full" for v in lock["postgres"]})
    images = {"neon", "neon-build-tools"}
    images |= {b.replace("_", "-") for b in lock["core_binaries"]}
    images |= set(lock["compute_components"]) | set(lock["autoscaling_images"])
    for version in lock["postgres"]:
        images |= {f"compute-node-{version}", f"vm-compute-node-{version}"}
        if profiles[version] == "full":
            images.add(f"neon-test-extensions-{version}")
    return images


def validate_lock(lock, channel):
    for key in ("autoscaling_commit",):
        if not isinstance(lock.get(key), str) or not SHA.fullmatch(lock[key]):
            raise ValueError(f"Invalid full source commit: {key}")
    if lock.get("registry") != "docker.io/williamluckyli":
        raise ValueError("Unexpected distribution registry")
    if lock.get("autoscaling_repository") != "william-lbn/autoscaling":
        raise ValueError("Unexpected Autoscaling source repository")
    if lock.get("postgres_repository") != "william-lbn/postgres":
        raise ValueError("Unexpected PostgreSQL source repository")
    for version, commit in lock["postgres"].items():
        if not re.fullmatch(r"v[0-9]+", version) or not SHA.fullmatch(commit):
            raise ValueError("PostgreSQL sources must be full pinned commits")
    if channel != "dsga":
        return
    if lock.get("helm_repository") != "william-lbn/neon-helm" or not SHA.fullmatch(lock.get("helm_commit", "")):
        raise ValueError("Missing pinned Helm source repository/commit")
    profiles = lock.get("postgres_profiles", {})
    if set(profiles) != set(lock["postgres"]):
        raise ValueError("PostgreSQL profiles do not cover the exact source lock")
    if profiles != {"v14": "full", "v15": "full", "v16": "full", "v17": "full", "v18": "dsga-core"}:
        raise ValueError("Unsupported PostgreSQL profile matrix for this candidate pipeline")


def verify_records(images, lock, tag, arch, neon_commit, channel):
    """纯校验函数供负向契约测试复用；任何版本/来源/digest 漂移均先拒绝。"""
    validate_lock(lock, channel)
    if not SHA.fullmatch(neon_commit):
        raise ValueError("Invalid Neon source commit")
    if channel == "dsga" and (arch != "amd64" or not DSGA_VERSION.fullmatch(tag)):
        raise ValueError("DSGA candidate requires the immutable amd64 SemVer tag")
    expected = expected_images(lock)
    if set(images) != expected:
        raise ValueError(f"Distribution incomplete. Missing={sorted(expected - images.keys())}; extra={sorted(images.keys() - expected)}")
    for name, item in images.items():
        if item.get("image") != f"{lock['registry']}/{name}:{tag}":
            raise ValueError(f"Image is outside this release: {name}")
        if not DIGEST.fullmatch(item.get("digest", "")):
            raise ValueError(f"Invalid digest: {name}")
        autoscaling = name in lock["autoscaling_images"]
        expected_commit = lock["autoscaling_commit"] if autoscaling else neon_commit
        if item.get("source_commit") != expected_commit:
            raise ValueError(f"Image source is not the locked commit: {name}")
        if channel == "dsga":
            if item.get("name") != name or item.get("kind") != "runtime":
                raise ValueError(f"Image record name/kind differs from manifest key: {name}")
            expected_repo = lock["autoscaling_repository"] if autoscaling else "william-lbn/neon"
            if item.get("source_repository") != expected_repo:
                raise ValueError(f"Wrong image source repository: {name}")
            if item.get("architecture") != f"linux/{arch}":
                raise ValueError(f"Image architecture differs from release: {name}")
            if item.get("execution") != execution_identity(tag):
                raise ValueError(f"Image comes from another workflow run/attempt: {name}")


def verify_capabilities(capabilities, lock, tag, arch, images):
    if not isinstance(capabilities, dict) or capabilities.get("kind") != "distribution-capabilities":
        raise ValueError("Missing actual Autoscaling build capabilities")
    required = {
        "source_repository": lock["autoscaling_repository"],
        "source_commit": lock["autoscaling_commit"],
        "version": tag,
        "architecture": f"linux/{arch}",
        "release_channel": "candidate",
        "normal_qualification": False,
        "production_ready": False,
        "vm_builder_default_system_mode": "legacy",
        "latest_eligible": False,
        "execution": execution_identity(tag),
    }
    for key, expected in required.items():
        if capabilities.get(key) != expected or (isinstance(expected, bool) and capabilities.get(key) is not expected):
            raise ValueError(f"Candidate capability mismatch: {key}")
    kernel = capabilities.get("kernel", {})
    for key in ("artifact_sha256", "config_sha256", "notes_sha256"):
        if not re.fullmatch(r"[a-f0-9]{64}", kernel.get(key, "")):
            raise ValueError(f"Missing actual kernel build identity: {key}")
    expected_digests = {name: images[name]["digest"] for name in lock["autoscaling_images"]}
    if capabilities.get("image_digests") != expected_digests:
        raise ValueError("Autoscaling capability image digests differ from release records")
    expected_kernel = images["vm-kernel"]["image"] + "@" + images["vm-kernel"]["digest"]
    if kernel.get("image") != expected_kernel:
        raise ValueError("Kernel capability is not bound to the released vm-kernel image")
    if kernel.get("normal_qualification") is not False or kernel.get("normal_metadata_guard") is not False:
        raise ValueError("The public kernel profile cannot grant normal physical qualification")
    return capabilities


def load_native_receipts(directory):
    receipts = {}
    for path in pathlib.Path(directory).rglob("source-manifest.json"):
        item = json.loads(path.read_text())
        version = f"v{item.get('pg_major')}"
        if version in receipts:
            raise ValueError(f"Conflicting/duplicate native PG receipt: {version}")
        checksums = path.with_name("postgres.sha256").read_text().split()
        if not checksums or not re.fullmatch(r"[a-f0-9]{64}", checksums[0]):
            raise ValueError("Native PG gate has no binary digest")
        item["postgres_binary_sha256"] = checksums[0]
        receipts[version] = item
    return receipts


def verify_native_receipts(receipts, lock, tag):
    if not isinstance(receipts, dict) or set(receipts) != {"v17", "v18"}:
        raise ValueError("Both native PostgreSQL gates are required")
    execution = execution_identity(tag)
    for version, item in receipts.items():
        expected = {"kind": "postgres-native-validation", "source_repository": "william-lbn/postgres",
                    "source_commit": lock["postgres"][version], "pg_major": int(version[1:]),
                    "scope": "native-kernel-not-vm", "normal_qualification": False, "production_ready": False,
                    "run_id": execution["run_id"], "run_attempt": execution["run_attempt"],
                    "execution_repository": execution["repository"]}
        for key, value in expected.items():
            if item.get(key) != value or (isinstance(value, bool) and item.get(key) is not value):
                raise ValueError(f"Native PG receipt mismatch: {version}/{key}")
        if not re.fullmatch(r"[a-f0-9]{64}", item.get("postgres_binary_sha256", "")):
            raise ValueError("Native PG receipt has an invalid binary digest")
    return receipts


def make_manifest(images, lock, tag, arch, neon_commit, verification_commit, channel, capabilities=None, native_receipts=None):
    verify_records(images, lock, tag, arch, neon_commit, channel)
    output = {"version": tag, "architecture": "linux/" + arch,
        "created_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "neon_commit": neon_commit, "verification_commit": verification_commit,
        "source_lock": lock, "images": dict(sorted(images.items()))}
    if channel == "dsga":
        if verification_commit != neon_commit:
            raise ValueError("DSGA validation must use the exact build source")
        output.update({
            "schema_version": 2, "chart_version": tag, "channel": "dsga",
            "release_tier": "candidate", "vm_system_mode": "legacy",
            "qualification": {"production_ready": False, "physical_memory_release_qualified": False},
            "autoscaling_capabilities": verify_capabilities(capabilities, lock, tag, arch, images),
            "postgres_native_validation": verify_native_receipts(native_receipts, lock, tag),
            "execution": execution_identity(tag),
        })
    return output


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    record = sub.add_parser("record")
    for name in ["name", "image", "digest", "output"]:
        record.add_argument("--" + name, required=True)
    record.add_argument("--arch", choices=["amd64", "arm64"], default=os.environ.get("ARCH", "amd64"))
    lookup = sub.add_parser("lookup")
    lookup.add_argument("--directory", default="artifacts")
    lookup.add_argument("--name", required=True)
    manifest = sub.add_parser("manifest")
    manifest.add_argument("--directory", default="artifacts")
    manifest.add_argument("--tag", required=True)
    manifest.add_argument("--arch", choices=["amd64", "arm64"], required=True)
    manifest.add_argument("--output", required=True)
    manifest.add_argument("--source-commit")
    manifest.add_argument("--channel", choices=["legacy", "dsga"], default="legacy")
    manifest.add_argument("--capabilities")
    manifest.add_argument("--native-receipts")
    args = parser.parse_args()
    if args.command == "record":
        if not DIGEST.fullmatch(args.digest):
            raise ValueError("Missing or invalid image digest")
        if args.image.split("/")[-1].split(":")[0] != args.name:
            raise ValueError("Image name differs from record name")
        execution = None
        tag = args.image.rsplit(":", 1)[-1]
        if DSGA_VERSION.fullmatch(tag):
            execution = validate_current_execution(tag)
        path = pathlib.Path(args.output)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps({"kind": "runtime", "name": args.name, "image": args.image,
            "digest": args.digest, "source_repository": "william-lbn/neon",
            "architecture": f"linux/{args.arch}", "execution": execution,
            "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()}, indent=2) + "\n")
        return
    images = load_images(args.directory)
    if args.command == "lookup":
        item = images[args.name]
        print(item["image"] + "@" + item["digest"])
        return
    verification_commit = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    neon_commit = args.source_commit or verification_commit
    if not SHA.fullmatch(neon_commit):
        raise ValueError("Invalid source commit")
    source_lock = json.loads(subprocess.check_output(["git", "show", neon_commit + ":ci/distribution.json"], text=True))
    if source_lock != LOCK:
        raise ValueError("Validation source lock differs from the image build source")
    capabilities = json.loads(pathlib.Path(args.capabilities).read_text()) if args.capabilities else None
    native_receipts = load_native_receipts(args.native_receipts) if args.native_receipts else None
    if args.channel == "dsga":
        validate_current_execution(args.tag)
    output = make_manifest(images, LOCK, args.tag, args.arch, neon_commit, verification_commit, args.channel, capabilities, native_receipts)
    pathlib.Path(args.output).write_text(json.dumps(output, indent=2) + "\n")
    print(f"Verified complete distribution: {len(images)} images; channel={args.channel}")


if __name__ == "__main__":
    main()
