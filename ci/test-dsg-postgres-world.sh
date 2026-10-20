#!/usr/bin/env bash
# 在锁定的真实PG17上顺序跑开/关全量原生回归与TAP。只能使用独占的开发build，
# check-world会重建其中的tmp_install；不可与另一份make/安装/专项并行执行。
# 只启动临时测试集群，不构建镜像/部署。平台/可选配置skip保留原生报告，
# 不能据总退出码把它们算作已执行，也不能代替零skip的DSGA/完整Neon专项。
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
    echo "DSGA native regression tests require --enable-injection-points" >&2
    exit 2
fi
prove_program=$(command -v "${PROVE:-prove}")
perl -MIPC::Run -e 1
cd "$task_root"
python3 ci/compute.py validate --source-ref index --require-pg-checkout v17
python3 - "$task_root" "$pg_build" "$pg_install" <<'PY'
import json
from pathlib import Path
import subprocess
import sys

root, build, installed = map(Path, sys.argv[1:])
expected = json.loads((root / 'vendor/revisions.json').read_text())['v17'][1]
# check-world使用build生成tmp_install，不能只验证另一份安装目录的二进制。
for executable in (build / 'src/backend/postgres', installed / 'bin/postgres'):
    version = subprocess.check_output([str(executable), '--version'], text=True)
    if f'({expected})' not in version or not version.startswith('postgres (PostgreSQL) 17.'):
        raise SystemExit(f'PG17 binary does not match source lock: {executable}: {version.strip()}')
PY

make -C "$task_root/pgxn/neon_rmgr" PG_CONFIG="$pg_config" clean
make -C "$task_root/pgxn/neon_rmgr" PG_CONFIG="$pg_config" install
reports="$pg_build/dsga-world"
mkdir -p "$reports"
python3 - "$reports" "$("$pg_config" --pkglibdir)/neon_rmgr" <<'PY'
from pathlib import Path
import sys

reports, library = sys.argv[1:]
library = library.replace('\\', '\\\\').replace("'", "''")
for mode in ('off', 'on'):
    (Path(reports) / f'{mode}.conf').write_text(
        f"shared_preload_libraries = '{library}'\n"
        f'enable_dynamic_shared_buffers = {mode}\n'
        "max_shared_buffers = '1GB'\n"
    )
PY

failed=0
for mode in off on; do
    logfile="$reports/$mode.log"
    echo "running native check-world and TAP: dynamic=$mode, log=$logfile"
    # -k继续收集其他模块；失败仍保留非零状态，不能只汇报最后一个通过的组。
    if make -C "$pg_build" -j"${BUILD_JOBS:-4}" -k enable_tap_tests=yes \
        PROVE="$prove_program" TEMP_CONFIG="$reports/$mode.conf" check-world >"$logfile" 2>&1; then
        if python3 - "$logfile" <<'PY'
from pathlib import Path
import re
import sys

log = Path(sys.argv[1]).read_text()
# make成功但TAP未启用/未进入关键目录也必须拒绝；不硬编码上游断言总数。
for marker in ('# +++ tap check in src/test/recovery +++',
               '# +++ tap check in src/bin/pg_upgrade +++'):
    if marker not in log:
        raise SystemExit(f'missing required TAP execution: {marker}')
for name in ('003_logical_slots', '061_neon_shutdown_restartpoint'):
    if not re.search(rf'^t/{name}\.pl\s+\.+\s+ok\s*$', log, re.M):
        raise SystemExit(f'required TAP file did not run successfully: {name}')
reports = re.findall(r'^Files=(\d+), Tests=(\d+),', log, re.M)
if not reports or 'Result: FAIL' in log:
    raise SystemExit('native TAP report missing or failed')
print(f'native checks passed; TAP reports={len(reports)}, '
      f'files reported={sum(int(a) for a, _ in reports)}, '
      f'assertions reported={sum(int(b) for _, b in reports)}')
print('platform/optional skips remain in the log; reported totals include skips')
PY
        then
            :
        else
            failed=1
        fi
    else
        tail -80 "$logfile" >&2
        failed=1
    fi
done
exit "$failed"
