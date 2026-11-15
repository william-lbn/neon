"""Evaluate the measured SLO portion of a DSGA release acceptance."""

import hashlib
import json
import math
from pathlib import Path


LIMITS = {
    "sql_success_ratio_min": "sql_success_ratio",
    "sql_p99_ms_max": "sql_p99_ms",
    "sql_p99_regression_ratio_max": "sql_p99_regression_ratio",
    "grow_duration_ms_max": "grow_duration_ms",
    "shrink_duration_ms_max": "shrink_duration_ms",
    "observer_cpu_overhead_ratio_max": "observer_cpu_overhead_ratio",
}
REQUIRED_ASSERTIONS = frozenset({
    "no_data_mismatch", "actual_normal_lowered", "no_duplicate_release", "no_unexpected_restart",
})
SCOPE_KEYS = frozenset({"release", "profile", "workload"})


def _object(pairs):
    obj = {}
    for key, value in pairs:
        if key in obj:
            raise ValueError(f"Duplicate JSON key {key}")
        obj[key] = value
    return obj


def _json(raw):
    return json.loads(raw, object_pairs_hook=_object)


def _metric(value, metric):
    if type(value) not in (int, float) or not -1 <= value <= 2**53 or not math.isfinite(value):
        raise ValueError(f"Missing or invalid measurement/threshold for {metric}")
    if metric == "sql_success_ratio" and not 0 <= value <= 1:
        raise ValueError("SQL success ratios must be between 0 and 1")
    if metric.endswith("_ms") and value < 0:
        raise ValueError(f"Negative duration for {metric}")
    return value


def _record(document, name):
    value = document[name]
    if not isinstance(value, dict):
        raise ValueError(f"SLO {name} must be a JSON object")
    return value


def evaluate(policy, evidence, version, distribution_sha256):
    if policy["calibration_status"] != "CALIBRATED":
        raise ValueError("Production SLO thresholds are not calibrated")
    scope = policy["scope"]
    if (not isinstance(scope, dict) or scope.keys() != SCOPE_KEYS or
            not all(isinstance(v, str) and v.strip() for v in scope.values()) or
            scope["release"] != version or evidence["scope"] != scope or
            evidence["distribution_manifest_sha256"] != distribution_sha256):
        raise ValueError("SLO evidence differs from the exact release, profile or workload")

    assertions = policy["hard_assertions"]
    if (not isinstance(assertions, list) or not all(isinstance(name, str) for name in assertions) or
            len(set(assertions)) != len(assertions) or not REQUIRED_ASSERTIONS.issubset(assertions)):
        raise ValueError("Required SLO hard assertions are missing or duplicated")
    observations = _record(evidence, "hard_assertions")
    failed = [name for name in assertions if observations.get(name) is not True]
    if failed:
        raise ValueError("SLO hard assertions failed: " + ", ".join(failed))

    thresholds = _record(policy, "metrics")
    metrics = _record(evidence, "metrics")
    expected_counts = _record(policy, "required_samples")
    counts = _record(evidence, "samples")
    summary = {}
    for threshold, name in LIMITS.items():
        limit = _metric(thresholds.get(threshold), name)
        observation = _metric(metrics.get(name), name)
        required = expected_counts.get(name)
        count = counts.get(name)
        if type(required) is not int or required <= 0 or type(count) is not int or count < required:
            raise ValueError(f"The policy requires measured samples for {name}")
        if (threshold.endswith("_min") and observation < limit) or (threshold.endswith("_max") and observation > limit):
            raise ValueError(f"SLO threshold failed for {name}")
        summary[name] = {"measured": observation, "threshold": limit, "samples": count}

    return {"scope": scope, "distribution_manifest_sha256": distribution_sha256,
            "slo_passed": True, "hard_assertions": assertions, "metrics": summary}


def verify(policy_path, evidence_path, distribution_path):
    manifest = Path(distribution_path).read_bytes()
    return evaluate(_json(Path(policy_path).read_bytes()), _json(Path(evidence_path).read_bytes()),
                    _json(manifest)["version"], hashlib.sha256(manifest).hexdigest())
