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
gcc-12 -Wall -Wextra -Werror -Wno-unused-parameter -Wno-declaration-after-statement -shared -fPIC -I/opt/pg18/include/postgresql/server -I/opt/pg18/include /input/pg18_neon_aio_probe.c -o /evidence/pg18_neon_aio_probe.so -ldl >/evidence/probe-compile.log 2>&1
export NEON_PG18_AIO_PROBE=/evidence/pg18_neon_aio_probe.so
sha256sum /evidence/pg18_neon_aio_probe.so >/evidence/probe-ELF.sha256
python3 -B -u /input/test_dsg_pg18_remote_aio.py RemoteAio.test_PG18_S_NEON_SLRU3_001_real_rpc_and_narrowing_rejections
