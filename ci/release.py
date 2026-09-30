#!/usr/bin/env python3
"""Record and verify a complete distribution before promoting any latest tags."""
import argparse
import datetime
import json
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
LOCK = json.loads((ROOT / "ci/distribution.json").read_text())


def load_images(directory):
    images = {}
    for path in sorted(pathlib.Path(directory).rglob("*.json")):
        item = json.loads(path.read_text())
        if item.get("kind") != "runtime":
            continue
        name = item["image"].split("/")[-1].split(":")[0]
        if name in images and images[name] != item:
            raise ValueError(f"Conflicting image records: {name}")
        images[name] = item
    return images


def expected_images():
    return {"neon", "neon-build-tools"} | {b.replace("_", "-") for b in LOCK["core_binaries"]} | set(LOCK["compute_components"]) | set(LOCK["autoscaling_images"]) | {
        f"{prefix}-{version}" for version in LOCK["postgres"]
        for prefix in ["compute-node", "vm-compute-node", "neon-test-extensions"]
    }


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    record = sub.add_parser("record")
    for name in ["name", "image", "digest", "output"]:
        record.add_argument("--" + name, required=True)
    lookup = sub.add_parser("lookup")
    lookup.add_argument("--directory", default="artifacts")
    lookup.add_argument("--name", required=True)
    manifest = sub.add_parser("manifest")
    manifest.add_argument("--directory", default="artifacts")
    manifest.add_argument("--tag", required=True)
    manifest.add_argument("--arch", required=True)
    manifest.add_argument("--output", required=True)
    args = parser.parse_args()
    if args.command == "record":
        if not re.fullmatch(r"sha256:[a-f0-9]{64}", args.digest):
            raise ValueError("Missing or invalid image digest")
        path = pathlib.Path(args.output)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps({"kind": "runtime", "name": args.name, "image": args.image,
            "digest": args.digest, "source_repository": "william-lbn/neon",
            "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()}, indent=2) + "\n")
        return
    images = load_images(args.directory)
    if args.command == "lookup":
        item = images[args.name]
        print(item["image"] + "@" + item["digest"])
        return
    expected = expected_images()
    if set(images) != expected:
        raise ValueError(f"Distribution incomplete. Missing={sorted(expected - images.keys())}; extra={sorted(images.keys() - expected)}")
    neon_commit = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    for name, item in images.items():
        if not item["image"].startswith(LOCK["registry"] + "/") or not item["image"].endswith(":" + args.tag):
            raise ValueError(f"Image is outside this release: {name}")
        if not re.fullmatch(r"sha256:[a-f0-9]{64}", item["digest"]):
            raise ValueError(f"Invalid digest: {name}")
        expected_commit = LOCK["autoscaling_commit"] if name in LOCK["autoscaling_images"] else neon_commit
        if item["source_commit"] != expected_commit:
            raise ValueError(f"Image source is not the locked commit: {name}")
    output = {"version": args.tag, "architecture": "linux/" + args.arch,
        "created_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "neon_commit": neon_commit,
        "source_lock": LOCK, "images": dict(sorted(images.items()))}
    pathlib.Path(args.output).write_text(json.dumps(output, indent=2) + "\n")
    print(f"Verified complete distribution: {len(images)} images")


if __name__ == "__main__":
    main()
