#!/usr/bin/env python3
"""Split the upstream full compute build without dropping extensions.

Each completed stage is imported by digest. Extension jobs export installed files
and the original regression sources instead of uploading compiler scratch files.
The final image still uses the upstream EXTENSIONS=all assembly and runtime.
"""
import argparse
import json
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
LOCK = json.loads((ROOT / "ci/distribution.json").read_text())
SOURCE = ROOT / "compute/compute-node.Dockerfile"
COMMON = [
    "build-deps", "pg-build", "build-deps-with-cargo", "pg-build-with-cargo",
    "rust-extensions-build", "rust-extensions-build-pgrx12",
    "rust-extensions-build-pgrx14", "compute-tools", "neon-ext-build",
]
FROM = re.compile(r"^FROM\s+(.+?)(?:\s+AS\s+([\w-]+))?\s*$", re.I | re.M)


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
    header, blocks = stages()
    images = records(args.artifacts)
    output = ["# syntax=docker/dockerfile:1.7\n", header]
    for name, block in blocks.items():
        if (args.pg, name) in images and name != args.target:
            output.append(f"FROM {images[args.pg, name]} AS {name}\n")
            output.append("ARG PG_VERSION\nARG DEBIAN_VERSION\nARG BUILD_TAG\nARG TARGETARCH\nARG BUILD_JOBS=2\n")
            continue
        if name == "runtime":
            block = FROM.sub(lambda m: m.group(0) + " AS runtime", block, count=1)
        block = FROM.sub(lambda m: m.group(0) + "\nARG BUILD_JOBS=2", block, count=1)
        block = block.replace("$(nproc)", '"$BUILD_JOBS"')
        block = block.replace("$(getconf _NPROCESSORS_ONLN)", '"$BUILD_JOBS"')
        block = block.replace("--default-toolchain stable", "--default-toolchain 1.88.0")
        if name == "build-deps":
            block = FROM.sub(lambda m: m.group(0) + "\nENV CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_DEBUG=0", block, count=1)
        if name == "compute-tools":
            block = FROM.sub(lambda m: m.group(0) + "\nARG GIT_VERSION", block, count=1)
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
    item = {"kind": "compute-stage" if args.stage else "runtime", "pg": args.pg,
            "aliases": aliases, "image": args.image, "digest": digest,
            "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()}
    path = pathlib.Path(args.output)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(item, indent=2) + "\n")


def validate():
    tracked = subprocess.check_output(["git", "ls-tree", "HEAD", "vendor/"], cwd=ROOT, text=True)
    for version, commit in LOCK["postgres"].items():
        if f"160000 commit {commit}\tvendor/postgres-{version}" not in tracked:
            raise ValueError(f"Postgres lock differs from gitlink: {version}")
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
    print(f"Validated {len(LOCK['core_binaries'])} binaries, 4 pinned PG versions, {len(targets)} extension stages")


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("validate")
    sub.add_parser("matrix")
    sub.add_parser("common")
    rendering = sub.add_parser("render")
    for name in ["pg", "target", "output"]:
        rendering.add_argument("--" + name, required=True)
    rendering.add_argument("--artifacts", default="artifacts")
    rendering.add_argument("--payload", action="store_true")
    recording = sub.add_parser("record")
    for name in ["pg", "target", "image", "metadata", "output"]:
        recording.add_argument("--" + name, required=True)
    recording.add_argument("--stage", action="store_true")
    args = parser.parse_args()
    if args.command == "validate":
        validate()
    elif args.command == "matrix":
        print(json.dumps({"target": extension_targets()}, separators=(",", ":")))
    elif args.command == "common":
        print(" ".join(COMMON))
    elif args.command == "render":
        render(args)
    else:
        record(args)


if __name__ == "__main__":
    main()
