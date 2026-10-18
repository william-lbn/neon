#!/usr/bin/env bash
# 在已有的本地 PG 构建上运行完整检查；Neon 自定义 WAL 必须加载配套 rmgr。
# 本脚本不下载、不切换版本，也不安装到系统目录。PG_CONFIG 必须来自调用方
# 已安装的开发前缀，测试配置仅通过 TEMP_CONFIG 注入，不改生产配置。
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

make -C "$task_root/pgxn/neon_rmgr" PG_CONFIG="$pg_config" all
test_directory=$(mktemp -d "${TMPDIR:-/tmp}/neon-pg-check.XXXXXX")
trap 'rm -f "$test_directory/postgresql.conf"; rmdir "$test_directory"' EXIT
python3 - "$task_root/pgxn/neon_rmgr/neon_rmgr" "$test_directory/postgresql.conf" <<'PY'
from pathlib import Path
import sys

# 配置文件采用 PostgreSQL 字符串转义；不能使用 shell 的引号规则。
library = sys.argv[1].replace("'", "''")
Path(sys.argv[2]).write_text(f"shared_preload_libraries = '{library}'\n")
PY

make -C "$pg_build" -j"${BUILD_JOBS:-4}" \
    TEMP_CONFIG="$test_directory/postgresql.conf" check-world
