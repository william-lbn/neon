#!/bin/bash
set -euo pipefail
source /data/neon-build/config/env.sh
export PYTHONDONTWRITEBYTECODE=1
exec python3 /data/neon-build/validation/compute-no-rpath-pg-20261031T193328Z-45711b/driver/build-runtime.py
