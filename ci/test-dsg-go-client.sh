#!/usr/bin/env bash
# 显式跨仓库门禁：编译自有Autoscaling Go race测试程序，由真实Rust/PG夹具驱动。
# 不连接用户服务，不构建镜像/部署；失败即非零，不以ignored/零项代替执行。
set -euo pipefail
if [[ $# != 2 ]]; then
    echo "usage: $0 POSTGRES_INSTALL_DIR AUTOSCALING_SOURCE_DIR" >&2
    exit 2
fi
task_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
pg_install=$(cd "$1" && pwd)
go_source=$(cd "$2" && pwd)
cd "$task_root"
python3 ci/compute.py validate --source-ref index --require-pg-checkout v17
expected_sha=$(python3 - "$task_root" "$pg_install" "$go_source" <<'PY'
import json
from pathlib import Path
import subprocess
import sys
root, installed, go = map(Path, sys.argv[1:])
expected = json.loads((root / 'vendor/revisions.json').read_text())['v17'][1]
version = subprocess.check_output([str(installed / 'bin/postgres'), '--version'], text=True)
if not version.startswith('postgres (PostgreSQL) 17.') or f'({expected})' not in version:
    raise SystemExit('native Go gate binary does not match PG17 source lock')
if 'module github.com/neondatabase/autoscaling' not in (go / 'go.mod').read_text():
    raise SystemExit('unexpected Go module')
if not (go / 'pkg/dsga/compute/native_e2e_test.go').is_file():
    raise SystemExit('native Go test source is missing')
print(expected)
PY
)
task_reports=$(mktemp -d)
trap 'rm -rf "$task_reports"' EXIT
export NEON_TEST_PG_BIN="$pg_install/bin"
export NEON_TEST_DSG_EXPECTED_SHA="$expected_sha"
export NEON_TEST_DSG_GO_BIN="$task_reports/dsg_go_test"
git -C "$go_source" rev-parse HEAD
# 提交前候选可有改动，但必须公开标出参与编译的源码状态，不能只打印旧HEAD
# 就宣称执行了准确已提交版本。所有者AGENTS.md等非代码修改不混入该记录。
git -C "$go_source" status --short -- pkg/api/dsga pkg/dsga go.mod go.sum
go version
(
    cd "$go_source"
    go test -race -tags dsga_native_e2e -c -o "$NEON_TEST_DSG_GO_BIN" ./pkg/dsga/compute
)
"$NEON_TEST_DSG_GO_BIN" -test.list '^TestNativeComputeHTTPChain$' > "$task_reports/go-list"
python3 - "$task_reports/go-list" <<'PY'
from pathlib import Path
import sys
if Path(sys.argv[1]).read_text().splitlines() != ['TestNativeComputeHTTPChain']:
    raise SystemExit('native Go gate must contain exactly one required test')
PY
cargo test --locked -p compute_tools --test dsg_go_http -- --include-ignored --nocapture --test-threads=1 \
    2>&1 | tee "$task_reports/run"
python3 - "$task_reports/run" <<'PY'
from pathlib import Path
import re
import sys
text = Path(sys.argv[1]).read_text()
reports = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; '
                     r'(\d+) measured; (\d+) filtered out;', text)
if len(reports) != 1 or tuple(map(int, reports[0])) != (1, 0, 0, 0, 0):
    raise SystemExit('native cross-repository Rust gate did not execute fully')
if text.count('--- PASS: TestNativeComputeHTTPChain') != 2 or '--- SKIP:' in text or '--- FAIL:' in text:
    raise SystemExit('native Go gate did not execute fully')
if text.count('DSGA_NATIVE_OWNER_LOOP epoch=2 buffers=4096 journal=recorded replay=confirmed') != 1:
    raise SystemExit('native Go gate did not execute the confirmed ownership and persistent PG loop phase')
print('Actual Go -> TLS -> Compute Router -> SQL -> PG gate passed in both normal and post-restart processes; zero skip/ignored/filtered')
PY
