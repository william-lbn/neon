"""合成制品只验证发行拒错合同，不声称真实OCI/KVM验收。"""
import copy
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

import normal_release as normal
import release
import test_release


class NormalReleaseTests(unittest.TestCase):
    def setUp(self):
        self.base = test_release.ReleaseContractTests()
        self.base.setUp()
        self.images, self.lock = self.base.images, self.base.lock
        self.runtime = {"compute_ctl": "1" * 64, "neonvmd": "2" * 64, "dsga_resource_guard": "3" * 64}
        self.images["neonvm-daemon"]["runtime_elf_sha256"] = {k: self.runtime[k] for k in ("neonvmd", "dsga_resource_guard")}
        self.images["vm-builder"]["builder_sha256"] = "4" * 64
        for index, name in enumerate(sorted(normal.EXTRA_IMAGES), 100):
            original = self.images["vm-kernel" if name == "vm-kernel-normal" else "compute-node-v17"]
            self.images[name] = {**copy.deepcopy(original), "name": name,
                                "image": f"{self.lock['registry']}/{name}:{self.base.tag}", "digest": "sha256:" + f"{index:064x}"}
        self.base.capabilities["image_digests"]["vm-kernel-normal"] = self.images["vm-kernel-normal"]["digest"]
        self.base.capabilities["normal_kernel"] = {
            "image": release.image_reference(self.images["vm-kernel-normal"]), "artifact_sha256": "5" * 64,
            "config_sha256": "6" * 64, "notes_sha256": "7" * 64, "profile": "normal-build-fixture",
            "release": "fixture-release", "normal_metadata_guard": True, "normal_qualification": False,
        }
        for version in sorted(normal.VERSIONS):
            compute = self.images["compute-node-" + version]
            compute.update(postgres_source=self.lock["postgres"][version], postgres_elf_sha256="8" * 64,
                           runtime_elf_sha256={"compute_ctl": self.runtime["compute_ctl"]})
            item = self.images["vm-compute-node-" + version]
            item["guest_disk"]["system_mode"] = normal.MODE
            item["guest_disk"]["normal_guest"] = self.bundle(version)

    def bundle(self, version):
        item = self.images["vm-compute-node-" + version]
        pinned = self.lock["normal_guest_inputs"]
        base_ref = normal.immutable_reference(self.images["normal-guest-base-" + version])
        inputs = {"base_image": base_ref, "disk_tools_image": normal.immutable_reference(self.images["neonvm-runner"]),
                  "kernel_image": normal.immutable_reference(self.images["vm-kernel-normal"]),
                  "neon_source": self.base.neon, "guest_autoscaling_source": self.lock["autoscaling_commit"],
                  "postgres_major": int(version[1:]), "postgres_elf_sha256": "8" * 64,
                  "builder_sha256": "4" * 64, "kernel_sha256": "5" * 64, "runtime_elf_sha256": self.runtime}
        docs = {"assembly-inputs.json": inputs,
                "base-provenance.json": {"base_image": base_ref, "platform_image": base_ref,
                    "config_digest": "sha256:" + "9" * 64, "labels": {
                        "org.opencontainers.image.revision": self.base.neon,
                        "io.neon.guest.autoscaling.source": self.lock["autoscaling_commit"],
                        "io.neon.pg.major": version[1:], "io.neon.pg.source": self.lock["postgres"][version],
                        "io.neon.ordinary.package-lock.sha256": pinned["ordinary_package_lock_sha256"]}},
                "ordinary-input-readback.json": {"kind": "normal-ordinary-input-readback", "protocol_version": 1,
                    "package_lock_sha256": pinned["ordinary_package_lock_sha256"], "derived_file_count": 1,
                    "inputs": [{"path": "apk/input.apk", "sha256": "a" * 64, "bytes": 10}]},
                "oci-metadata.json": {"containerimage.digest": item["digest"]},
                "normal-readback.json": {"kind": "normal-guest-oci-readback", "protocol_version": 1, "builder_sha256": "4" * 64,
                    "oci": {"image_digest": item["digest"], "architecture": "linux/amd64", "platform_manifest_digest": item["digest"],
                            "config_digest": "sha256:" + "a" * 64, "rootfs_diff_ids": ["sha256:" + "b" * 64],
                            "assets": {name: [{"layer_digest": "sha256:" + "b" * 64, "material": {"sha256": value, "size": 100}}]
                                       for name, value in (("disk.qcow2", item["guest_disk"]["sha256"]), ("vmlinuz", "5" * 64))}},
                    "root": {"kind": "normal-guest-root-readback", "protocol_version": 1, "architecture": "linux/amd64",
                             "system_profile": "neon-dsga-normal-system-source-v3", "build_plan_sha256": pinned["build_plan_sha256"],
                             "base_image_sha256": base_ref.rsplit(":", 1)[1], "postgres_major": int(version[1:]),
                             "postgres_elf_sha256": "8" * 64, "runtime_elf_sha256": self.runtime,
                             "system_manifest_sha256": "c" * 64, "native_birth_manifest_sha256": "d" * 64}}}
        raw = {name: json.dumps(value) for name, value in docs.items()}
        archive_sha = hashlib.sha256(b"synthetic archive fixture").hexdigest()
        receipt = {"kind": "normal-guest-assembly", "protocol_version": 1, "build_mode": normal.MODE,
                   "image": release.image_reference(item), "architecture": "linux/amd64", "execution": self.base.execution,
                   "packaging_source": self.base.neon, "compute_source": self.base.neon,
                   "guest_autoscaling_source": self.lock["autoscaling_commit"], "postgres_source": self.lock["postgres"][version],
                   "postgres_major": int(version[1:]), "disk_sha256": item["guest_disk"]["sha256"], "kernel_sha256": "5" * 64,
                   "system_manifest_sha256": "c" * 64, "native_birth_manifest_sha256": "d" * 64, "runtime_elf_sha256": self.runtime,
                   "files_sha256": {**{name: hashlib.sha256(value.encode()).hexdigest() for name, value in raw.items()}, "guest.oci.tar": archive_sha}}
        return {"assembly_json": json.dumps(receipt), "documents": raw, "archive_sha256": archive_sha}

    def proof(self, version="v18"):
        return self.images["vm-compute-node-" + version]["guest_disk"]["normal_guest"]

    def mutate(self, filename, callback, version="v18"):
        proof = self.proof(version)
        if filename == "assembly-receipt.json":
            value = json.loads(proof["assembly_json"])
            callback(value)
            proof["assembly_json"] = json.dumps(value)
        else:
            value = json.loads(proof["documents"][filename])
            callback(value)
            raw = json.dumps(value)
            proof["documents"][filename] = raw
            self.mutate("assembly-receipt.json", lambda receipt: receipt["files_sha256"].update({filename: hashlib.sha256(raw.encode()).hexdigest()}), version)

    def test_complete_pair_is_schema_three_and_keeps_qualification_closed(self):
        output = self.base.manifest()
        self.assertEqual(output["schema_version"], 3)
        self.assertEqual(output["vm_system_mode"], "mixed")
        self.assertEqual(output["vm_system_modes"], {v: normal.MODE if v in normal.VERSIONS else "legacy" for v in self.lock["postgres"]})
        self.assertFalse(output["qualification"]["production_ready"])
        self.assertFalse(output["qualification"]["physical_memory_release_qualified"])
        # 原生gate不同工具链的ELF不会替代正式compute镜像的独立提取值。
        self.assertNotEqual(self.base.native["v18"]["postgres_binary_sha256"], self.images["compute-node-v18"]["postgres_elf_sha256"])

    def test_half_pair_extra_inventory_and_lower_version_cannot_claim_normal(self):
        original = copy.deepcopy(self.images)
        mutations = [lambda: self.images["vm-compute-node-v18"]["guest_disk"].update(system_mode="legacy"),
                     lambda: self.images.pop("normal-guest-base-v17"), lambda: self.images.pop("vm-kernel-normal"),
                     lambda: self.images["vm-compute-node-v16"]["guest_disk"].update(system_mode=normal.MODE)]
        for change in mutations:
            with self.subTest(change=change):
                change()
                with self.assertRaises(ValueError): self.base.manifest()
                self.images.clear(); self.images.update(copy.deepcopy(original))

    def test_assembly_source_attempt_major_and_manifest_digest_drift(self):
        original = copy.deepcopy(self.proof())
        changes = {"execution": {**self.base.execution, "run_attempt": "1"}, "packaging_source": "a" * 40,
                   "compute_source": "a" * 40, "guest_autoscaling_source": "a" * 40, "postgres_source": "a" * 40,
                   "postgres_major": 17, "system_manifest_sha256": "0" * 64, "native_birth_manifest_sha256": "f" * 64}
        for key, value in changes.items():
            with self.subTest(field=key):
                self.mutate("assembly-receipt.json", lambda receipt: receipt.update({key: value}))
                with self.assertRaises(ValueError): self.base.manifest()
                self.images["vm-compute-node-v18"]["guest_disk"]["normal_guest"] = copy.deepcopy(original)

    def test_changed_original_bytes_or_archive_are_rejected(self):
        proof = self.proof()
        proof["documents"]["normal-readback.json"] += " "
        with self.assertRaisesRegex(ValueError, "document digest"): self.base.manifest()
        proof["documents"]["normal-readback.json"] = proof["documents"]["normal-readback.json"][:-1]
        proof["archive_sha256"] = "f" * 64
        with self.assertRaisesRegex(ValueError, "archive digest"): self.base.manifest()

    def test_independent_runtime_digests_are_required_and_all_three_are_checked(self):
        original = copy.deepcopy(self.proof())
        for name in self.runtime:
            with self.subTest(binary=name):
                self.mutate("normal-readback.json", lambda doc: doc["root"]["runtime_elf_sha256"].update({name: "f" * 64}))
                with self.assertRaisesRegex(ValueError, "final root"): self.base.manifest()
                self.images["vm-compute-node-v18"]["guest_disk"]["normal_guest"] = copy.deepcopy(original)
        self.images["compute-node-v18"].pop("runtime_elf_sha256")
        with self.assertRaisesRegex(ValueError, "independent Compute"): self.base.manifest()

    def test_same_run_compute_pg_and_builder_elf_are_original_sources(self):
        for name, field, value in [("compute-node-v18", "postgres_elf_sha256", "f" * 64),
                                   ("compute-node-v18", "postgres_source", "f" * 40),
                                   ("vm-builder", "builder_sha256", "f" * 64)]:
            with self.subTest(field=field):
                old = self.images[name][field]
                self.images[name][field] = value
                with self.assertRaises(ValueError): self.base.manifest()
                self.images[name][field] = old

    def test_same_source_base_kernel_tools_must_be_exact_same_run_oci(self):
        original = copy.deepcopy(self.proof())
        for field in ("base_image", "kernel_image", "disk_tools_image"):
            with self.subTest(field=field):
                self.mutate("assembly-inputs.json", lambda doc: doc.update({field: doc[field].rsplit(":", 1)[0] + ":" + "f" * 64}))
                with self.assertRaisesRegex(ValueError, "assembly inputs"): self.base.manifest()
                self.images["vm-compute-node-v18"]["guest_disk"]["normal_guest"] = copy.deepcopy(original)

    def test_build_plan_package_lock_base_source_and_final_visible_layer_are_bound(self):
        original = copy.deepcopy(self.proof())
        changes = [("normal-readback.json", lambda d: d["root"].update(build_plan_sha256="f" * 64)),
                   ("ordinary-input-readback.json", lambda d: d.update(package_lock_sha256="f" * 64)),
                   ("base-provenance.json", lambda d: d["labels"].update({"io.neon.pg.source": "f" * 40})),
                   ("normal-readback.json", lambda d: d["oci"]["assets"]["disk.qcow2"].append({"layer_digest": "sha256:" + "f" * 64, "material": None}))]
        for filename, change in changes:
            with self.subTest(filename=filename):
                self.mutate(filename, change)
                with self.assertRaises(ValueError): self.base.manifest()
                self.images["vm-compute-node-v18"]["guest_disk"]["normal_guest"] = copy.deepcopy(original)

    def test_actual_normal_kernel_is_independent_of_default_kernel(self):
        original = copy.deepcopy(self.base.capabilities["normal_kernel"])
        for field, value in [("artifact_sha256", "f" * 64), ("config_sha256", "0" * 64),
                             ("image", release.image_reference(self.images["vm-kernel"])), ("normal_metadata_guard", 1),
                             ("normal_qualification", True), ("release", "")]:
            with self.subTest(field=field):
                self.base.capabilities["normal_kernel"][field] = value
                with self.assertRaises(ValueError): self.base.manifest()
                self.base.capabilities["normal_kernel"] = copy.deepcopy(original)

    def test_bundle_reads_original_files_and_rejects_symlink_or_partial_json(self):
        proof = self.proof()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "assembly-receipt.json").write_text(proof["assembly_json"])
            for name, raw in proof["documents"].items(): (root / name).write_text(raw)
            (root / "guest.oci.tar").write_bytes(b"synthetic archive fixture")
            self.assertEqual(normal.read_bundle(root), proof)
            (root / "normal-readback.json").unlink()
            (root / "normal-readback.json").symlink_to(root / "oci-metadata.json")
            with self.assertRaisesRegex(ValueError, "regular files"): normal.read_bundle(root)
        with self.assertRaisesRegex(ValueError, "duplicate"):
            normal.document('{"kind":1,"kind":2}')
        with self.assertRaises(ValueError): normal.document('{"kind":')
        with self.assertRaisesRegex(ValueError, "non-finite"): normal.document('{"kind":NaN}')

    def test_omitted_original_proofs_never_fall_back_to_legacy_or_claim_qualification(self):
        image_name = "vm-compute-node-v18"
        original = copy.deepcopy(self.images)
        changes = [lambda: self.images[image_name]["guest_disk"].pop("normal_guest"),
                   lambda: self.images["vm-builder"].pop("builder_sha256"),
                   lambda: self.images["compute-node-v18"].pop("postgres_elf_sha256"),
                   lambda: self.images["neonvm-daemon"].pop("runtime_elf_sha256"),
                   lambda: self.proof()["documents"].pop("ordinary-input-readback.json"),
                   lambda: self.mutate("assembly-receipt.json", lambda d: d.pop("execution")),
                   lambda: self.mutate("normal-readback.json", lambda d: d["root"].pop("native_birth_manifest_sha256"))]
        for change in changes:
            with self.subTest(change=change):
                change()
                with self.assertRaises(ValueError): self.base.manifest()
                self.images.clear(); self.images.update(copy.deepcopy(original))
        self.base.capabilities.pop("normal_kernel")
        with self.assertRaises(ValueError): self.base.manifest()

    def test_record_cli_consumes_completed_bundle_and_missing_evidence_writes_nothing(self):
        item = self.images["vm-compute-node-v18"]
        proof = self.proof()
        environment = {"GITHUB_REPOSITORY": "william-lbn/neon", "GITHUB_RUN_ID": "123456", "GITHUB_RUN_ATTEMPT": "2"}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bundle, records = root / "bundle", root / "records"
            bundle.mkdir(); records.mkdir()
            (bundle / "assembly-receipt.json").write_text(proof["assembly_json"])
            for name, raw in proof["documents"].items(): (bundle / name).write_text(raw)
            (bundle / "guest.oci.tar").write_bytes(b"synthetic archive fixture")
            for name, record in self.images.items(): (records / (name + ".json")).write_text(json.dumps(record))
            output = root / "record.json"
            argv = ["release.py", "record", "--name", item["name"], "--image", item["image"], "--digest", item["digest"],
                    "--output", str(output), "--vm-disk", str(root / "disk.qcow2"), "--vm-input-images", str(records),
                    "--normal-guest", str(bundle)]
            disk = {key: value for key, value in item["guest_disk"].items() if key != "normal_guest"}
            with mock.patch.object(sys, "argv", argv), mock.patch.dict(os.environ, environment), \
                 mock.patch.object(release, "LOCK", self.lock), \
                 mock.patch.object(release.subprocess, "check_output", return_value=self.base.neon), \
                 mock.patch.object(release, "read_vm_disk", return_value=disk) as read_disk:
                release.main()
                self.assertEqual(json.loads(output.read_text()), item)
                self.assertEqual(read_disk.call_args.args[-1], normal.MODE)
                output.unlink()
                (bundle / "normal-readback.json").unlink()
                with self.assertRaises(ValueError): release.main()
                self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
