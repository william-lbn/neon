set -eu
test "$(cat /sys/fs/cgroup/memory.max)" = 8589934592
test "$(cat /sys/fs/cgroup/cpu.max)" = "600000 100000"
sha256sum -c formats-source.sha256 >/out/input-check.txt
for major in 14 15 16 17 18; do
 test -f /home/nonroot/pg_install/v$major/include/postgresql/server/pg_config.h
 grep '^#define PG_VERSION_NUM' /home/nonroot/pg_install/v$major/include/postgresql/server/pg_config.h >>/out/actual-header-versions.txt
 find /home/nonroot/pg_install/v$major/include -type f -print0 | sort -z | xargs -0 sha256sum >>/out/actual-headers.sha256
done
rustc --version >/out/rustc-version.txt
set +e
cargo build --locked --offline -p postgres_ffi --example pg18_native_formats >/out/ffi-example-build.log 2>&1
ffi=$?;echo "$ffi" >/out/ffi-example-build.exitcode
cargo check --locked --offline -p wal_decoder -p pageserver -p safekeeper -p compute_tools >/out/affected-consumers.log 2>&1
echo "$?" >/out/affected-consumers.exitcode
cargo test --locked --offline -p pageserver --lib walingest::tests::pg18_checkpoint_wal_level_records -- --exact --nocapture >/out/checkpoint-wallevel-test.log 2>&1
echo "$?" >/out/checkpoint-wallevel-test.exitcode
if test "$ffi" = 0; then strip --strip-debug -o /out/pg18_native_formats /home/nonroot/target/debug/examples/pg18_native_formats;sha256sum /out/pg18_native_formats >/out/reader-ELF.sha256;fi
for metric in cpu.stat memory.peak io.stat memory.events cpu.max memory.max;do cat /sys/fs/cgroup/$metric >/out/$metric;done
exit 0
