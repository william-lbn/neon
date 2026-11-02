set -Eeuo pipefail
trap 'rc=$?; for metric in cpu.stat memory.peak io.stat memory.events; do cat /sys/fs/cgroup/$metric > /evidence/$metric; done; exit "$rc"' EXIT
test "$(cat /sys/fs/cgroup/memory.max)" = 3221225472
test "$(cat /sys/fs/cgroup/cpu.max)" = "200000 100000"
cat /sys/fs/cgroup/cpu.max >/evidence/cpu.max
cat /sys/fs/cgroup/memory.max >/evidence/memory.max
sha256sum /opt/pg18/bin/postgres /opt/pg18/lib/postgresql/neon.so /opt/pg18/lib/postgresql/neon_rmgr.so >/evidence/actual-artifacts.sha256
/opt/pg18/bin/pg_config >/evidence/pg-config.txt
/opt/pg18/bin/postgres --version >/evidence/postgres-version.txt
ldd /opt/pg18/lib/postgresql/neon.so >/evidence/neon-ldd.txt
ldd /opt/pg18/lib/postgresql/neon_rmgr.so >/evidence/neon-rmgr-ldd.txt
export NEON_TEST_PG_BIN=/opt/pg18/bin NEON_PG18_EVIDENCE=/evidence LD_LIBRARY_PATH=/opt/pg18/lib
python3 -B -u /input/test_dsg_pg18_formal_extension.py
