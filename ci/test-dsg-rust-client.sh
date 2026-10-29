#!/usr/bin/env bash
# 在前序完整Neon安装后的独立PG临时集群运行SDK闭环。原生集群用例默认ignored，
# 这里显式include-ignored并校验列出/通过数量，防止零测试或未执行成为绿色门禁。
# 只使用宿主PG，不构建镜像、不部署、不接入用户已有数据库。
set -euo pipefail
if [[ $# != 1 ]]; then
    echo "usage: $0 POSTGRES_INSTALL_DIR" >&2
    exit 2
fi
task_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
pg_install=$(cd "$1" && pwd)
cd "$task_root"
python3 ci/compute.py validate --source-ref index --require-pg-checkout v17
expected_sha=$(python3 - "$task_root" "$pg_install" <<'PY'
import json
from pathlib import Path
import subprocess
import sys
root, installed = map(Path, sys.argv[1:])
expected = json.loads((root / 'vendor/revisions.json').read_text())['v17'][1]
version = subprocess.check_output([str(installed / 'bin/postgres'), '--version'], text=True)
if not version.startswith('postgres (PostgreSQL) 17.') or f'({expected})' not in version:
    raise SystemExit('SDK test binary does not match PG17 source lock')
print(expected)
PY
)
export NEON_TEST_PG_BIN="$pg_install/bin"
export NEON_TEST_DSG_EXPECTED_SHA="$expected_sha"
task_reports=$(mktemp -d)
trap 'rm -rf "$task_reports"' EXIT
run_suite() {
    local task_package=$1 task_suite=$2
    cargo test --locked -p "$task_package" --features compute_api/postgres-client --test "$task_suite" \
        -- --list > "$task_reports/list"
    python3 - "$task_reports/list" <<'PY'
from pathlib import Path
import sys
names = [line for line in Path(sys.argv[1]).read_text().splitlines() if line.endswith(': test')]
if not names:
    raise SystemExit('DSGA real Rust client suite contains zero tests')
PY
    cargo test --locked -p "$task_package" --features compute_api/postgres-client --test "$task_suite" \
        -- --include-ignored --nocapture --test-threads=1 2>&1 | tee "$task_reports/run"
    python3 - "$task_reports/list" "$task_reports/run" <<'PY'
from pathlib import Path
import re
import sys
listed, output = map(lambda path: Path(path).read_text(), sys.argv[1:])
count = sum(line.endswith(': test') for line in listed.splitlines())
reports = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; '
                     r'(\d+) measured; (\d+) filtered out;', output)
if len(reports) != 1 or tuple(map(int, reports[0])) != (count, 0, 0, 0, 0):
    raise SystemExit('DSGA Rust client suite did not execute every required test')
print(f'All {count} DSGA suite tests executed successfully; zero ignored/filtered')
PY
}
# 完整SDK原生集群，以及Compute真实Router（含普通认证门禁和原生HTTP链路），
# 分别校验非零/全量。不能只执行普通认证项而让原生HTTP/冻结契约集成保持ignored。
run_suite compute_api dsg_admission
run_suite compute_api dsg_admission_continuation
run_suite compute_api dsg_admission_continuation_native
run_suite compute_api dsg_shared_cost
run_suite compute_api dsg_postgres_client
run_suite compute_tools dsg_http
