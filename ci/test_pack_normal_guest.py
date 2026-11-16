import argparse
import json
from pathlib import Path
import tempfile
import unittest

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
