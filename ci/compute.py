#!/usr/bin/env python3
"""Split the upstream full compute build without dropping extensions.

Each completed stage is imported by digest. Extension jobs export installed files
and the original regression sources instead of uploading compiler scratch files.
The final image still uses the upstream EXTENSIONS=all assembly and runtime.
"""
import argparse
import json
import os
import pathlib
import re
import subprocess

from source_lock import validate_postgres_sources
from release import DSGA_VERSION, validate_current_execution

ROOT = pathlib.Path(__file__).resolve().parents[1]
LOCK = json.loads((ROOT / "ci/distribution.json").read_text())
SOURCE = ROOT / "compute/compute-node.Dockerfile"
COMMON = [
    "build-deps", "pg-build", "build-deps-with-cargo", "pg-build-with-cargo",
    "rust-extensions-build", "rust-extensions-build-pgrx12",
    "rust-extensions-build-pgrx14", "compute-tools", "neon-ext-build",
]
FROM = re.compile(r"^FROM\s+(.+?)(?:\s+AS\s+([\w-]+))?\s*$", re.I | re.M)


def postgres_profile(pg):
    """发行范围属于源码锁；不把 PG18 核心包误报成全第三方扩展已适配。"""
    profiles = LOCK.get("postgres_profiles", {v: "full" for v in LOCK["postgres"]})
    if set(profiles) != set(LOCK["postgres"]):
        raise ValueError("Postgres profiles must cover exactly the pinned versions")
    profile = profiles.get(pg)
    if profile not in {"full", "dsga-core"}:
        raise ValueError(f"Unsupported Postgres distribution profile: {pg}")
    if (pg == "v18") != (profile == "dsga-core"):
        raise ValueError("PG18 requires dsga-core until its full extensions are qualified")
    return profile


def postgres_matrix():
    return {"include": [
        {"pg": pg, "major": int(pg[1:]), "debian": "bookworm" if int(pg[1:]) >= 17 else "bullseye",
         "profile": postgres_profile(pg)}
        for pg in LOCK["postgres"]
    ]}


def common_stages(pg):
    if pg is not None and postgres_profile(pg) == "dsga-core":
        # 核心 Neon 扩展仍真实编译；不执行只适配到 PG17 的第三方 pgrx 构建。
        return [name for name in COMMON if not name.startswith("rust-extensions-build")]
    return COMMON


def runtime_targets(pg):
    return ["runtime", "extension-tests"] if postgres_profile(pg) == "full" else ["runtime"]


def stages():
    text = SOURCE.read_text()
    matches = list(FROM.finditer(text))
    blocks = {}
    for i, match in enumerate(matches):
        end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
        blocks[match.group(2) or "runtime"] = text[match.start():end]
    return text[:matches[0].start()], blocks


def extension_targets():
    _, blocks = stages()
    return list(dict.fromkeys(re.findall(r"^COPY\s+--from=([\w-]+)", blocks["extensions-all"], re.M)))


def records(directory):
    result = {}
    for path in sorted(pathlib.Path(directory).rglob("*.json")):
        item = json.loads(path.read_text())
        if item.get("kind") != "compute-stage":
            continue
        for alias in item["aliases"]:
            key = (item["pg"], alias)
            value = item["image"] + "@" + item["digest"]
            if key in result and result[key] != value:
                raise ValueError(f"Conflicting stage {key}: {path}")
            result[key] = value
    return result


def render(args):
    profile = postgres_profile(args.pg)
    if profile == "dsga-core" and (args.payload or args.target == "extension-tests"):
        raise ValueError("PG18 dsga-core has no full third-party extension/test image")
    header, blocks = stages()
    images = records(args.artifacts)
    output = ["# syntax=docker/dockerfile:1.7\n", header]
    for name, block in blocks.items():
        if (args.pg, name) in images and name != args.target:
            output.append(f"FROM {images[args.pg, name]} AS {name}\n")
            output.append("ARG PG_VERSION\nARG DEBIAN_VERSION\nARG TARGETARCH\nARG BUILD_JOBS=2\n")
            continue
        if name == "runtime":
            block = FROM.sub(lambda m: m.group(0) + " AS runtime", block, count=1)
        block = FROM.sub(lambda m: m.group(0) + "\nARG BUILD_JOBS=2", block, count=1)
        if name in {"build-deps", "pgbouncer", "runtime"}:
            block = FROM.sub(lambda m: m.group(0) + "\nARG DEBIAN_VERSION\nCOPY ci/apt-sources.sh /usr/local/bin/neon-apt-sources\nRUN bash /usr/local/bin/neon-apt-sources \"$DEBIAN_VERSION\"", block, count=1)
        block = block.replace("$(nproc)", '"$BUILD_JOBS"')
        block = block.replace("$(getconf _NPROCESSORS_ONLN)", '"$BUILD_JOBS"')
        block = block.replace("--default-toolchain stable", "--default-toolchain 1.88.0")
        if name == "build-deps":
            block = FROM.sub(lambda m: m.group(0) + "\nENV CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_DEBUG=0", block, count=1)
        if name == "compute-tools":
            block = FROM.sub(lambda m: m.group(0) + "\nARG GIT_VERSION", block, count=1)
        if name == "extension-tests" and args.pg in {"v14", "v15"}:
            # Upstream publishes this harness only for PG16/17 and provides no
            # pg_hint_plan test patches for the older versions. Keep their
            # original regression sources without referencing nonexistent files.
            block = block.replace("COPY compute/patches/pg_hint_plan_${PG_VERSION:?}.patch /ext-src\n", "")
            block = block.replace("RUN cd /ext-src/pg_hint_plan-src && patch -p1 < /ext-src/pg_hint_plan_${PG_VERSION:?}.patch\n", "")
        output.append(block)
    if args.payload:
        if args.target not in extension_targets():
            raise ValueError("Payload export is only valid for extension stages")
        output.append(f"\nFROM scratch AS payload\nCOPY --from={args.target} /usr/local/pgsql/ /usr/local/pgsql/\n")
        sources = re.findall(r"^COPY\s+--from=([\w-]+-src)", blocks[args.target], re.M)
        if len(sources) != 1:
            raise ValueError(f"Expected one extension source for {args.target}: {sources}")
        source = args.target if args.target == "postgis-build" else sources[0]
        output.append(f"COPY --from={source} /ext-src/ /ext-src/\n")
        if args.target == "postgis-build":
            output.append("COPY --from=postgis-build /sfcgal/ /sfcgal/\n")
        if args.target == "h3-pg-build":
            output.append("COPY --from=h3-pg-build /h3/ /h3/\n")
    pathlib.Path(args.output).write_text("".join(output))


def record(args):
    digest = json.loads(pathlib.Path(args.metadata).read_text())["containerimage.digest"]
    if not re.fullmatch(r"sha256:[a-f0-9]{64}", digest):
        raise ValueError("Missing or invalid image digest")
    _, blocks = stages()
    aliases = [args.target]
    if args.target in extension_targets():
        aliases += re.findall(r"^COPY\s+--from=([\w-]+-src)", blocks[args.target], re.M)
    tag = args.image.rsplit(":", 1)[-1]
    # 中间 stage 的 tag 还包含 stage 后缀；其原始发行版本由环境 TAG 传入。
    release_tag = os.environ.get("TAG", tag) if args.stage else tag
    execution = validate_current_execution(release_tag) if DSGA_VERSION.fullmatch(release_tag) else None
    item = {"kind": "compute-stage" if args.stage else "runtime", "pg": args.pg,
            "name": args.image.split("/")[-1].split(":")[0],
            "aliases": aliases, "image": args.image, "digest": digest,
            "source_repository": "william-lbn/neon",
            "architecture": "linux/" + args.arch, "execution": execution,
            "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()}
    path = pathlib.Path(args.output)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(item, indent=2) + "\n")


def validate(source_ref="HEAD", require_checkout=False):
    validate_postgres_sources(ROOT, source_ref, require_checkout)
    binary_text = (ROOT / "Dockerfile").read_text()
    for binary in LOCK["core_binaries"]:
        if f"--bin {binary}" not in binary_text:
            raise ValueError(f"Missing build for core binary {binary}")
        if f"target/release/{binary}" not in binary_text:
            raise ValueError(f"Missing runtime copy for {binary}")
    targets = extension_targets()
    _, blocks = stages()
    for name in COMMON + targets:
        if name not in blocks:
            raise ValueError(f"Unknown stage {name}")
    for pg in LOCK["postgres"]:
        postgres_profile(pg)
    print(f"Validated {len(LOCK['core_binaries'])} binaries, {len(LOCK['postgres'])} pinned PG versions, {len(targets)} full-profile extension stages")


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    validation = sub.add_parser("validate")
    validation.add_argument("--source-ref", default="HEAD", help="HEAD, another commit, or index before committing")
    validation.add_argument("--require-pg-checkout", action="append", default=[], metavar="VERSION")
    sub.add_parser("matrix")
    sub.add_parser("postgres-matrix")
    common = sub.add_parser("common")
    common.add_argument("--pg")
    runtime = sub.add_parser("runtime-targets")
    runtime.add_argument("--pg", required=True)
    profile = sub.add_parser("profile")
    profile.add_argument("--pg", required=True)
    rendering = sub.add_parser("render")
    for name in ["pg", "target", "output"]:
        rendering.add_argument("--" + name, required=True)
    rendering.add_argument("--artifacts", default="artifacts")
    rendering.add_argument("--payload", action="store_true")
    recording = sub.add_parser("record")
    for name in ["pg", "target", "image", "metadata", "output"]:
        recording.add_argument("--" + name, required=True)
    recording.add_argument("--stage", action="store_true")
    recording.add_argument("--arch", choices=["amd64", "arm64"], default=os.environ.get("ARCH", "amd64"))
    args = parser.parse_args()
    if args.command == "validate":
        validate(args.source_ref, args.require_pg_checkout)
    elif args.command == "matrix":
        print(json.dumps({"target": extension_targets()}, separators=(",", ":")))
    elif args.command == "postgres-matrix":
        print(json.dumps(postgres_matrix(), separators=(",", ":")))
    elif args.command == "common":
        print(" ".join(common_stages(args.pg)))
    elif args.command == "runtime-targets":
        print(" ".join(runtime_targets(args.pg)))
    elif args.command == "profile":
        print(postgres_profile(args.pg))
    elif args.command == "render":
        render(args)
    else:
        record(args)


if __name__ == "__main__":
    main()
