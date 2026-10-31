#!/bin/bash
set -euo pipefail
source /data/neon-build/config/env.sh
exec python3 /data/neon-build/validation/rust-array-authority-20261031T185200Z-59ddba/worker.py /data/neon-build/validation/rust-array-authority-20261031T185200Z-59ddba 20261031T185200Z-59ddba
