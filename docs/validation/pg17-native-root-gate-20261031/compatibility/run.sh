#!/usr/bin/env bash
set -Eeuo pipefail
REPORT=/data/neon-build/validation/pg17-native-root-gate-20261030T230349Z/compatibility
set +e
timeout 240 docker run --rm --init --network none --cpus=2 --memory=2g --pids-limit=256 --cap-drop ALL -e PYTHONDONTWRITEBYTECODE=1 -e NEON_BIRTH_EVIDENCE=/evidence --mount type=bind,src="$REPORT",dst=/evidence 127.0.0.1:5001/neon/pg17-native-root-wire@sha256:af97e387041b349d0138b39b9a7c90c9d612db3ef07906fe1271db7162ee414d python3 /evidence/runner.py 2>&1 | tee "$REPORT/run.log"
rc=${PIPESTATUS[0]}
set -e
printf '%s\n' "$rc" > "$REPORT/exit-code.txt"
exit "$rc"
