#!/bin/bash
set -euo pipefail
source /data/neon-build/config/env.sh
exec python3 -u /data/neon-build/validation/compute-no-rpath-closure-20261031T202356Z-0d36d0/worker.py >/data/neon-build/validation/compute-no-rpath-closure-20261031T202356Z-0d36d0/unit.log 2>&1
