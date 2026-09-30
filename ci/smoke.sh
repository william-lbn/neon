#!/usr/bin/env bash
# Exercise the published images with the upstream Compose topology.
set -euo pipefail
cd "$(dirname "$0")/.."
compose_dir="${RUNNER_TEMP:?}/neon-smoke-pg${PG_VERSION:?}"
mkdir -p "$compose_dir"
cp -a docker-compose/. "$compose_dir/"
# GitHub's checkout owner differs from the upstream container's UID 1000.
sudo chown -R 1000:1000 "$compose_dir/pageserver_config"
export COMPOSE_FILE="$compose_dir/docker-compose.yml"
export COMPOSE_PROJECT_NAME="neon-ci-pg$PG_VERSION"
export REPOSITORY TAG PG_VERSION

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

docker pull "$REPOSITORY/neon:$TAG"
version=$(docker run --rm "$REPOSITORY/neon:$TAG" pageserver --version)
echo "$version"
[[ "$version" != *git-env:local* && "$version" != *'"testing"'* ]]
for binary in proxy pg_sni_router storage_broker storage_controller endpoint_storage storage_scrubber vm-monitor; do
  docker run --rm --entrypoint "/usr/local/bin/$binary" "$REPOSITORY/neon:$TAG" --help >/dev/null
done
docker compose build compute1
docker compose up -d minio minio_create_buckets storage_broker pageserver safekeeper1 safekeeper2 safekeeper3 compute1
wait_compute
docker compose exec -T compute1 psql -h localhost -p 55433 -U cloud_admin -d postgres -v ON_ERROR_STOP=1 <<'SQL'
CREATE TABLE ci_durability (id integer PRIMARY KEY, payload text NOT NULL);
INSERT INTO ci_durability SELECT i, md5(i::text) FROM generate_series(1, 1000) i;
CREATE EXTENSION vector;
SELECT '[1,2,3]'::vector;
CREATE EXTENSION postgis;
SELECT ST_AsText(ST_Point(1,2));
CHECKPOINT;
SQL
before=$(docker compose exec -T compute1 psql -h localhost -p 55433 -U cloud_admin -d postgres -Atc "SELECT md5(string_agg(payload, ',' ORDER BY id)) FROM ci_durability")
# Removing the compute's local filesystem forces a fresh basebackup from Neon storage.
docker compose stop compute1
docker compose rm -f compute1
docker compose up -d compute1
wait_compute
after=$(docker compose exec -T compute1 psql -h localhost -p 55433 -U cloud_admin -d postgres -Atc "SELECT md5(string_agg(payload, ',' ORDER BY id)) FROM ci_durability")
[[ "$before" = "$after" ]]
echo "PG$PG_VERSION: queries, vector, PostGIS and recovery after compute replacement passed"
