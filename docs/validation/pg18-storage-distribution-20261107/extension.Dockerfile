FROM 127.0.0.1:5001/neon/compute-v17-stage@sha256:ece1e02faadce8c6c0c949e8314226ccfa958cef74902b89f790d89a0e28c020 AS build
USER root
RUN --network=none set -eu; mv /usr/local/pgsql /opt/old-pg; install -d -o 1000 -g 1000 /usr/local/pgsql
COPY --from=pgstage --chown=1000:1000 /usr/local/pgsql/ /usr/local/pgsql/
USER nonroot
WORKDIR /home/nonroot/walredo
COPY --chown=1000:1000 . .
RUN --network=none set -eu; mkdir /home/nonroot/proof; echo '162e8f20c7a2e2b34f25b74288ee495e61fd70519d62829ab1899ca8501bd4ef  /usr/local/pgsql/bin/postgres' | sha256sum -c -; /usr/local/pgsql/bin/pg_config --version; make -C pgxn/neon_walredo -j3 PG_CONFIG=/usr/local/pgsql/bin/pg_config all install > /home/nonroot/proof/build.log 2>&1 || { tail -80 /home/nonroot/proof/build.log; exit 1; }; cp /usr/local/pgsql/lib/neon_walredo.so /home/nonroot/proof/; readelf -d /home/nonroot/proof/neon_walredo.so > /home/nonroot/proof/dynamic.txt; sha256sum /home/nonroot/proof/neon_walredo.so > /home/nonroot/proof/elf.sha256
FROM scratch
COPY --from=build /home/nonroot/proof/ /
