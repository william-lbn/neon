#!/usr/bin/env python3
"""从固定输入组装 normal Guest，生成离线 OCI 并独立读回最终根盘。"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

import dsga_release
import release


IMAGE_NAME = re.compile(r"[a-z0-9]+(?:[._-][a-z0-9]+)*(?::[0-9]+)?(?:/[a-z0-9]+(?:[._-][a-z0-9]+)*)*")
IMAGE_TAG = re.compile(r"[a-zA-Z0-9_][a-zA-Z0-9_.-]{0,127}")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"Duplicate JSON key {key}")
        result[key] = value
    return result


def read_json(raw):
    return json.loads(raw, object_pairs_hook=unique_object)


def pinned_sha(value, pattern):
    return isinstance(value, str) and bool(pattern.fullmatch(value)) and value != "0" * len(value)


def pinned_digest(value):
    return isinstance(value, str) and value.startswith("sha256:") and pinned_sha(value[7:], release.SHA256)


def read_inputs(path):
    value = read_json(path.read_bytes())
    images = ("base_image", "disk_tools_image", "kernel_image")
    files = ("builder_sha256", "kernel_sha256", "postgres_elf_sha256")
    sources = ("neon_source", "guest_autoscaling_source")
    if (not isinstance(value, dict) or set(value) != {*images, *files, *sources, "postgres_major"}
            or type(value["postgres_major"]) is not int or value["postgres_major"] not in (17, 18)):
        raise ValueError("Invalid normal Guest assembly inputs")
    for field in images:
        ref = value[field]
        name, separator, digest = ref.partition("@") if isinstance(ref, str) else ("", "", "")
        if not separator or not IMAGE_NAME.fullmatch(name) or not pinned_digest(digest):
            raise ValueError(f"Normal Guest {field} must use an immutable OCI digest")
    if (not all(pinned_sha(value[k], release.SHA256) for k in files)
            or not all(pinned_sha(value[k], release.SHA) for k in sources)):
        raise ValueError("Normal Guest files and source commits must be independently pinned")
    return value


def file_sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def inspect_manifest(reference):
    raw = subprocess.check_output(["docker", "buildx", "imagetools", "inspect", "--raw", reference])
    if reference.rsplit("@", 1)[1] != "sha256:" + hashlib.sha256(raw).hexdigest():
        raise ValueError("Normal Guest base manifest bytes differ from the requested digest")
    manifest = read_json(raw)
    if not isinstance(manifest, dict) or manifest.get("schemaVersion") != 2:
        raise ValueError("Invalid normal Guest base manifest")
    return manifest


def verify_base_provenance(inputs, postgres_source, package_lock_sha256):
    reference = inputs["base_image"]
    manifest = inspect_manifest(reference)
    if "manifests" in manifest:
        descriptors = manifest["manifests"]
        if not isinstance(descriptors, list):
            raise ValueError("Invalid normal Guest base manifest list")
        platforms = [
            descriptor.get("digest") for descriptor in descriptors
            if isinstance(descriptor, dict) and isinstance(descriptor.get("platform"), dict)
            and descriptor["platform"].get("os") == "linux" and descriptor["platform"].get("architecture") == "amd64"
        ]
        if len(platforms) != 1 or not pinned_digest(platforms[0]):
            raise ValueError("Normal Guest base lacks one immutable linux/amd64 manifest")
        reference = reference.rsplit("@", 1)[0] + "@" + platforms[0]
        manifest = inspect_manifest(reference)
    config = manifest.get("config")
    if not isinstance(config, dict) or not pinned_digest(config.get("digest")):
        raise ValueError("Normal Guest base lacks a pinned image configuration")
    image = read_json(subprocess.check_output([
        "docker", "buildx", "imagetools", "inspect", "--format", "{{json .Image}}", reference,
    ]))
    if not isinstance(image, dict) or (image.get("os"), image.get("architecture")) != ("linux", "amd64"):
        raise ValueError("Normal Guest base platform differs from linux/amd64")
    image_config = image.get("config")
    labels = image_config.get("Labels") if isinstance(image_config, dict) else None
    expected = {
        "org.opencontainers.image.revision": inputs["neon_source"],
        "io.neon.guest.autoscaling.source": inputs["guest_autoscaling_source"],
        "io.neon.pg.major": str(inputs["postgres_major"]),
        "io.neon.pg.source": postgres_source,
        "io.neon.ordinary.package-lock.sha256": package_lock_sha256,
    }
    if not isinstance(labels, dict) or any(labels.get(key) != value for key, value in expected.items()):
        raise ValueError("Normal Guest base source labels differ from the locked assembly inputs")
    # 标签从该平台基础镜像读回；既有独立验收继续验证内核、PG ELF 和根盘。
    return {"base_image": inputs["base_image"], "platform_image": reference,
            "config_digest": config["digest"], "labels": expected}


def docker_build(builder, dockerfile, context, destination, arguments, options=()):
    subprocess.run([
        "docker", "buildx", "build", "--builder", builder, "--platform", "linux/amd64", "--network=none",
        "--provenance=false", "--file", str(dsga_release.ROOT / "ci" / dockerfile), "--output", destination,
        *options, *[arg for name, value in arguments.items() for arg in ("--build-arg", f"{name}={value}")],
        str(context),
    ], check=True)


def pack(args):
    inputs = read_inputs(args.inputs)
    repository, separator, tag = args.tag.rpartition(":")
    if not separator or not IMAGE_NAME.fullmatch(repository) or not IMAGE_TAG.fullmatch(tag):
        raise ValueError("Guest archive must use an explicit OCI repository and tag")
    major = inputs["postgres_major"]
    root = dsga_release.ROOT
    lock = release.LOCK
    pinned = dsga_release.verify_normal_guest_input_lock(lock)
    plan = root / "compute/normal-build-plan-amd64.json"
    ordinary = root / "compute/normal-ordinary-package-lock-amd64.json"
    builder = args.vm_builder.resolve(strict=True)
    if file_sha(builder) != inputs["builder_sha256"]:
        raise ValueError("Normal Guest Builder bytes differ from the pinned input")
    autoscaling = args.autoscaling_source.resolve(strict=True)
    readback = autoscaling / "ci/distribution.py"
    actual = subprocess.check_output(["git", "-C", str(autoscaling), "rev-parse", "HEAD"], text=True).strip()
    if actual != lock["autoscaling_commit"]:
        raise ValueError("Independent Guest readback must use the locked Autoscaling source")
    subprocess.run(["git", "-C", str(autoscaling), "diff", "--quiet", actual, "--",
                    "ci/distribution.py", "ci/distribution.json"], check=True)
    output = args.output.resolve()
    if output.exists():
        raise FileExistsError(output)
    postgres_source = lock["postgres"][f"v{major}"]
    base_provenance = verify_base_provenance(inputs, postgres_source, pinned["ordinary_package_lock_sha256"])
    # 只有最终 OCI 独立读回成功才公开完成目录。构建或验证中断后可以用相同产物名重试。
    with tempfile.TemporaryDirectory(prefix=".normal-guest-", dir=output.parent) as directory:
        scratch = Path(directory)
        candidate = scratch / "candidate"
        candidate.mkdir()
        context = scratch / "input"
        context.mkdir()
        shutil.copyfile(plan, context / "build-plan.json")
        shutil.copyfile(ordinary, context / "package-lock.json")
        staged_builder = context / "vm-builder"
        shutil.copyfile(builder, staged_builder)
        staged_builder.chmod(0o500)
        if file_sha(staged_builder) != inputs["builder_sha256"]:
            raise ValueError("Staged Normal Guest Builder differs from the pinned input")
        original = args.ordinary_source_root.resolve(strict=True)
        receipt = subprocess.check_output([
            staged_builder, "--system-mode=normal-split-v3", "--normal-ordinary-controls-version=2",
            f"--normal-ordinary-package-lock={ordinary}",
            f"--normal-ordinary-package-lock-sha256={pinned['ordinary_package_lock_sha256']}",
            f"--normal-ordinary-input-root={original}",
        ])
        # 文件集合来自已锁定的包清单。容器在使用暂存输入前重新校验全部文件。
        for path in read_json(ordinary.read_bytes())["inputs"]:
            stage = context / "ordinary-sources" / path
            stage.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(original / path, stage)
        disk = scratch / "disk"
        docker_build(args.buildx_builder, "normal-guest-disk.Dockerfile", context, f"type=local,dest={disk}", {
            "GUEST_BASE_IMAGE": inputs["base_image"], "DISK_TOOLS_IMAGE": inputs["disk_tools_image"],
            "BUILD_PLAN_SHA256": pinned["build_plan_sha256"], "PACKAGE_LOCK_SHA256": pinned["ordinary_package_lock_sha256"],
            "VM_BUILDER_SHA256": inputs["builder_sha256"], "POSTGRES_MAJOR": str(major),
            "POSTGRES_SHA256": inputs["postgres_elf_sha256"],
        }, ("--target", "disk"))
        generated = disk / "ordinary-input-readback.json"
        if read_json(generated.read_bytes()) != read_json(receipt):
            raise ValueError("Guest build observed a different ordinary source tree")
        shutil.copyfile(generated, candidate / generated.name)
        (candidate / "assembly-inputs.json").write_text(json.dumps(inputs, indent=2, sort_keys=True) + "\n")
        (candidate / "base-provenance.json").write_text(json.dumps(base_provenance, indent=2, sort_keys=True) + "\n")
        disk_sha = file_sha(disk / "disk.qcow2")
        archive, metadata = candidate / "guest.oci.tar", candidate / "oci-metadata.json"
        docker_build(args.buildx_builder, "normal-guest-oci.Dockerfile", disk, f"type=oci,dest={archive}", {
            "BOOT_KERNEL_IMAGE": inputs["kernel_image"], "NEON_SOURCE": inputs["neon_source"],
            "AUTOSCALING_SOURCE": inputs["guest_autoscaling_source"], "POSTGRES_SOURCE": postgres_source,
            "POSTGRES_MAJOR": str(major), "ORDINARY_PACKAGE_LOCK_SHA256": pinned["ordinary_package_lock_sha256"],
        }, ("--metadata-file", str(metadata), "--tag", args.tag))
        digest = read_json(metadata.read_bytes())["containerimage.digest"]
        subprocess.run([
            "python3", str(readback), "verify-normal-guest-oci", "--archive", str(archive), "--image-digest", digest,
            "--disk-sha256", disk_sha, "--kernel-sha256", inputs["kernel_sha256"], "--builder", str(staged_builder),
            "--builder-sha256", inputs["builder_sha256"], "--plan", str(plan), "--plan-sha256", pinned["build_plan_sha256"],
            "--base-sha256", inputs["base_image"].rsplit(":", 1)[1], "--postgres-major", str(major),
            "--postgres-sha256", inputs["postgres_elf_sha256"], "--output", str(candidate / "normal-readback.json"),
        ], check=True, cwd=scratch, env={**os.environ, "TMPDIR": str(scratch)})
        candidate.rename(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("inputs", "vm-builder", "ordinary-source-root", "autoscaling-source", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--buildx-builder", required=True)
    parser.add_argument("--tag", required=True)
    pack(parser.parse_args())


if __name__ == "__main__":
    main()
