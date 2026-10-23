#!/usr/bin/env python3
"""新鲜Go Renderer与真实Rust Compute解析器的定向门禁，不启动PG/VM/部署。"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile

from test_dsg_creation_plan import fingerprint, run


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("control_plane", type=Path)
    parser.add_argument("--report-dir", type=Path)
    args = parser.parse_args()
    neon = Path(__file__).resolve().parents[1]
    control = args.control_plane.resolve()
    sources = [
        control / "api/internal/control/dsga_renderer.go",
        control / "api/internal/control/dsga_renderer_test.go",
        control / "api/internal/control/dsga_creation.go",
        control / "api/internal/control/dsga_creation_plan.go",
        control / "api/internal/functionsql/identity.go",
        neon / "compute_tools/tests/dsg_renderer_contract.rs",
        neon / "compute_tools/src/compute.rs",
        neon / "compute_tools/src/dsg_roles.rs",
        neon / "libs/compute_api/src/spec.rs",
        neon / "libs/compute_api/src/responses.rs",
        Path(__file__).resolve(),
        Path(__file__).with_name("test_dsg_creation_plan.py"),
    ]
    before = fingerprint(sources)
    if args.report_dir:
        report = args.report_dir.resolve()
        report.mkdir()
    else:
        report = Path(tempfile.mkdtemp(prefix="neon-dsga-renderer-"))
    print(f"renderer evidence: {report}", flush=True)
    contract = report / "go-renderer.json"
    env = os.environ.copy()
    env["NEON_DSGA_RENDERER_CONTRACT"] = str(contract)
    metadata = {"status": "running", "source_sha256": before}
    try:
        # 新增Renderer用例及与已有创建屏障的组合；不重跑未变的旧plan/PG/SDK测试。
        run(["go", "test", "-race", "-count=1", "-timeout=90s", "-json",
             "./internal/control", "-run", "^TestDSGARenderer"],
            control / "api", env, report / "go.jsonl", 180)
        events = [json.loads(line) for line in (report / "go.jsonl").read_text().splitlines()]
        required = {"TestDSGARendererDeterministicPrimaryAndReplicaSafety",
                    "TestDSGARendererRejectsUnsafeInputsWithoutPartialOutput",
                    "TestDSGARendererUsesOriginalUIDAndImmutableCreationBarrier",
                    "TestDSGARendererNativeContractExport"}
        passed = {e.get("Test") for e in events if e.get("Action") == "pass"}
        if not required <= passed or any(e.get("Action") in {"skip", "fail"} for e in events):
            raise RuntimeError("Renderer用例未完整实际执行，不能用零用例或跳过冒充")
        samples = json.loads(contract.read_bytes())
        if len(samples) != 2 or {v["name"] for v in samples} != {"read_write", "read_only"}:
            raise RuntimeError("必须由本次Go Renderer同时输出主/只读原生配置")
        run(["cargo", "test", "--locked", "-p", "compute_tools", "--test", "dsg_renderer_contract",
             "go_safe_renderer_is_accepted_by_actual_compute_parser", "--", "--ignored", "--exact"],
            neon, env, report / "rust.log", 600)
        if "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;" not in (report / "rust.log").read_text():
            raise RuntimeError("真实Rust Compute解析器未执行通过")
        if before != fingerprint(sources):
            raise RuntimeError("验证中源码已变，不能认证最终代码")
        metadata.update(status="passed", go_top_level_passed=4,
                        go_pass_events=sum(e.get("Action") == "pass" and "Test" in e for e in events),
                        rust_passed=1, samples=2,
                        contract_sha256=hashlib.sha256(contract.read_bytes()).hexdigest())
    finally:
        if metadata["status"] == "running":
            metadata["status"] = "failed"
        metadata["source_unchanged"] = before == fingerprint(sources)
        (report / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print("Renderer Go4 + Rust1 passed; 2 fresh modes, zero skip/ignored/filtered", flush=True)


if __name__ == "__main__":
    main()
