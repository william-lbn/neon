import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import slo_acceptance as slo


VERSION = "0.2.0-dsga.12345.1.amd64"
DIGEST = "1" * 64


class SloAcceptanceTests(unittest.TestCase):
    def setUp(self):
        scope = {"release": VERSION, "profile": "fixture-7gi", "workload": "fixture-abba"}
        self.policy = {
            "calibration_status": "CALIBRATED", "scope": scope,
            "metrics": {key: 1 for key in slo.LIMITS},
            "required_samples": {name: 6 for name in slo.LIMITS.values()},
            "hard_assertions": sorted(slo.REQUIRED_ASSERTIONS),
        }
        self.evidence = {
            "scope": dict(scope), "distribution_manifest_sha256": DIGEST,
            "metrics": {name: .8 for name in slo.LIMITS.values()},
            "samples": {name: 8 for name in slo.LIMITS.values()},
            "hard_assertions": dict.fromkeys(slo.REQUIRED_ASSERTIONS, True),
        }
        self.evidence["metrics"]["sql_success_ratio"] = 1

    def evaluate(self):
        return slo.evaluate(self.policy, self.evidence, VERSION, DIGEST)

    def test_calibrated_exact_qualified_measurement(self):
        report = self.evaluate()
        self.assertTrue(report["slo_passed"])
        self.assertEqual(report["metrics"]["sql_p99_ms"], {"measured": .8, "threshold": 1, "samples": 8})
        self.assertEqual(report["distribution_manifest_sha256"], DIGEST)

    def test_unmeasured_or_invalid_threshold_rejected(self):
        self.policy["calibration_status"] = "NOT_MEASURED"
        with self.assertRaisesRegex(ValueError, "not calibrated"):
            self.evaluate()
        self.policy["calibration_status"] = "CALIBRATED"
        for invalid in (None, False, float("nan"), -1):
            with self.subTest(invalid=invalid):
                self.policy["metrics"]["sql_p99_ms_max"] = invalid
                with self.assertRaisesRegex(ValueError, "sql_p99_ms"):
                    self.evaluate()

    def test_failed_hard_fact_or_incomplete_samples_rejected(self):
        self.evidence["hard_assertions"]["actual_normal_lowered"] = False
        with self.assertRaisesRegex(ValueError, "actual_normal_lowered"):
            self.evaluate()
        self.evidence["hard_assertions"]["actual_normal_lowered"] = True
        self.evidence["samples"]["shrink_duration_ms"] = 5
        with self.assertRaisesRegex(ValueError, "samples for shrink_duration_ms"):
            self.evaluate()

    def test_different_profile_or_candidate_bytes_rejected(self):
        self.evidence["scope"]["profile"] = "another-profile"
        with self.assertRaisesRegex(ValueError, "exact release"):
            self.evaluate()
        self.evidence["scope"]["profile"] = self.policy["scope"]["profile"]
        self.evidence["distribution_manifest_sha256"] = "2" * 64
        with self.assertRaisesRegex(ValueError, "exact release"):
            self.evaluate()

    def test_both_lower_and_upper_measured_thresholds_are_enforced(self):
        self.evidence["metrics"]["sql_success_ratio"] = .99
        with self.assertRaisesRegex(ValueError, "threshold failed for sql_success_ratio"):
            self.evaluate()
        self.evidence["metrics"]["sql_success_ratio"] = 1
        self.evidence["metrics"]["sql_p99_ms"] = 1.01
        with self.assertRaisesRegex(ValueError, "threshold failed for sql_p99_ms"):
            self.evaluate()

    def test_disk_inputs_bind_raw_distribution_and_reject_duplicate_keys(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps({"version": VERSION})+"\n")
            policy, evidence = root / "policy.json", root / "evidence.json"
            policy.write_text(json.dumps(self.policy))
            self.evidence["distribution_manifest_sha256"] = hashlib.sha256(manifest.read_bytes()).hexdigest()
            evidence.write_text(json.dumps(self.evidence))
            self.assertTrue(slo.verify(policy, evidence, manifest)["slo_passed"])
            manifest.write_text('{"version": "old", "version": "'+VERSION+'"}')
            with self.assertRaisesRegex(ValueError, "Duplicate JSON key version"):
                slo.verify(policy, evidence, manifest)
