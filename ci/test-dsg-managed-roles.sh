#!/usr/bin/env bash
# 受管角色真实门禁：默认cargo test的ignored不作为通过，必须显式运行真实PG。
# 仅独占临时集群；不接收用户数据库URL、不构建镜像、不准备或执行部署。
set -euo pipefail
if [[ $# != 1 ]]; then
    echo "usage: $0 POSTGRES_INSTALL_DIR" >&2
    exit 2
fi
task_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
pg_install=$(cd "$1" && pwd)
cd "$task_root"
python3 ci/compute.py validate --source-ref index --require-pg-checkout v17
export NEON_TEST_DSG_EXPECTED_SHA
NEON_TEST_DSG_EXPECTED_SHA=$(python3 - "$task_root" "$pg_install" <<'PY'
import json
from pathlib import Path
import subprocess
import sys
root, installed = map(Path, sys.argv[1:])
sha = json.loads((root / 'vendor/revisions.json').read_text())['v17'][1]
version = subprocess.check_output([str(installed / 'bin/postgres'), '--version'], text=True)
if f'({sha})' not in version or not version.startswith('postgres (PostgreSQL) 17.'):
    raise SystemExit('managed-role gate requires the exact pinned PG17 binary')
print(sha)
PY
)
export NEON_TEST_PG_BIN="$pg_install/bin"
task_report=$(mktemp -d)
trap 'rm -rf "$task_report"' EXIT
# 包含本crate全部lib测试：旧逻辑、角色策略及HBA/真实PG一起运行，不能用过滤或
# 零用例退出0伪造成功。已有真实集群失败会由夹具保留独占目录/日志。
cargo test --locked -p compute_tools --lib -- --include-ignored --nocapture --test-threads=1 \
    2>&1 | tee "$task_report/tests.log"
python3 - "$task_report/tests.log" <<'PY'
from pathlib import Path
import re
import sys
text = Path(sys.argv[1]).read_text()
reports = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;', text)
if len(reports) != 1 or int(reports[0][0]) < 24 or tuple(map(int, reports[0][1:])) != (0,0,0,0):
    raise SystemExit('managed-role library gate failed, skipped, filtered or did not run fully')
for name in ['native_roles_use_actual_dispatch_and_preserve_admission_and_replay',
             'native_fault_rollback_lock_deadline_and_session_identity',
             'native_replica_attests_replayed_roles_without_catalog_writes']:
    if f'test dsg_roles::tests::{name} ... ok' not in text:
        raise SystemExit(f'required native role case did not pass: {name}')
# HBA实际启动会输出PG日志，test名与末尾ok不一定同一行；全suite零失败/跳过/
# 过滤且数量完整，再要求该原生用例确实出现，避免漏跑文件后用旧18项冒充。
if 'test dsg_hba::tests::native_hba_peer_hold_scram_jwt_reload_proof_and_startup_overrides ...' not in text:
    raise SystemExit('required native HBA startup/activation proof case is missing')
print('Actual managed-role lib/PG/physical-standby gate passed; zero skip/ignored/filtered')
PY
