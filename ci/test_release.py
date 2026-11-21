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
        control = self.lock["control_plane"]
        for i, name in enumerate(sorted(release.expected_images(self.lock, "dsga")), 1):
            if name in self.lock["autoscaling_images"]:
                repo, commit = self.lock["autoscaling_repository"], self.lock["autoscaling_commit"]
            elif name in control["images"]:
                repo, commit = control["repository"], control["commit"]
            else:
                repo, commit = "william-lbn/neon", self.neon
            self.images[name] = {
                "kind": "runtime", "name": name,
                "image": f"{self.lock['registry']}/{name}:{self.tag}",
                "digest": "sha256:" + f"{i:064x}",
                "source_repository": repo, "source_commit": commit,
                "architecture": "linux/amd64", "execution": copy.deepcopy(self.execution),
            }
        for version in self.lock["postgres"]:
            name = f"vm-compute-node-{version}"
            self.images[name]["guest_disk"] = {
                "system_mode": "legacy", "format": "qcow2", "virtual_size": 4 * 2**30,
                "image": release.image_reference(self.images[name]), "sha256": "e" * 64,
                "inputs": release.vm_input_references(self.images, name),
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

    def test_reusable_workflow_refs_match_the_exact_source_lock(self):
        # Cross-repository workflow code and image source commits are one immutable
        # release contract. The feature-branch dispatcher must call the complete
        # candidate workflow, while PR-only native gates must not duplicate push builds.
        dsga_release.verify_workflow_pins(self.lock)
        workflow = (Path(__file__).resolve().parents[1] / ".github/workflows/dsga-release.yml").read_text()
        self.assertIn("  workflow_call:", workflow)
        self.assertIn("      arch:", workflow)
        self.assertIn("      normal_guest:", workflow)
        self.assertIn("      DOCKERHUB_USERNAME:", workflow)
        self.assertIn("      DOCKERHUB_TOKEN:", workflow)
        # Compute and image validation need registry access; pass only the two
        # Docker Hub credentials to those reusable workflows.
        compute_job = workflow.split("  compute:\n", 1)[1].split("\n  compute-components:", 1)[0]
        validate_job = workflow.split("  validate:\n", 1)[1].split("\n  helm:", 1)[0]
        validator_workflow = (Path(__file__).resolve().parents[1] / ".github/workflows/_validate-dsga.yml").read_text()
        self.assertIn("    secrets:", validator_workflow)
        self.assertIn("      DOCKERHUB_USERNAME:", validator_workflow)
        self.assertIn("      DOCKERHUB_TOKEN:", validator_workflow)
        for job in (compute_job, validate_job):
            self.assertIn("DOCKERHUB_USERNAME: ${{ secrets.DOCKERHUB_USERNAME }}", job)
            self.assertIn("DOCKERHUB_TOKEN: ${{ secrets.DOCKERHUB_TOKEN }}", job)
            self.assertNotIn("secrets: inherit", job)
        # Cross-repository workflows receive only the credentials they actually use.
        # Cross-repository builders receive only Docker Hub credentials.
        self.assertNotIn("secrets: inherit", workflow)
        self.assertIn(
            "DOCKERHUB_USERNAME: ${{ secrets.DOCKERHUB_USERNAME }}",
            workflow,
        )
        self.assertIn(
            "DOCKERHUB_TOKEN: ${{ secrets.DOCKERHUB_TOKEN }}",
            workflow,
        )

        # The reusable release is immutable once registry writes begin. A later Push
        # may queue the newest candidate, but must never cancel this run mid-publication.
        self.assertIn("cancel-in-progress: false", workflow)
        self.assertIn("dsga-distribution-${{ github.ref }}-", workflow)

        branch_pr = (Path(__file__).resolve().parents[1] / ".github/workflows/pull-request.yml").read_text()
        # Both PR validation and the branch-push release dispatcher are registered.
        self.assertIn("  pull_request:", branch_pr)
        self.assertIn("  push:", branch_pr)
        self.assertIn("branches: ['codex/dsga-neon-v3-20261019']", branch_pr)
        self.assertIn("  workflow_dispatch:", branch_pr)
        # Stale PR builds may be cancelled, but an active immutable candidate release
        # must finish; otherwise it can leave an incomplete tag/draft in the registry.
        self.assertIn("group: neon-dsga-ci-${{ github.workflow }}-", branch_pr)
        self.assertIn("cancel-in-progress: ${{ github.event_name == 'pull_request' }}", branch_pr)

        # The expensive PG17/PG18 standalone gates run for PRs only. On feature pushes,
        # the unified release workflow owns those same native checks in one run, avoiding
        # duplicate native builds while keeping the complete candidate evidence together.
        self.assertIn("  dsga-pg17:\n    if: github.event_name == 'pull_request'", branch_pr)
        self.assertIn("  dsga-pg18:\n    if: github.event_name == 'pull_request'", branch_pr)
        self.assertIn("git submodule update --init --depth 1 vendor/postgres-v17", branch_pr)

        pg18_gate = (
            "uses: william-lbn/postgres/.github/workflows/build.yml@"
            + self.lock["postgres"]["v18"]
        )
        self.assertIn(pg18_gate, branch_pr)
        self.assertIn("source_sha: " + self.lock["postgres"]["v18"], branch_pr)

        # Push/manual dispatch on the one approved development branch invokes the full
        # immutable candidate pipeline; it must never run for arbitrary branches or PRs.
        self.assertIn("  dsga-unified-release:", branch_pr)
        # Publishing is hard-gated by the complete source-level test suite for both
        # Push and manual candidate dispatch; no registry write can start before it.
        self.assertIn("  contracts:", branch_pr)
        self.assertIn("python3 -m unittest discover -s ci -p 'test_*.py' -v", branch_pr)
        self.assertIn("  dsga-unified-release:\n    needs: contracts", branch_pr)
        self.assertIn(
            "uses: william-lbn/neon/.github/workflows/dsga-release.yml@47084902bcb5ee0e2a88ee100ea19dae3de5f2e3",
            branch_pr,
        )
        self.assertIn(
            "if: (github.event_name == 'push' || github.event_name == 'workflow_dispatch') "
            "&& github.ref == 'refs/heads/codex/dsga-neon-v3-20261019'",
            branch_pr,
        )
        self.assertRegex(
            branch_pr,
            r"uses: william-lbn/neon/\.github/workflows/dsga-release\.yml@[a-f0-9]{40}",
        )
        self.assertIn("      normal_guest: false", branch_pr)
        self.assertNotIn("secrets: inherit", branch_pr)
        self.assertIn(
            "DOCKERHUB_USERNAME: ${{ secrets.DOCKERHUB_USERNAME }}",
            branch_pr,
        )
        self.assertIn(
            "DOCKERHUB_TOKEN: ${{ secrets.DOCKERHUB_TOKEN }}",
            branch_pr,
        )
        self.assertIn("  workflow_dispatch:", workflow)

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

    def test_candidate_retains_complete_control_plane_source_inventory(self):
        result = self.manifest()
        control = result["source_lock"]["control_plane"]
        self.assertEqual(len(control["images"]), 9)
        self.assertTrue(all(result["images"][image]["source_commit"] == control["commit"] for image in control["images"]))
        self.assertNotIn("control-api", release.expected_images(self.lock, "legacy"))

    def test_normal_guest_requires_the_exact_shared_build_plan_and_package_lock(self):
        inputs = dsga_release.verify_normal_guest_input_lock(self.lock)
        self.assertEqual(inputs["architecture"], "linux/amd64")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "compute").mkdir()
            (root / "compute/normal-build-plan-amd64.json").write_bytes(b"modified input\n")
            with self.assertRaisesRegex(ValueError, "differs from the locked bytes"):
                dsga_release.verify_normal_guest_input_lock(self.lock, root)
            plan = "compute/normal-build-plan-amd64.json"
            (root / plan).write_bytes((dsga_release.ROOT / plan).read_bytes())
            (root / "compute/normal-ordinary-package-lock-amd64.json").write_bytes(b"modified package source lock\n")
            with self.assertRaisesRegex(ValueError, "ordinary package lock differs from the locked bytes"):
                dsga_release.verify_normal_guest_input_lock(self.lock, root)
        self.lock["normal_guest_inputs"]["ordinary_package_lock_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "independently pinned SHA256"):
            dsga_release.verify_normal_guest_input_lock(self.lock)

    def test_control_plane_receipts_are_required_and_locked(self):
        original = self.images.pop("control-api")
        with self.assertRaisesRegex(ValueError, "Distribution incomplete"):
            self.manifest()
        self.images["control-api"] = original
        for field, value, error in [
            ("source_commit", self.neon, "locked commit"),
            ("source_repository", "william-lbn/neon", "source repository"),
            ("architecture", "linux/arm64", "architecture"),
            ("execution", {**self.execution, "run_attempt": "1"}, "run/attempt"),
        ]:
            with self.subTest(field=field):
                prior = self.images["control-api"][field]
                self.images["control-api"][field] = value
                with self.assertRaisesRegex(ValueError, error):
                    self.manifest()
                self.images["control-api"][field] = prior

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

    def test_vm_disk_proof_must_match_its_output_and_each_input(self):
        name = "vm-compute-node-v18"
        disk = self.images[name].pop("guest_disk")
        with self.assertRaisesRegex(ValueError, "VM disk proof"):
            self.manifest()
        self.images[name]["guest_disk"] = disk
        for field, value in [
            ("image", self.images["vm-compute-node-v17"]["guest_disk"]["image"]),
            ("inputs", {**disk["inputs"], "compute": release.image_reference(self.images["compute-node-v17"])}),
            ("inputs", {**disk["inputs"], "daemon": release.image_reference(self.images["vm-builder"])}),
            ("sha256", "truncated"), ("system_mode", "normal-split-v3"), ("virtual_size", True),
        ]:
            with self.subTest(field=field):
                self.images[name]["guest_disk"] = {**disk, field: value}
                with self.assertRaisesRegex(ValueError, "VM disk"):
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

    def test_native_receipt_loader_requires_one_valid_postgres_binary_checksum(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for major in (17, 18):
                evidence = root / f"pg{major}" / "build" / "evidence"
                evidence.mkdir(parents=True)
                (evidence / "source-manifest.json").write_text(json.dumps({
                    "kind": "postgres-native-validation",
                    "schema_version": 1,
                    "pg_major": major,
                }))
                (evidence / "postgres.sha256").write_text(
                    ("a" if major == 17 else "b") * 64
                    + "  /home/runner/work/postgres/postgres\n"
                )
            receipts = release.load_native_receipts(root)
            self.assertEqual(set(receipts), {"v17", "v18"})
            self.assertEqual(receipts["v17"]["postgres_binary_sha256"], "a" * 64)
            self.assertEqual(receipts["v18"]["postgres_binary_sha256"], "b" * 64)

            checksum = root / "pg18" / "build" / "evidence" / "postgres.sha256"
            checksum.write_text("bad-checksum  /home/runner/work/postgres/postgres\n")
            with self.assertRaisesRegex(ValueError, "invalid postgres binary checksum"):
                release.load_native_receipts(root)

    def test_native_receipt_loader_rejects_missing_or_ambiguous_binary_checksums(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            evidence = root / "pg17"
            evidence.mkdir()
            (evidence / "source-manifest.json").write_text(json.dumps({"pg_major": 17}))
            with self.assertRaisesRegex(ValueError, "missing postgres.sha256"):
                release.load_native_receipts(root)

            checksum = evidence / "postgres.sha256"
            checksum.write_text("a" * 64 + "  /tmp/not-postgres\\n")
            with self.assertRaisesRegex(ValueError, "invalid postgres binary checksum"):
                release.load_native_receipts(root)

            checksum.write_text("a" * 64 + "  /tmp/postgres\\n" + "b" * 64 + "  /tmp/postgres\\n")
            with self.assertRaisesRegex(ValueError, "exactly one binary checksum"):
                release.load_native_receipts(root)

    def test_native_receipt_loader_rejects_non_integer_postgres_major(self):
        for major in (True, 17.0, "17", None):
            with self.subTest(major=major), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                evidence = root / "evidence"
                evidence.mkdir()
                (evidence / "source-manifest.json").write_text(json.dumps({"pg_major": major}))
                with self.assertRaisesRegex(ValueError, "Malformed native PostgreSQL source manifest"):
                    release.load_native_receipts(root)

    def test_native_receipt_loader_rejects_duplicate_major_and_missing_receipts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, "No native PostgreSQL source manifests"):
                release.load_native_receipts(root)
            for name in ("first", "duplicate"):
                evidence = root / name
                evidence.mkdir()
                (evidence / "source-manifest.json").write_text(json.dumps({"pg_major": 17}))
                (evidence / "postgres.sha256").write_text("c" * 64 + "  /tmp/postgres\n")
            with self.assertRaisesRegex(ValueError, "Conflicting/duplicate"):
                release.load_native_receipts(root)

    def test_native_pg_receipt_schema_version_is_locked(self):
        self.native["v18"]["schema_version"] = 2
        with self.assertRaisesRegex(ValueError, "Native PG receipt mismatch"):
            self.manifest()

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
