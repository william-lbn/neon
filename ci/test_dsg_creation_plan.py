#!/usr/bin/env python3
"""从自有控制面本次源码导出启动契约，并交真实Rust DTO消费，不使用陈旧快照。

本脚本不启动PG/VM，不下载代码、不构建镜像或部署；跨语言序列化通过不代表
完整创建/权限/预算闭环通过。Go/Rust工具链、SDK和依赖缓存由调用环境提供。
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


def fingerprint(paths):
    return {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}


def run(args, cwd, env, log, timeout):
    with log.open("w") as output:
        subprocess.run(
            args,
            cwd=cwd,
            env=env,
            stdout=output,
            stderr=subprocess.STDOUT,
            timeout=timeout,
            check=True,
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("control_plane", type=Path, help="自有control-plane实际源码目录")
    parser.add_argument("--report-dir", type=Path, help="新的证据目录；已存在时拒绝覆盖")
    args = parser.parse_args()
    neon = Path(__file__).resolve().parents[1]
    control = args.control_plane.resolve()
    sources = [
        control / "api/internal/control/dsga_creation_plan.go",
        control / "api/internal/control/dsga_creation_plan_test.go",
        neon / "libs/compute_api/src/dynamic_shared_buffers.rs",
        neon / "libs/compute_api/src/dsg_control.rs",
        neon / "libs/compute_api/tests/dsg_creation_plan.rs",
        Path(__file__).resolve(),
    ]
    before = fingerprint(sources)
    # 每次使用新目录。旧Go没有导出器时，不能靠go test“无匹配测试也退出0”或
    # 上次遗留的JSON假装本轮互通；文件存在和准确pass事件必须同时满足。
    if args.report_dir:
        report = args.report_dir.resolve()
        report.mkdir()
    else:
        report = Path(tempfile.mkdtemp(prefix="neon-dsga-plan-"))
    print(f"creation-plan evidence: {report}", flush=True)
    contract = report / "go-contract.json"
    env = os.environ.copy()
    env["NEON_DSGA_NATIVE_PLAN_CONTRACT"] = str(contract)
    metadata = {"status": "running", "source_sha256": before}
    (report / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    try:
        run(
            [
                "go", "test", "-race", "-count=1", "-timeout=90s", "-json",
                "./internal/control", "-run", "^TestDSGACreationPlanNativeContractExport$",
            ],
            control / "api", env, report / "go.jsonl", 180,
        )
        events = [json.loads(line) for line in (report / "go.jsonl").read_text().splitlines()]
        passes = [event for event in events if event.get("Action") == "pass"
                  and event.get("Test") == "TestDSGACreationPlanNativeContractExport"]
        if len(passes) != 1 or not contract.is_file():
            raise RuntimeError("本次Go生产者未实际运行/未导出，不能使用旧契约")
        cases = json.loads(contract.read_bytes())
        if len(cases) != 11 or len({case["name"] for case in cases}) != 11:
            raise RuntimeError("Go导出样本不完整或名字重复")
        run(
            [
                "cargo", "test", "--locked", "-p", "compute_api", "--test", "dsg_creation_plan",
                "go_frozen_creation_plan_matches_native_bootstrap_contract", "--", "--ignored", "--exact",
            ],
            neon, env, report / "rust.log", 600,
        )
        if "test result: ok. 1 passed; 0 failed; 0 ignored;" not in (report / "rust.log").read_text():
            raise RuntimeError("原生Rust契约测试没有实际运行通过")
        if before != fingerprint(sources):
            raise RuntimeError("验证期间源码变化，结果不能绑定最终提交")
        metadata.update(
            status="passed", go_pass_events=1, rust_pass_tests=1, contract_cases=len(cases),
            valid_cases=sum(case["valid"] for case in cases),
            invalid_cases=sum(not case["valid"] for case in cases),
            contract_sha256=hashlib.sha256(contract.read_bytes()).hexdigest(),
        )
    except BaseException:
        metadata["status"] = "failed"
        raise
    finally:
        # 失败保留日志，且即使命令异常也记录实际恢复状态；不把异常吞成绿色结果。
        metadata["source_unchanged"] = before == fingerprint(sources)
        (report / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print("Go exporter + native Rust: passed, 11 cases (2 valid / 9 rejected)", flush=True)


if __name__ == "__main__":
    main()
