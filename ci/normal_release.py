"""消费 normal Guest 原打包/读回证据；不授予实例或物理缩容资格。"""
import hashlib
import json
from pathlib import Path
import re

MODE = "normal-split-v3"
VERSIONS = {"v17", "v18"}
EXTRA_IMAGES = {"normal-guest-base-v17", "normal-guest-base-v18", "vm-kernel-normal"}
DOCUMENTS = {"assembly-inputs.json", "base-provenance.json", "ordinary-input-readback.json",
             "normal-readback.json", "oci-metadata.json"}
HASH = re.compile(r"[a-f0-9]{64}")


def require(condition, message):
    if not condition:
        raise ValueError("Normal Guest " + message)


def checksum(value):
    return isinstance(value, str) and HASH.fullmatch(value) is not None and value != "0" * 64


def digest(value):
    return isinstance(value, str) and value.startswith("sha256:") and checksum(value[7:])


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "JSON has duplicate fields")
        result[key] = value
    return result


def document(raw):
    require(isinstance(raw, str) and len(raw.encode()) <= 8 << 20, "document is absent or oversized")
    value = json.loads(raw, object_pairs_hook=unique_object,
                       parse_constant=lambda _: require(False, "JSON contains a non-finite number"))
    require(isinstance(value, dict), "document must be an object")
    return value


def same(value, expected, message):
    require(isinstance(value, dict) and all(type(value.get(k)) is type(v) and value[k] == v
            for k, v in expected.items()), message)


def image_reference(item):
    return item["image"] + "@" + item["digest"]


def immutable_reference(item):
    # Packer输入不接受tag，发行record本身仍用同run不可变tag。
    return item["image"].rsplit(":", 1)[0] + "@" + item["digest"]


def declared(images):
    selected = set()
    for version in VERSIONS:
        item = images.get("vm-compute-node-" + version, {})
        require(isinstance(item, dict), "VM disk record is malformed")
        disk = item.get("guest_disk", {})
        require(isinstance(disk, dict), "VM disk proof is malformed")
        if disk.get("system_mode") == MODE:
            selected.add(version)
    require(not selected or selected == VERSIONS, "VM disk PG17 and PG18 must declare normal together")
    return bool(selected)


def read_bundle(directory):
    """原完成目录只读取一次；JSON原字节保留到manifest以复核producer摘要。"""
    directory = Path(directory)
    def read(name):
        path = directory / name
        require(path.is_file() and not path.is_symlink(), "bundle requires original regular files")
        return path
    assembly = read("assembly-receipt.json").read_bytes().decode("utf-8")
    docs = {name: read(name).read_bytes().decode("utf-8") for name in sorted(DOCUMENTS)}
    archive = read("guest.oci.tar")
    require(archive.stat().st_size > 0, "OCI archive is empty")
    with archive.open("rb") as stream:
        archive_sha = hashlib.file_digest(stream, "sha256").hexdigest()
    proof = {"assembly_json": assembly, "documents": docs, "archive_sha256": archive_sha}
    unpack(proof)
    return proof


def unpack(proof):
    require(isinstance(proof, dict) and set(proof) == {"assembly_json", "documents", "archive_sha256"},
            "assembly evidence is missing")
    receipt = document(proof["assembly_json"])
    docs = proof["documents"]
    require(isinstance(docs, dict) and set(docs) == DOCUMENTS, "original readback documents are incomplete")
    hashes = receipt.get("files_sha256")
    require(isinstance(hashes, dict) and set(hashes) == DOCUMENTS | {"guest.oci.tar"}
            and all(checksum(value) for value in hashes.values()), "file digest set is incomplete")
    parsed = {}
    for name, raw in docs.items():
        parsed[name] = document(raw)
        require(hashlib.sha256(raw.encode()).hexdigest() == hashes[name], "original document digest differs: " + name)
    require(checksum(proof["archive_sha256"]) and proof["archive_sha256"] == hashes["guest.oci.tar"],
            "OCI archive digest differs")
    return receipt, parsed


def verify(proof, item, images, lock, neon_commit, execution):
    receipt, docs = unpack(proof)
    version = item["name"].removeprefix("vm-compute-node-")
    require(version in VERSIONS, "is only supported for PG17 and PG18")
    major = int(version[1:])
    # record入口可以先于全发行汇总，故其每个实际输入仍独立核原run及源码。
    tag = item["image"].rsplit(":", 1)[1]
    names = {"compute-node-" + version, "normal-guest-base-" + version,
             "vm-kernel-normal", "neonvm-runner", "neonvm-daemon", "vm-builder"}
    for name in names:
        auto = name in {"vm-kernel-normal", "neonvm-runner", "neonvm-daemon", "vm-builder"}
        original = images.get(name)
        same(original, {"kind": "runtime", "name": name, "architecture": "linux/amd64", "execution": execution,
                        "image": f"{lock['registry']}/{name}:{tag}",
                        "source_repository": lock["autoscaling_repository"] if auto else "william-lbn/neon",
                        "source_commit": lock["autoscaling_commit"] if auto else neon_commit},
             "input image source or run/attempt differs: " + name)
        require(digest(original.get("digest")), "input image digest is invalid")
    pinned = lock.get("normal_guest_inputs")
    require(isinstance(pinned, dict) and pinned.get("architecture") == "linux/amd64"
            and all(checksum(pinned.get(k)) for k in ("build_plan_sha256", "ordinary_package_lock_sha256")),
            "build plan and ordinary package lock are required")
    same(receipt, {"kind": "normal-guest-assembly", "protocol_version": 1, "build_mode": MODE,
                  "image": image_reference(item), "architecture": "linux/amd64", "execution": execution,
                  "packaging_source": neon_commit, "compute_source": neon_commit,
                  "guest_autoscaling_source": lock["autoscaling_commit"],
                  "postgres_source": lock["postgres"][version], "postgres_major": major},
         "assembly source, image or run/attempt differs")
    require(all(checksum(receipt.get(k)) for k in ("disk_sha256", "kernel_sha256", "system_manifest_sha256", "native_birth_manifest_sha256")),
            "disk, kernel, System or NativeBirth digest is absent")
    require(item["guest_disk"]["sha256"] == receipt["disk_sha256"], "published VM disk differs from assembly")
    inputs = docs["assembly-inputs.json"]
    refs = {"base_image": immutable_reference(images["normal-guest-base-" + version]),
            "kernel_image": immutable_reference(images["vm-kernel-normal"]),
            "disk_tools_image": immutable_reference(images["neonvm-runner"])}
    compute, daemon, builder = (images[name] for name in ("compute-node-" + version, "neonvm-daemon", "vm-builder"))
    # 原native gate独立构建，ELF可以不同。这里只绑定同run正式compute镜像的提取值。
    require(compute.get("postgres_source") == lock["postgres"][version] and checksum(compute.get("postgres_elf_sha256")),
            "compute image lacks its independently extracted PostgreSQL artifact")
    compute_elf, daemon_elf = compute.get("runtime_elf_sha256"), daemon.get("runtime_elf_sha256")
    require(isinstance(compute_elf, dict) and set(compute_elf) == {"compute_ctl"}
            and isinstance(daemon_elf, dict) and set(daemon_elf) == {"neonvmd", "dsga_resource_guard"},
            "source images lack independent Compute/Daemon/Guard ELF digests")
    runtime = {**compute_elf, **daemon_elf}
    require(all(checksum(value) for value in runtime.values()) and checksum(builder.get("builder_sha256")),
            "runtime or Builder digest is invalid")
    same(inputs, {**refs, "neon_source": neon_commit, "guest_autoscaling_source": lock["autoscaling_commit"],
                  "postgres_major": major, "postgres_elf_sha256": compute["postgres_elf_sha256"],
                  "builder_sha256": builder["builder_sha256"], "kernel_sha256": receipt["kernel_sha256"],
                  "runtime_elf_sha256": runtime}, "assembly inputs differ from same-run immutable artifacts")
    require(receipt.get("runtime_elf_sha256") == runtime, "assembly runtime ELF digests differ")
    base = docs["base-provenance.json"]
    same(base, {"base_image": refs["base_image"]}, "base provenance differs from the original base image")
    platform = base.get("platform_image", "")
    require(isinstance(platform, str) and platform.partition("@")[0] == refs["base_image"].partition("@")[0]
            and digest(platform.partition("@")[2]) and digest(base.get("config_digest")), "base platform/config is unpinned")
    same(base.get("labels"), {"org.opencontainers.image.revision": neon_commit,
                             "io.neon.guest.autoscaling.source": lock["autoscaling_commit"],
                             "io.neon.pg.major": str(major), "io.neon.pg.source": lock["postgres"][version],
                             "io.neon.ordinary.package-lock.sha256": pinned["ordinary_package_lock_sha256"]},
         "base source labels differ")
    ordinary = docs["ordinary-input-readback.json"]
    same(ordinary, {"kind": "normal-ordinary-input-readback", "protocol_version": 1,
                    "package_lock_sha256": pinned["ordinary_package_lock_sha256"]}, "ordinary package proof differs")
    require(type(ordinary.get("derived_file_count")) is int and ordinary["derived_file_count"] > 0
            and isinstance(ordinary.get("inputs"), list) and ordinary["inputs"], "ordinary input inventory is empty")
    paths = []
    for entry in ordinary["inputs"]:
        require(isinstance(entry, dict) and isinstance(entry.get("path"), str) and checksum(entry.get("sha256"))
                and type(entry.get("bytes")) is int and entry["bytes"] > 0, "ordinary input entry is invalid")
        paths.append(entry["path"])
    require(len(paths) == len(set(paths)), "ordinary input inventory duplicates a path")
    readback = docs["normal-readback.json"]
    same(readback, {"kind": "normal-guest-oci-readback", "protocol_version": 1,
                    "builder_sha256": builder["builder_sha256"]}, "final OCI readback is absent")
    oci = readback.get("oci")
    same(oci, {"image_digest": item["digest"], "architecture": "linux/amd64"}, "final OCI identity differs")
    require(digest(oci.get("platform_manifest_digest")) and digest(oci.get("config_digest"))
            and isinstance(oci.get("rootfs_diff_ids"), list) and oci["rootfs_diff_ids"]
            and all(digest(value) for value in oci["rootfs_diff_ids"]), "final OCI config/layers are incomplete")
    same(docs["oci-metadata.json"], {"containerimage.digest": item["digest"]}, "producer OCI digest differs")
    for name, expected in (("disk.qcow2", receipt["disk_sha256"]), ("vmlinuz", receipt["kernel_sha256"])):
        history = oci.get("assets", {}).get(name)
        require(isinstance(history, list) and history, "final disk/kernel layer history is absent")
        for layer in history:
            require(isinstance(layer, dict) and digest(layer.get("layer_digest")), "asset layer identity is invalid")
            material = layer.get("material")
            require(material is None or isinstance(material, dict) and checksum(material.get("sha256"))
                    and type(material.get("size")) is int and material["size"] > 0, "asset material is invalid")
        require(history[-1].get("material") is not None and history[-1]["material"]["sha256"] == expected,
                "final visible disk/kernel differs from the assembly")
    same(readback.get("root"), {"kind": "normal-guest-root-readback", "protocol_version": 1,
          "architecture": "linux/amd64", "system_profile": "neon-dsga-normal-system-source-v3",
          "build_plan_sha256": pinned["build_plan_sha256"], "base_image_sha256": refs["base_image"].rsplit(":", 1)[1],
          "postgres_major": major, "postgres_elf_sha256": compute["postgres_elf_sha256"],
          "system_manifest_sha256": receipt["system_manifest_sha256"],
          "native_birth_manifest_sha256": receipt["native_birth_manifest_sha256"], "runtime_elf_sha256": runtime},
         "final root differs from the independently built artifacts")
    return receipt


def verify_kernel(capabilities, images):
    kernel = capabilities.get("normal_kernel")
    same(kernel, {"image": image_reference(images["vm-kernel-normal"]), "normal_metadata_guard": True,
                  "normal_qualification": False}, "normal kernel OCI/capability proof is absent")
    require(all(checksum(kernel.get(k)) for k in ("artifact_sha256", "config_sha256", "notes_sha256"))
            and all(isinstance(kernel.get(k), str) and kernel[k].strip() for k in ("release", "profile")),
            "actual normal kernel build identity is incomplete")
    for version in VERSIONS:
        receipt, _ = unpack(images["vm-compute-node-" + version]["guest_disk"]["normal_guest"])
        require(receipt["kernel_sha256"] == kernel["artifact_sha256"], "Guest boot kernel differs from the actual normal kernel")
    return kernel
