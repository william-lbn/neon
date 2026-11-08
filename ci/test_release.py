"""跨仓发行接缝的拒错测试：使用明确合成制品，不冒充 registry 或 VM 运行证据。"""
import argparse
import copy
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import compute
import dsga_release
import release


class ReleaseContractTests(unittest.TestCase):
    def setUp(self):
        self.lock = copy.deepcopy(release.LOCK)
        self.tag = "0.2.0-dsga.123456.2.amd64"
        self.neon = "f" * 40
        self.execution = release.execution_identity(self.tag)
        self.images = {}
        for i, name in enumerate(sorted(release.expected_images(self.lock)), 1):
            autoscaling = name in self.lock["autoscaling_images"]
            self.images[name] = {
                "kind": "runtime", "name": name,
                "image": f"{self.lock['registry']}/{name}:{self.tag}",
                "digest": "sha256:" + f"{i:064x}",
                "source_repository": self.lock["autoscaling_repository"] if autoscaling else "william-lbn/neon",
                "source_commit": self.lock["autoscaling_commit"] if autoscaling else self.neon,
                "architecture": "linux/amd64", "execution": copy.deepcopy(self.execution),
            }
        self.capabilities = {
            "kind": "distribution-capabilities", "schema_version": 1,
            "source_repository": self.lock["autoscaling_repository"],
            "source_commit": self.lock["autoscaling_commit"], "version": self.tag,
            "architecture": "linux/amd64", "release_channel": "candidate",
            "normal_qualification": False, "production_ready": False, "latest_eligible": False,
            "vm_builder_default_system_mode": "legacy", "execution": copy.deepcopy(self.execution),
            "kernel": {"artifact_sha256": "a" * 64, "config_sha256": "b" * 64, "notes_sha256": "c" * 64,
                       "image": self.images["vm-kernel"]["image"] + "@" + self.images["vm-kernel"]["digest"],
                       "normal_qualification": False, "normal_metadata_guard": False},
            "image_digests": {name: self.images[name]["digest"] for name in self.lock["autoscaling_images"]},
        }
        self.native = {version: {
            "kind": "postgres-native-validation", "schema_version": 1,
            "source_repository": "william-lbn/postgres", "source_commit": self.lock["postgres"][version],
            "pg_major": int(version[1:]), "scope": "native-kernel-not-vm", "normal_qualification": False,
            "production_ready": False, "postgres_binary_sha256": "d" * 64,
            "run_id": self.execution["run_id"], "run_attempt": self.execution["run_attempt"],
            "execution_repository": self.execution["repository"],
        } for version in ["v17", "v18"]}

    def manifest(self):
        return release.make_manifest(self.images, self.lock, self.tag, "amd64", self.neon, self.neon,
                                     "dsga", self.capabilities, self.native)

    def test_complete_candidate_has_exact_profiles_and_stays_unqualified(self):
        result = self.manifest()
        self.assertEqual(result["schema_version"], 2)
        self.assertEqual(result["version"], result["chart_version"])
        self.assertEqual(result["channel"], "dsga")
        self.assertEqual(result["vm_system_mode"], "legacy")
        self.assertEqual(result["qualification"], {"production_ready": False, "physical_memory_release_qualified": False})
        self.assertIn("compute-node-v18", result["images"])
        self.assertIn("vm-compute-node-v18", result["images"])
        self.assertNotIn("neon-test-extensions-v18", result["images"])
        self.assertEqual(set(result["postgres_native_validation"]), {"v17", "v18"})

    def test_missing_pg18_image_is_rejected(self):
        del self.images["compute-node-v18"]
        with self.assertRaisesRegex(ValueError, "Distribution incomplete"):
            self.manifest()

    def test_unexpected_unqualified_full_pg18_test_image_is_rejected(self):
        self.images["neon-test-extensions-v18"] = copy.deepcopy(self.images["neon-test-extensions-v17"])
        with self.assertRaisesRegex(ValueError, "Distribution incomplete"):
            self.manifest()

    def test_wrong_autoscaling_source_is_rejected(self):
        self.images["autoscaler-agent"]["source_commit"] = "0" * 40
        with self.assertRaisesRegex(ValueError, "locked commit"):
            self.manifest()

    def test_wrong_neon_source_repository_is_rejected(self):
        self.images["compute-node-v18"]["source_repository"] = "somebody/neon"
        with self.assertRaisesRegex(ValueError, "source repository"):
            self.manifest()

    def test_tag_registry_and_digest_corruption_are_rejected(self):
        for key, value in [("image", "docker.io/other/neon:" + self.tag),
                           ("image", self.images["neon"]["image"].replace(self.tag, "latest")),
                           ("digest", "sha256:short")]:
            with self.subTest(field=key, value=value):
                original = self.images["neon"][key]
                self.images["neon"][key] = value
                with self.assertRaises(ValueError):
                    self.manifest()
                self.images["neon"][key] = original

    def test_mixed_architectures_are_rejected(self):
        self.images["pageserver"]["architecture"] = "linux/arm64"
        with self.assertRaisesRegex(ValueError, "architecture"):
            self.manifest()

    def test_old_attempt_image_is_rejected(self):
        self.images["pageserver"]["execution"]["run_attempt"] = "1"
        with self.assertRaisesRegex(ValueError, "run/attempt"):
            self.manifest()

    def test_missing_profile_or_pg18_full_claim_is_rejected(self):
        self.lock["postgres_profiles"]["v18"] = "full"
        with self.assertRaisesRegex(ValueError, "profile matrix"):
            self.manifest()

    def test_parallel_different_validation_source_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "exact build source"):
            release.make_manifest(self.images, self.lock, self.tag, "amd64", self.neon, "e" * 40,
                                  "dsga", self.capabilities, self.native)

    def test_capability_source_or_run_drift_is_rejected(self):
        for field, value in [("source_commit", "e" * 40), ("version", "latest"),
                             ("execution", {**self.execution, "run_attempt": "1"})]:
            with self.subTest(field=field):
                original = self.capabilities[field]
                self.capabilities[field] = value
                with self.assertRaisesRegex(ValueError, "capability mismatch"):
                    self.manifest()
                self.capabilities[field] = original

    def test_capability_digest_set_must_equal_released_images(self):
        self.capabilities["image_digests"]["autoscaler-agent"] = "sha256:" + "a" * 64
        with self.assertRaisesRegex(ValueError, "capability image digests"):
            self.manifest()

    def test_capability_kernel_image_must_equal_released_kernel(self):
        self.capabilities["kernel"]["image"] += "x"
        with self.assertRaisesRegex(ValueError, "released vm-kernel"):
            self.manifest()

    def test_capability_missing_real_kernel_hash_is_rejected(self):
        del self.capabilities["kernel"]["config_sha256"]
        with self.assertRaisesRegex(ValueError, "actual kernel build identity"):
            self.manifest()

    def test_candidate_does_not_acquire_normal_qualification(self):
        for field in ["normal_qualification", "production_ready", "latest_eligible"]:
            for value in [True, 0]:
                with self.subTest(field=field, value=value):
                    self.capabilities[field] = value
                    with self.assertRaisesRegex(ValueError, "capability mismatch"):
                        self.manifest()
                    self.capabilities[field] = False

    def test_native_pg_gate_missing_or_wrong_source_is_rejected(self):
        self.native["v18"]["source_commit"] = "e" * 40
        with self.assertRaisesRegex(ValueError, "Native PG receipt mismatch"):
            self.manifest()
        del self.native["v18"]
        with self.assertRaisesRegex(ValueError, "Both native"):
            self.manifest()

    def test_native_pg_gate_from_another_attempt_is_rejected(self):
        self.native["v18"]["run_attempt"] = "1"
        with self.assertRaisesRegex(ValueError, "Native PG receipt mismatch"):
            self.manifest()

    def test_current_run_must_match_version_before_any_write(self):
        env = {"GITHUB_REPOSITORY": "william-lbn/neon", "GITHUB_RUN_ID": "123456", "GITHUB_RUN_ATTEMPT": "2"}
        with mock.patch.dict(os.environ, env):
            dsga_release.verify_attempt(self.tag, "amd64")
            with self.assertRaises(ValueError):
                dsga_release.verify_attempt("0.2.0-dsga.123456.1.amd64", "amd64")
            with self.assertRaises(ValueError):
                dsga_release.verify_attempt(self.tag, "arm64")

    def test_pg18_core_does_not_schedule_third_party_rust_stages(self):
        self.assertEqual(compute.runtime_targets("v18"), ["runtime"])
        self.assertEqual(compute.runtime_targets("v17"), ["runtime", "extension-tests"])
        self.assertNotIn("rust-extensions-build", compute.common_stages("v18"))
        self.assertIn("neon-ext-build", compute.common_stages("v18"))
        self.assertEqual(len(compute.postgres_matrix()["include"]), 5)

    def test_pg18_full_extension_recipe_is_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            args = argparse.Namespace(pg="v18", target="extension-tests", payload=False, output=Path(d)/"Dockerfile", artifacts=d)
            with self.assertRaisesRegex(ValueError, "no full"):
                compute.render(args)

    def test_compute_record_producer_matches_helm_required_shape(self):
        env = {"GITHUB_REPOSITORY": "william-lbn/neon", "GITHUB_RUN_ID": "123456", "GITHUB_RUN_ATTEMPT": "2"}
        with tempfile.TemporaryDirectory() as d, mock.patch.dict(os.environ, env):
            metadata = Path(d, "metadata.json")
            metadata.write_text(json.dumps({"containerimage.digest": "sha256:" + "a" * 64}))
            output = Path(d, "compute-node-v18.json")
            args = argparse.Namespace(metadata=metadata, pg="v18", target="runtime", stage=False,
                                      image=f"docker.io/williamluckyli/compute-node-v18:{self.tag}",
                                      output=output, arch="amd64")
            compute.record(args)
            record = json.loads(output.read_text())
            self.assertEqual(record["name"], "compute-node-v18")
            self.assertEqual(record["kind"], "runtime")
            self.assertEqual(record["architecture"], "linux/amd64")
            self.assertEqual(record["execution"], self.execution)
            self.assertEqual(record["source_repository"], "william-lbn/neon")
            self.assertRegex(record["source_commit"], r"^[0-9a-f]{40}$")

    def test_runtime_record_name_must_match_map_key(self):
        del self.images["compute-node-v18"]["name"]
        with self.assertRaisesRegex(ValueError, "name/kind"):
            self.manifest()

    def test_duplicate_runtime_digest_records_cannot_hide_conflict(self):
        with tempfile.TemporaryDirectory() as d:
            item = self.images["neon"]
            Path(d, "a.json").write_text(json.dumps(item))
            Path(d, "b.json").write_text(json.dumps({**item, "digest": "sha256:" + "9" * 64}))
            with self.assertRaisesRegex(ValueError, "Conflicting image"):
                release.load_images(d)


if __name__ == "__main__":
    unittest.main()
