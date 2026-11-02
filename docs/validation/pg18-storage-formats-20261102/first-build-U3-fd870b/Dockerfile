FROM 127.0.0.1:5001/neon/neon@sha256:17e4322dfbe985234940955984b4e7ebcd72071cebd130a1993fab1121f3318e AS old_pg_headers
FROM 127.0.0.1:5001/neon/compute-v17-stage@sha256:ece1e02faadce8c6c0c949e8314226ccfa958cef74902b89f790d89a0e28c020 AS build
USER root
COPY --from=old_pg_headers /usr/local/v14/include/ /home/nonroot/pg_install/v14/include/
COPY --from=old_pg_headers /usr/local/v15/include/ /home/nonroot/pg_install/v15/include/
COPY --from=old_pg_headers /usr/local/v16/include/ /home/nonroot/pg_install/v16/include/
COPY --from=old_pg_headers /usr/local/v17/include/ /home/nonroot/pg_install/v17/include/
RUN mkdir /stderr-work /out && chown 1000:1000 /stderr-work /out
COPY --chown=1000:1000 pg18-install/ /opt/pg18/
RUN mkdir -p /home/nonroot/pg_install && ln -s /opt/pg18 /home/nonroot/pg_install/v18
WORKDIR /stderr-work
COPY --chown=1000:1000 . .
USER nonroot
ENV RUSTUP_TOOLCHAIN=1.88.0-x86_64-unknown-linux-gnu PATH=/home/nonroot/.rustup/toolchains/1.88.0-x86_64-unknown-linux-gnu/bin:${PATH} CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/home/nonroot/target CARGO_NET_OFFLINE=true POSTGRES_INSTALL_DIR=/home/nonroot/pg_install
ARG NONCE
RUN --network=none --mount=type=cache,id=neon-cargo-registry-amd64,target=/home/nonroot/.cargo/registry,uid=1000,gid=1000,sharing=locked --mount=type=cache,id=neon-cargo-git-amd64,target=/home/nonroot/.cargo/git,uid=1000,gid=1000,sharing=locked --mount=type=cache,id=neon-core-target-amd64,target=/home/nonroot/target,uid=1000,gid=1000,sharing=locked set +e; bash -x /stderr-work/build-formats.sh >/out/launcher.log 2>&1; rc=$?; echo "$rc" >/out/launcher.exitcode; exit 0
FROM scratch
COPY --from=build /out/ /
