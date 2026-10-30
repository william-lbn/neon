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
cd "$task_root"
python3 ci/compute.py validate --source-ref index --require-pg-checkout v17

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
# 逻辑复制正向用例必须实际解码COPY流，不能只证明物理通道被拒绝。
# test_decoding只供独占临时集群使用，不加入产品默认扩展或生产部署。
make -C "$pg_build/contrib/test_decoding" -j"${BUILD_JOBS:-4}" install
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
suite.addTests(unittest.defaultTestLoader.discover('.', pattern='test_dsg_control.py'))
admission = unittest.defaultTestLoader.discover('.', pattern='test_dsg_admission.py')
# 准入新增范围必须真实打包/执行，不能由旧控制套件绿色掩盖缺失文件。
if admission.countTestCases() < 3:
    raise SystemExit('DSGA fenced admission suite is missing required cases')
suite.addTests(admission)
closure = unittest.defaultTestLoader.discover('.', pattern='test_dsg_admission_closure.py')
# 关闭必须能封住未到达的原开放；此项使用真实内核，不由模型回执替代。
if closure.countTestCases() < 1:
    raise SystemExit('DSGA original admission closure suite is missing required cases')
suite.addTests(closure)
frames = unittest.defaultTestLoader.discover('.', pattern='test_dsg_admission_frames.py')
# 大帧在完整读取前的内存边界不能由旧准入回执测试替代；漏打包须明确失败。
if frames.countTestCases() < 5:
    raise SystemExit('DSGA bounded admission frame suite is missing required cases')
suite.addTests(frames)
work = unittest.defaultTestLoader.discover('.', pattern='test_dsg_work.py')
# 真实在途与事务、退出清理不能由旧门禁/消息内存测试代替。
if work.countTestCases() < 8:
    raise SystemExit('DSGA native work registry suite is missing required cases')
suite.addTests(work)
prestart = unittest.defaultTestLoader.discover('.', pattern='test_dsg_prestart.py')
# PGPROC前窗口由Postmaster实际跟踪，不能只跑已建立PGPROC的旧8项。
if prestart.countTestCases() < 6:
    raise SystemExit('DSGA Postmaster prestart suite is missing required cases')
suite.addTests(prestart)
connections = unittest.defaultTestLoader.discover('.', pattern='test_dsg_connection_gate.py')
if connections.countTestCases() < 6:
    raise SystemExit('DSGA continuous connection gate suite is missing required cases')
suite.addTests(connections)
finishes = unittest.defaultTestLoader.discover('.', pattern='test_dsg_transaction_finish.py')
if finishes.countTestCases() < 9:
    raise SystemExit('DSGA retained transaction finish suite is missing required cases')
suite.addTests(finishes)
prepared = unittest.defaultTestLoader.discover('.', pattern='test_dsg_prepared.py')
if prepared.countTestCases() < 6:
    raise SystemExit('DSGA native prepared registry suite is missing required cases')
suite.addTests(prepared)
dsm = unittest.defaultTestLoader.discover('.', pattern='test_dsg_dsm.py')
# 必须实际打包并执行分配前额度/真实pin/失败保留/主段去重，不由旧prepared代替。
if dsm.countTestCases() < 8:
    raise SystemExit('DSGA native DSM reservation suite is missing required cases')
suite.addTests(dsm)
partition_lock = unittest.defaultTestLoader.discover('.', pattern='test_dsg_upstream_partition_index.py')
if partition_lock.countTestCases() != 1:
    raise SystemExit('PG17 upstream partition index lock regression is missing')
suite.addTests(partition_lock)
replication = unittest.defaultTestLoader.discover('.', pattern='test_dsg_replication.py')
# 新测试文件遗漏打包时discover会返回空suite，不能让旧80项绿色覆盖本批缺测。
if replication.countTestCases() < 6:
    raise SystemExit('DSGA replication boundary suite is missing required cases')
suite.addTests(replication)
early_birth = unittest.defaultTestLoader.discover('.', pattern='test_dsg_early_arrays.py')
# 此套件真实覆盖NNP=0拒绝、早期R、logger恢复和扩展ACL，漏源或漏装不能绿色。
if early_birth.countTestCases() != 6:
    raise SystemExit('PG17 early birth suite must contain exactly six required native cases')
suite.addTests(early_birth)
if suite.countTestCases() == 0:
    raise SystemExit('DSGA lifecycle suite contains zero tests')
result = unittest.TextTestRunner(verbosity=2).run(suite)
if not result.wasSuccessful() or result.skipped:
    raise SystemExit('DSGA lifecycle suite failed or skipped required tests')
PY
