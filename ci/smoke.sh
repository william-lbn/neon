#!/usr/bin/env bash
# 用准确 digest 在隔离的 Compose 拓扑中验证 SQL、Neon 存储和 Compute 丢盘恢复。
# PG18 dsga-core 只验证实际交付扩展；不会通过删除测试冒称 full 扩展已适配。
set -euo pipefail
cd "$(dirname "$0")/.."
compose_dir="${RUNNER_TEMP:?}/neon-smoke-pg${PG_VERSION:?}"
mkdir -p "$compose_dir"
cp -a docker-compose/. "$compose_dir/"
profile="${COMPUTE_PROFILE:-full}"
case "$profile" in full|dsga-core) ;; *) echo 'Unknown compute profile' >&2; exit 2 ;; esac
export COMPOSE_FILE="$compose_dir/docker-compose.yml"
export COMPOSE_PROJECT_NAME="neon-ci-pg$PG_VERSION"
export REPOSITORY TAG PG_VERSION
neon_image="$REPOSITORY/neon:$TAG"
if [[ -n "${DISTRIBUTION_MANIFEST:-}" ]]; then
  # JSON 是 Compose 可读的 YAML 子集；不在 shell 中拼接镜像 JSON 或任意输入。
  python3 - "$DISTRIBUTION_MANIFEST" "$compose_dir" "$PG_VERSION" "$profile" <<'PY'
import json,sys
from pathlib import Path
manifest=json.loads(Path(sys.argv[1]).read_text())
directory=Path(sys.argv[2]); pg='v'+sys.argv[3]; profile=sys.argv[4]
if manifest['source_lock']['postgres_profiles'][pg] != profile:
    raise SystemExit('Smoke profile differs from distribution manifest')
if manifest['version'] != __import__('os').environ['TAG']:
    raise SystemExit('Smoke version differs from distribution manifest')
def image(name):
    record=manifest['images'][name]
    return record['image']+'@'+record['digest']
services={name:{'image':image('neon')} for name in ['pageserver','safekeeper1','safekeeper2','safekeeper3','storage_broker']}
compute=manifest['images']['compute-node-'+pg]
services['compute1']={'build':{'args':{'TAG':manifest['version']+'@'+compute['digest']}},
                      'environment':{'COMPUTE_PROFILE':profile}}
(directory/'ci-images.json').write_text(json.dumps({'services':services}))
(directory/'neon-image.txt').write_text(image('neon'))
if profile=='dsga-core':
    file=directory/'compute_wrapper/var/db/postgres/configs/config.json'
    config=json.loads(file.read_text())
    for setting in config['spec']['cluster']['settings']:
        if setting['name']=='shared_preload_libraries':
            setting['value']='neon,pg_stat_statements'
    file.write_text(json.dumps(config,indent=2)+'\n')
PY
  export COMPOSE_FILE="$COMPOSE_FILE:$compose_dir/ci-images.json"
  neon_image=$(cat "$compose_dir/neon-image.txt")
fi
# GitHub checkout owner differs from the container UID 1000.
sudo chown -R 1000:1000 "$compose_dir/pageserver_config"
cleanup() {
  mkdir -p logs
  docker compose logs --no-color > "logs/smoke-pg$PG_VERSION.log" 2>&1 || true
  docker compose down --volumes || true
}
trap cleanup EXIT
wait_compute() {
  for attempt in $(seq 1 90); do
    if docker compose exec -T compute1 psql -h localhost -p 55433 -U cloud_admin -d postgres -Atc 'SELECT 1' 2>/dev/null | grep -qx 1; then
      return 0
    fi
    sleep 2
  done
  docker compose logs --no-color
  return 1
}
sql() {
  docker compose exec -T compute1 psql -h localhost -p 55433 -U cloud_admin -d postgres -XAt -v ON_ERROR_STOP=1 "$@"
}
docker pull "$neon_image"
version=$(docker run --rm "$neon_image" pageserver --version)
echo "$version"
[[ "$version" != *git-env:local* && "$version" != *'"testing"'* ]]
for binary in proxy pg_sni_router storage_broker storage_controller endpoint_storage storage_scrubber vm-monitor; do
  docker run --rm --entrypoint "/usr/local/bin/$binary" "$neon_image" --help >/dev/null
done
docker compose build compute1
docker compose up -d minio minio_create_buckets storage_broker pageserver safekeeper1 safekeeper2 safekeeper3 compute1
wait_compute
[[ "$(sql -c "SELECT current_setting('server_version_num')::integer/10000")" == "$PG_VERSION" ]]
sql <<'SQL'
CREATE TABLE ci_durability (id integer PRIMARY KEY, payload text NOT NULL);
INSERT INTO ci_durability SELECT i, md5(i::text) FROM generate_series(1, 1000) i;
CHECKPOINT;
SQL
if [[ "$profile" == full ]]; then
  sql <<'SQL'
CREATE EXTENSION vector;
SELECT '[1,2,3]'::vector;
CREATE EXTENSION postgis;
SELECT ST_AsText(ST_Point(1,2));
SQL
else
  # 真实扩展加载和 GUC 可见性检查；候选镜像启动默认仍不授予动态/物理许可。
  [[ "$(sql -c "SELECT count(*) FROM pg_settings WHERE name IN ('enable_dynamic_shared_buffers','max_shared_buffers')")" == 2 ]]
  [[ "$(sql -c 'SHOW enable_dynamic_shared_buffers')" == off ]]
  sql -c 'SELECT count(*) FROM pg_stat_statements'
fi
before=$(sql -c "SELECT md5(string_agg(payload, ',' ORDER BY id)) FROM ci_durability")
# 删除 Compute 本地文件系统后，必须由 Neon 持久层重建相同数据。
docker compose stop compute1
docker compose rm -f compute1
docker compose up -d compute1
wait_compute
after=$(sql -c "SELECT md5(string_agg(payload, ',' ORDER BY id)) FROM ci_durability")
[[ "$before" = "$after" ]]
echo "PG$PG_VERSION ($profile): SQL, shipped extensions and recovery after compute replacement passed"
