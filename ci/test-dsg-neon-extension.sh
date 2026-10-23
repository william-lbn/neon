#!/usr/bin/env bash
# 在准确锁定的Neon PG17上构建并加载完整Neon扩展，真实运行存储兼容专项。
# 只使用调用方的开发前缀，不构建镜像/部署，不连接已有数据库。缺环境、
# 编译失败、零测试或skip均直接失败，不能以核心rmgr测试替代完整扩展验证。
set -euo pipefail

if [[ $# != 2 ]]; then
    echo "usage: $0 POSTGRES_BUILD_DIR POSTGRES_INSTALL_DIR" >&2
    exit 2
fi

task_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
pg_build=$(cd "$1" && pwd)
pg_install=$(cd "$2" && pwd)
pg_config="$pg_install/bin/pg_config"
if [[ ! -x "$pg_config" || ! -f "$pg_build/GNUmakefile" ]]; then
    echo "expected a configured PG build and its installed pg_config" >&2
    exit 2
fi
if [[ $("$pg_config" --configure) != *--enable-injection-points* ]]; then
    echo "Neon extension tests require --enable-injection-points" >&2
    exit 2
fi
cd "$task_root"
python3 ci/compute.py validate --source-ref index --require-pg-checkout v17
python3 - "$task_root" "$pg_install" <<'PY'
import json
from pathlib import Path
import subprocess
import sys

root, installed = map(Path, sys.argv[1:])
expected = json.loads((root / 'vendor/revisions.json').read_text())['v17'][1]
actual = subprocess.check_output([str(installed / 'bin/postgres'), '--version'], text=True)
if f'({expected})' not in actual or not actual.startswith('postgres (PostgreSQL) 17.'):
    raise SystemExit(f'PG17 binary does not match source lock: {actual.strip()}')
PY

# CARGO_TARGET_DIR明确为绝对路径，避免在communicator子目录中解析成另一份产物。
cargo_output=$(python3 - "${CARGO_TARGET_DIR:-$task_root/target}" <<'PY'
from pathlib import Path
import sys
print(Path(sys.argv[1]).resolve())
PY
)
export CARGO_TARGET_DIR="$cargo_output"
extension_build="$pg_build/dsga-neon-extensions"
mkdir -p "$extension_build/neon" "$extension_build/neon_test_utils"

# 分别clean，防止已有对象来自旧PG头/ABI。旧make对分组目标可能重复调用cargo，
# 外层使用-j1；Rust内部并发仍由CARGO_BUILD_JOBS控制，所有依赖遵守Cargo.lock。
for extension in neon neon_test_utils; do
    make -C "$extension_build/$extension" -f "$task_root/pgxn/$extension/Makefile" \
        PG_CONFIG="$pg_config" NEON_CARGO_ARTIFACT_TARGET_DIR="$cargo_output/debug" CARGO_PROFILE= clean
    make -j1 -C "$extension_build/$extension" -f "$task_root/pgxn/$extension/Makefile" \
        PG_CONFIG="$pg_config" NEON_CARGO_ARTIFACT_TARGET_DIR="$cargo_output/debug" \
        CARGO_BUILD_FLAGS="${CARGO_BUILD_FLAGS:---locked}" CARGO_PROFILE= install
done
make -C "$task_root/pgxn/neon_rmgr" PG_CONFIG="$pg_config" clean
make -C "$task_root/pgxn/neon_rmgr" PG_CONFIG="$pg_config" install
make -C "$pg_build/src/test/modules/test_dsg_region" install
make -C "$pg_build/src/test/modules/injection_points" install

export NEON_TEST_PG_BIN="$pg_install/bin"
export NEON_TEST_DSG_LIBRARY="$("$pg_config" --pkglibdir)/test_dsg_region"
export NEON_TEST_RMGR_LIBRARY="$("$pg_config" --pkglibdir)/neon_rmgr"
export NEON_TEST_NEON_LIBRARY="$("$pg_config" --pkglibdir)/neon"
export NEON_TEST_UTILS_LIBRARY="$("$pg_config" --pkglibdir)/neon_test_utils"
cd "$task_root/ci"
python3 - <<'PY'
import unittest

suite = unittest.defaultTestLoader.discover('.', pattern='test_dsg_neon_extension.py')
suite.addTests(unittest.defaultTestLoader.discover('.', pattern='test_dsg_neon_control.py'))
admission = unittest.defaultTestLoader.discover('.', pattern='test_dsg_neon_admission.py')
if admission.countTestCases() < 2:
    raise SystemExit('DSGA formal Neon admission suite is missing required cases')
suite.addTests(admission)
admission_status = unittest.defaultTestLoader.discover('.', pattern='test_dsg_admission_status.py')
if admission_status.countTestCases() < 2:
    raise SystemExit('DSGA readonly admission reconciliation suite is missing required cases')
suite.addTests(admission_status)
shared_cost = unittest.defaultTestLoader.discover('.', pattern='test_dsg_shared_cost.py')
if shared_cost.countTestCases() < 2:
    raise SystemExit('DSGA native shared cost suite is missing required cases')
suite.addTests(shared_cost)
if suite.countTestCases() == 0:
    raise SystemExit('Neon extension suite contains zero tests')
result = unittest.TextTestRunner(verbosity=2).run(suite)
if not result.wasSuccessful() or result.skipped:
    raise SystemExit('Neon extension suite failed or skipped required tests')
PY
