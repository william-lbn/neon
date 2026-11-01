FROM 127.0.0.1:5001/neon/compute-v17-stage@sha256:ece1e02faadce8c6c0c949e8314226ccfa958cef74902b89f790d89a0e28c020 AS test
USER root
RUN mkdir /component /out && chown 1000:1000 /component /out
WORKDIR /component
COPY --chown=1000:1000 . .
USER nonroot
ENV RUSTUP_TOOLCHAIN=1.88.0-x86_64-unknown-linux-gnu
ENV PATH=/home/nonroot/.rustup/toolchains/1.88.0-x86_64-unknown-linux-gnu/bin:${PATH}
ENV CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/home/nonroot/target/pg-stderr-component CARGO_NET_OFFLINE=true
ARG NONCE
RUN --network=none --mount=type=cache,id=neon-cargo-registry-amd64,target=/home/nonroot/.cargo/registry,uid=1000,gid=1000,sharing=locked --mount=type=cache,id=neon-cargo-git-amd64,target=/home/nonroot/.cargo/git,uid=1000,gid=1000,sharing=locked --mount=type=cache,id=neon-core-target-amd64,target=/home/nonroot/target,uid=1000,gid=1000,sharing=locked set -eu; test "$(cat /sys/fs/cgroup/memory.max)" = 3221225472; cat /sys/fs/cgroup/memory.max >/out/memory.max; cat /sys/fs/cgroup/cpu.max >/out/cpu.max; sha256sum -c source.sha256 >/out/source-check.txt; cp source.sha256 /out/actual-source.sha256; rustc --version >/out/rustc-version.txt; set +e; cargo build --locked --offline --bin pg-stderr-suite > /out/suite-build.log 2>&1; result=$?; echo "$result" >/out/suite-build.exitcode; if [ "$result" = 0 ]; then cp /home/nonroot/target/pg-stderr-component/debug/pg-stderr-suite /out/pg-stderr-suite; fi; cp /sys/fs/cgroup/cpu.stat /out/cpu.stat; cp /sys/fs/cgroup/memory.peak /out/memory.peak; cp /sys/fs/cgroup/io.stat /out/io.stat; exit 0
FROM scratch
COPY --from=test /out/ /
