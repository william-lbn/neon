#!/usr/bin/env bash
# 在实际发行镜像内加载指定 PG 大版本的 WAL 回放进程。
# 必须在 Pageserver 镜像发布前执行，覆盖动态库缺失与 ABI 装载失败。
set -euo pipefail
if [[ $# != 2 || ! "$1" =~ @sha256:[0-9a-f]{64}$ || ! "$2" =~ ^[1-9][0-9]$ ]]; then
  echo "usage: verify-storage-distribution.sh IMAGE@sha256:DIGEST PG_MAJOR" >&2
  exit 2
fi
image="$1"
pg_major="$2"
timeout 30s docker run --rm --pull=never --network=none --read-only \
  --cap-drop=ALL --security-opt=no-new-privileges \
  --cpus=1 --memory=512m --memory-swap=512m \
  -e "LD_LIBRARY_PATH=/usr/local/v${pg_major}/lib" \
  --entrypoint "/usr/local/v${pg_major}/bin/postgres" \
  "$image" --wal-redo < /dev/null
