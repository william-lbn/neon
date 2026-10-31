#!/bin/bash
set -euo pipefail
source /data/neon-build/config/env.sh
exec python3 /data/neon-build/validation/rust-array-authority-20261031T191154Z-533e39/worker.py /data/neon-build/validation/rust-array-authority-20261031T191154Z-533e39 20261031T191154Z-533e39
