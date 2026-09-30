#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" = bullseye ]]; then
  # Debian bug #1147093: the security index remains live but its .deb files
  # were removed during the September 2026 archive transition. Preserve the
  # final LTS security updates instead of dropping the security repository.
  # APT still verifies Debian signatures and package hashes. Only the expiry
  # of this intentionally historical Release file is ignored.
  rm -f /etc/apt/sources.list.d/debian.sources
  cat > /etc/apt/sources.list <<'SOURCES'
deb http://archive.debian.org/debian bullseye main
deb http://archive.debian.org/debian bullseye-updates main
deb [check-valid-until=no] http://snapshot.debian.org/archive/debian-security/20260831T235959Z/ bullseye-security main
SOURCES
fi
