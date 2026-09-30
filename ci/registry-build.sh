#!/usr/bin/env bash
# Retry registry transport failures while preserving the compiler's exit status.
set -uo pipefail
log=$(mktemp)
trap 'rm -f "$log"' EXIT
for attempt in 1 2 3 4; do
  "$@" 2>&1 | tee "$log"
  status=${PIPESTATUS[0]}
  if [ "$status" -eq 0 ]; then exit 0; fi
  if [ "$attempt" -eq 4 ] || ! grep -Eiq '429 Too Many Requests|toomanyrequests|502 Bad Gateway|503 Service Unavailable|504 Gateway Timeout|unexpected EOF|connection reset|i/o timeout|TLS handshake timeout' "$log"; then
    exit "$status"
  fi
  delay=$((15 * 2 ** (attempt - 1)))
  echo "Registry transport failed; retry $attempt/3 in ${delay}s"
  sleep "$delay"
done
