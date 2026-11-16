import argparse
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import pack_normal_guest as packer


class PackNormalGuestTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.input_path = self.root / "inputs.json"
        self.inputs = {
            "postgres_major": 17,
            "base_image": "127.0.0.1:5001/neon/pg17-base@sha256:" + "1" * 64,
            "disk_tools_image": "registry.example/neon/disk-tools@sha256:" + "2" * 64,
            "kernel_image": "registry.example/neon/kernel@sha256:" + "3" * 64,
            "builder_sha256": "4" * 64,
            "kernel_sha256": "5" * 64,
            "postgres_elf_sha256": "6" * 64,
            "neon_source": "7" * 40,
            "guest_autoscaling_source": "8" * 40,
        }

    def write_inputs(self, **overrides):
        self.input_path.write_text(json.dumps(self.inputs | overrides))

    def test_accepts_both_postgres_majors_with_independent_pins(self):
        for major in (17, 18):
            self.write_inputs(postgres_major=major)
            self.assertEqual(packer.read_inputs(self.input_path), self.inputs | {"postgres_major": major})

    def test_rejects_moving_or_missing_content_pins(self):
        for changes in (
            {"base_image": "registry.example/neon/pg17-base:latest"},
            {"base_image": "registry.example/neon/pg17-base@sha256:" + "0" * 64},
            {"base_image": "https://registry.example/neon/pg17-base@sha256:" + "1" * 64},
            {"kernel_image": "registry.example/neon/kernel@sha256:" + "1" * 63},
            {"postgres_elf_sha256": "0" * 64},
            {"guest_autoscaling_source": "main"},
            {"postgres_major": "17"},
            {"postgres_major": 16},
            {"runtime_elf_sha256": {"compute_ctl": "a" * 64}},
            {"runtime_elf_sha256": None},
        ):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                self.write_inputs(**changes)
                packer.read_inputs(self.input_path)

    def test_rejects_ambiguous_input_document(self):
        self.input_path.write_text(json.dumps(self.inputs)[:-1] + ', "postgres_major": 18}')
        with self.assertRaisesRegex(ValueError, "Duplicate JSON key postgres_major"):
            packer.read_inputs(self.input_path)

    def test_modified_builder_cannot_publish_an_output(self):
        self.write_inputs()
        builder = self.root / "vm-builder"
        builder.write_bytes(b"modified builder")
        output = self.root / "output"
        args = argparse.Namespace(inputs=self.input_path, tag="registry.example/neon/guest:test",
                                  vm_builder=builder, output=output)
        with self.assertRaisesRegex(ValueError, "Builder bytes differ"):
            packer.pack(args)
        self.assertFalse(output.exists())

    def base_provenance_fixture(self):
        repository = self.inputs["base_image"].split("@")[0]
        postgres = "a" * 40
        ordinary = "b" * 64
        config_digest = "sha256:" + "c" * 64
        manifest = json.dumps({"schemaVersion": 2, "config": {"digest": config_digest}}).encode()
        reference = repository + "@sha256:" + hashlib.sha256(manifest).hexdigest()
        labels = {
            "org.opencontainers.image.revision": self.inputs["neon_source"],
            "io.neon.guest.autoscaling.source": self.inputs["guest_autoscaling_source"],
            "io.neon.pg.major": str(self.inputs["postgres_major"]),
            "io.neon.pg.source": postgres,
            "io.neon.ordinary.package-lock.sha256": ordinary,
        }
        image = {"os": "linux", "architecture": "amd64", "config": {"Labels": labels}}
        return self.inputs | {"base_image": reference}, postgres, ordinary, manifest, config_digest, image

    def test_base_manifest_binds_provenance_to_amd64_configuration(self):
        inputs, postgres, ordinary, manifest, config_digest, image = self.base_provenance_fixture()
        with patch.object(packer.subprocess, "check_output", side_effect=[manifest, json.dumps(image).encode()]):
            proof = packer.verify_base_provenance(inputs, postgres, ordinary)
        self.assertEqual(proof["platform_image"], inputs["base_image"])
        self.assertEqual(proof["config_digest"], config_digest)
        self.assertEqual(proof["labels"], image["config"]["Labels"])

    def test_base_index_requires_one_matching_platform_content(self):
        inputs, postgres, ordinary, manifest, _, image = self.base_provenance_fixture()
        member = inputs["base_image"].split("@")[1]
        repository = inputs["base_image"].split("@")[0]
        matching = {"digest": member, "platform": {"os": "linux", "architecture": "amd64"}}
        arm = {"digest": "sha256:" + "d" * 64, "platform": {"os": "linux", "architecture": "arm64"}}
        for descriptors, accepted in (([arm, matching], True), ([matching, matching], False), ([arm], False)):
            with self.subTest(descriptors=descriptors):
                index = json.dumps({"schemaVersion": 2, "manifests": descriptors}).encode()
                root = repository + "@sha256:" + hashlib.sha256(index).hexdigest()
                index_inputs = inputs | {"base_image": root}
                with patch.object(packer.subprocess, "check_output", side_effect=[index, manifest, json.dumps(image).encode()]) as inspect:
                    if accepted:
                        proof = packer.verify_base_provenance(index_inputs, postgres, ordinary)
                        self.assertEqual(proof["base_image"], root)
                        self.assertEqual(proof["platform_image"], inputs["base_image"])
                    else:
                        with self.assertRaisesRegex(ValueError, "one immutable linux/amd64"):
                            packer.verify_base_provenance(index_inputs, postgres, ordinary)
                        self.assertEqual(inspect.call_count, 1)

    def test_base_provenance_rejects_changed_manifest_and_mislabeled_inputs(self):
        inputs, postgres, ordinary, manifest, _, image = self.base_provenance_fixture()
        with patch.object(packer.subprocess, "check_output", return_value=manifest + b" ") as inspect:
            with self.assertRaisesRegex(ValueError, "manifest bytes differ"):
                packer.verify_base_provenance(inputs, postgres, ordinary)
            self.assertEqual(inspect.call_count, 1)
        for key in image["config"]["Labels"]:
            with self.subTest(label=key):
                labels = image["config"]["Labels"] | {key: "old-or-different"}
                stale = image | {"config": {"Labels": labels}}
                with patch.object(packer.subprocess, "check_output", side_effect=[manifest, json.dumps(stale).encode()]):
                    with self.assertRaisesRegex(ValueError, "source labels differ"):
                        packer.verify_base_provenance(inputs, postgres, ordinary)

    def test_assembly_receipt_is_bound_to_final_oci_and_root(self):
        inputs = self.inputs
        digest = "sha256:" + "a" * 64
        pinned = {"architecture": "linux/amd64", "build_plan_sha256": "b" * 64,
                  "ordinary_package_lock_sha256": "c" * 64}
        disk_sha = "d" * 64
        oci = {"image_digest": digest, "platform_manifest_digest": digest,
               "config_digest": "sha256:" + "e" * 64,
               "architecture": pinned["architecture"], "rootfs_diff_ids": [digest],
               "assets": {"disk.qcow2": [{"material": {"sha256": disk_sha}}],
                          "vmlinuz": [{"material": {"sha256": inputs["kernel_sha256"]}}]}}
        root = {"kind": "normal-guest-root-readback", "protocol_version": 1,
                "architecture": pinned["architecture"], "system_profile": "neon-dsga-normal-system-source-v3",
                "build_plan_sha256": pinned["build_plan_sha256"], "base_image_sha256": "1" * 64,
                "postgres_major": 17, "postgres_elf_sha256": inputs["postgres_elf_sha256"],
                "system_manifest_sha256": "f" * 64, "native_birth_manifest_sha256": "9" * 64}
        readback = {"kind": "normal-guest-oci-readback", "protocol_version": 1,
                    "builder_sha256": inputs["builder_sha256"], "oci": oci, "root": root}
        documents = {"normal-readback.json": readback, "assembly-inputs.json": inputs, "base-provenance.json": {},
                     "oci-metadata.json": {"containerimage.digest": digest},
                     "ordinary-input-readback.json": {"package_lock_sha256": pinned["ordinary_package_lock_sha256"]}}
        for name, doc in documents.items():
            (self.root / name).write_text(json.dumps(doc))
        (self.root / "guest.oci.tar").write_bytes(b"archive fixture")
        args = (self.root, "registry.example/neon/guest:test", inputs, pinned, "a" * 40, "b" * 40, disk_sha)
        proof = packer.assembly_receipt(*args)
        self.assertEqual(proof["image"], args[1] + "@" + digest)
        self.assertEqual(proof["compute_source"], inputs["neon_source"])
        self.assertEqual(proof["files_sha256"]["guest.oci.tar"], packer.file_sha(self.root / "guest.oci.tar"))
        for scope, key, invalid in ((oci, "image_digest", "sha256:" + "7" * 64),
                                    (root, "postgres_major", 18),
                                    (root, "build_plan_sha256", "0" * 64)):
            with self.subTest(key=key), patch.dict(scope, {key: invalid}):
                (self.root / "normal-readback.json").write_text(json.dumps(readback))
                with self.assertRaisesRegex(ValueError, "Independent normal Guest"):
                    packer.assembly_receipt(*args)
        runtime = {"compute_ctl": "a" * 64, "neonvmd": "b" * 64, "dsga_resource_guard": "c" * 64}
        self.write_inputs(runtime_elf_sha256=runtime)
        self.assertEqual(packer.read_inputs(self.input_path)["runtime_elf_sha256"], runtime)
        args = (*args[:2], inputs | {"runtime_elf_sha256": runtime}, *args[3:])
        root["runtime_elf_sha256"] = runtime
        (self.root / "normal-readback.json").write_text(json.dumps(readback))
        self.assertEqual(packer.assembly_receipt(*args)["runtime_elf_sha256"], runtime)
        root["runtime_elf_sha256"] = runtime | {"dsga_resource_guard": "f" * 64}
        (self.root / "normal-readback.json").write_text(json.dumps(readback))
        with self.assertRaisesRegex(ValueError, "runtime ELF readback differs"):
            packer.assembly_receipt(*args)
