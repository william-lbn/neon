#!/bin/bash
set -euo pipefail
source /data/neon-build/config/env.sh
exec python3 /data/neon-build/validation/rust-array-authority-20261031T190342Z-681d7b/worker.py /data/neon-build/validation/rust-array-authority-20261031T190342Z-681d7b 20261031T190342Z-681d7b
