#!/usr/bin/env bash
set -Eeuo pipefail
REPORT=/data/neon-build/validation/pg17-native-root-gate-20261030T230349Z
source /data/neon-build/config/env.sh
set +e
docker buildx build --builder neon-data --network none --progress plain --metadata-file "$REPORT/build-v2-metadata.json" --output type=image,name=127.0.0.1:5001/neon/pg17-native-root-wire:fd2bdeb-gate-v2-TEST_ONLY,push=true,registry.insecure=true -f "$REPORT/Dockerfile-v2" /data/neon-build/dev/pg17-native-root-gate-20261030T230349Z/snapshot 2>&1 | tee "$REPORT/build-v2.log"
rc=${PIPESTATUS[0]}
set -e
printf '%s\n' "$rc" > "$REPORT/build-v2-exit-code.txt"
test "$rc" = 0
docker pull 127.0.0.1:5001/neon/pg17-native-root-wire:fd2bdeb-gate-v2-TEST_ONLY > "$REPORT/pull-v2.log" 2>&1
