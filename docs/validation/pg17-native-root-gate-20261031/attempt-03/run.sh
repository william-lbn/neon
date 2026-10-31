#!/usr/bin/env bash
set -Eeuo pipefail
REPORT=/data/neon-build/validation/pg17-native-root-gate-20261030T230349Z/attempt-03
set +e
timeout 180 docker run --rm --init --network none --cpus=2 --memory=2g --pids-limit=128 --user 0 --cap-drop ALL --cap-add SYS_ADMIN --cap-add SYS_PTRACE --cap-add SETUID --cap-add SETGID --cap-add KILL --cap-add CHOWN --cap-add DAC_OVERRIDE --security-opt seccomp=unconfined --security-opt apparmor=unconfined -e PYTHONDONTWRITEBYTECODE=1 -e NEON_DSGA_ROOT_COMPONENT_TEST=1 -e NEON_ROOT_WIRE_EVIDENCE=/evidence --mount type=bind,src="$REPORT",dst=/evidence 127.0.0.1:5001/neon/pg17-native-root-wire@sha256:af97e387041b349d0138b39b9a7c90c9d612db3ef07906fe1271db7162ee414d python3 /evidence/test_root_birth.py 2>&1 | tee "$REPORT/test-root.log"
rc=${PIPESTATUS[0]}
set -e
printf '%s\n' "$rc" > "$REPORT/test-root-exit-code.txt"
exit "$rc"
