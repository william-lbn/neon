set -eu
mkdir /out
for metric in cpu.max memory.max;do cat /sys/fs/cgroup/$metric >/out/$metric;done
test "$(cat /sys/fs/cgroup/memory.max)" = 3221225472
test "$(cat /sys/fs/cgroup/cpu.max)" = "200000 100000"
sha256sum -c .source.sha256 >/out/input-check.txt
make -j2 BUILD_TYPE=release postgres-headers-install-v18 >/out/headers.log 2>&1
cp -a build/v18/configure.log /out/configure.log
cp -a pg_install/v18/include /out/include
cc -Ipg_install/v18/include/postgresql/server -Ipg_install/v18/include/postgresql -o /out/new-layout layout.c
cc -Ireference18/include/postgresql/server -Ireference18/include/postgresql -o /out/reference-layout layout.c
/out/new-layout >/out/new-layout.bin
/out/reference-layout >/out/reference-layout.bin
cmp /out/new-layout.bin /out/reference-layout.bin
for name in catalog/pg_control.h access/xlog.h access/xlog_internal.h access/xlogrecord.h pg_config_manual.h;do sha256sum pg_install/v18/include/postgresql/server/$name reference18/include/postgresql/server/$name >>/out/key-headers.sha256;cmp pg_install/v18/include/postgresql/server/$name reference18/include/postgresql/server/$name;done
grep -n "POSTGRES_VERSIONS\|neon: postgres-headers-install" Makefile >/out/default-neon-dependencies.txt
