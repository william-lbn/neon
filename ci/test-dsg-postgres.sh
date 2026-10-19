#!/usr/bin/env bash
# 在实际锁定的PG17构建上运行全部DSGA临时集群测试。与prepare逻辑检查分开：
# 必须构建/安装真实测试库并显式提供全部环境，缺环境或零测试不能产生绿色门禁。
# 不构建镜像、不连接现有数据库；Linux CI通过也不代表NeonVM/HugeTLB认证。
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
    echo "DSGA lifecycle tests require --enable-injection-points" >&2
    exit 2
fi

# 能力和真实源码身份均要检查。当前先完成PG17，不能把其他大版本静默当成已测。
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

make -C "$pg_build/src/test/modules/test_dsg_region" -j"${BUILD_JOBS:-4}" install
# 主仓扩展的头文件依赖不保证覆盖所有API变化；清理后重编，避免旧ABI库混入。
make -C "$task_root/pgxn/neon_rmgr" PG_CONFIG="$pg_config" clean
make -C "$task_root/pgxn/neon_rmgr" PG_CONFIG="$pg_config" all
export NEON_TEST_PG_BIN="$pg_install/bin"
export NEON_TEST_DSG_LIBRARY="$("$pg_config" --pkglibdir)/test_dsg_region"
export NEON_TEST_RMGR_LIBRARY="$task_root/pgxn/neon_rmgr/neon_rmgr"
cd "$task_root/ci"
python3 - <<'PY'
import unittest

suite = unittest.defaultTestLoader.discover('.', pattern='test_dsg_postgres.py')
if suite.countTestCases() == 0:
    raise SystemExit('DSGA lifecycle suite contains zero tests')
result = unittest.TextTestRunner(verbosity=2).run(suite)
if not result.wasSuccessful() or result.skipped:
    raise SystemExit('DSGA lifecycle suite failed or skipped required tests')
PY
