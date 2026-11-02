FROM 127.0.0.1:5001/neon/pg17-dsm-standalone-fix@sha256:b164d61a939502fa3ba983c0359f7832b9345ccefeb15c00326eaceab61a3a40 AS actualpg
FROM 127.0.0.1:5001/neon/compute-v17-stage@sha256:ece1e02faadce8c6c0c949e8314226ccfa958cef74902b89f790d89a0e28c020 AS check
USER nonroot
WORKDIR /home/nonroot/neon-extension-source
COPY --chown=1000:1000 --from=actualpg /usr/local/pgsql/ /usr/local/pgsql/
USER root
RUN mkdir /proof && chown 1000:1000 /proof
USER nonroot
COPY --chown=1000:1000 . .
RUN --network=none set -eu; test "$(cat /sys/fs/cgroup/memory.max)" = 3221225472; test "$(cat /sys/fs/cgroup/cpu.max)" = "200000 100000"; sha256sum -c .compat-syntax-inputs.sha256 >/proof/actual-input-verification.txt; echo '649f4535d07e682fe5015fd8df8be23f626e73bd772da21b16e1c032721c53c6  /usr/local/pgsql/bin/postgres' | sha256sum -c - >/proof/actual-PG17-ELF.txt; /usr/local/pgsql/bin/pg_config >/proof/pg-config.txt; failed=0; rc=0; gcc-12 $(/usr/local/pgsql/bin/pg_config --cflags) -D_GNU_SOURCE -I/usr/local/pgsql/include/server -I/usr/local/pgsql/include -I/usr/local/pgsql/include/internal -Ipgxn/neon -fsyntax-only pgxn/neon/libpagestore.c > /proof/libpagestore.log 2>&1 || rc=$?; echo "$rc" > /proof/libpagestore.exitcode; if test "$rc" != 0; then failed=1; cat /proof/libpagestore.log; fi; cat /sys/fs/cgroup/cpu.max >/proof/cpu.max; cat /sys/fs/cgroup/memory.max >/proof/memory.max; cat /sys/fs/cgroup/cpu.stat >/proof/cpu.stat; cat /sys/fs/cgroup/memory.peak >/proof/memory.peak; cat /sys/fs/cgroup/io.stat >/proof/io.stat; echo PASS_SEVEN_PG17_TRANSLATION_UNITS >/proof/verdict.txt
FROM scratch
COPY --from=check /proof/ /
