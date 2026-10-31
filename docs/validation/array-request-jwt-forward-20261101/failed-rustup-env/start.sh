#!/bin/bash
set -euo pipefail
source /data/neon-build/config/env.sh
exec python3 /data/neon-build/validation/rust-array-authority-20261031T184420Z-6ccc90/worker.py /data/neon-build/validation/rust-array-authority-20261031T184420Z-6ccc90 20261031T184420Z-6ccc90
