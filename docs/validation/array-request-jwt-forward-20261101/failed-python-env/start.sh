#!/bin/bash
set -euo pipefail
source /data/neon-build/config/env.sh
exec python3 /data/neon-build/validation/rust-array-authority-20261031T185059Z-ec6100/worker.py /data/neon-build/validation/rust-array-authority-20261031T185059Z-ec6100 20261031T185059Z-ec6100
