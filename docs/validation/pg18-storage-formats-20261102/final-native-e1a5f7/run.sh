set -Eeuo pipefail
trap 'rc=$?; for metric in cpu.stat memory.peak io.stat memory.events cpu.max memory.max;do cat /sys/fs/cgroup/$metric >/evidence/$metric;done;exit "$rc"' EXIT
test "$(cat /sys/fs/cgroup/memory.max)" = 3221225472
test "$(cat /sys/fs/cgroup/cpu.max)" = "200000 100000"
sha256sum /opt/pg18/bin/postgres /input/pg18_native_formats >/evidence/actual-artifacts.sha256
ldd /input/pg18_native_formats >/evidence/reader-ldd.txt
export NEON_TEST_PG_BIN=/opt/pg18/bin NEON_PG18_EVIDENCE=/evidence NEON_PG18_FORMAT_READER=/input/pg18_native_formats LD_LIBRARY_PATH=/opt/pg18/lib
python3 -B -u /input/test_dsg_pg18_formats.py
