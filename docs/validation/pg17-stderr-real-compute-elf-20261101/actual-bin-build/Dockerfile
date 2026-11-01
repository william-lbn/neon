FROM 127.0.0.1:5001/neon/compute-v17-stage@sha256:ece1e02faadce8c6c0c949e8314226ccfa958cef74902b89f790d89a0e28c020 AS build
USER root
RUN mkdir /stderr-work /out && chown 1000:1000 /stderr-work /out
WORKDIR /stderr-work
COPY --chown=1000:1000 . .
USER nonroot
ENV RUSTUP_TOOLCHAIN=1.88.0-x86_64-unknown-linux-gnu
ENV PATH=/home/nonroot/.rustup/toolchains/1.88.0-x86_64-unknown-linux-gnu/bin:${PATH}
ENV CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/home/nonroot/target CARGO_NET_OFFLINE=true
ARG NONCE
RUN --network=none --mount=type=cache,id=neon-cargo-registry-amd64,target=/home/nonroot/.cargo/registry,uid=1000,gid=1000,sharing=locked --mount=type=cache,id=neon-cargo-git-amd64,target=/home/nonroot/.cargo/git,uid=1000,gid=1000,sharing=locked --mount=type=cache,id=neon-core-target-amd64,target=/home/nonroot/target,uid=1000,gid=1000,sharing=locked set -eu; test "$(cat /sys/fs/cgroup/memory.max)" = 8589934592; sha256sum -c stderr-source.sha256 >/out/source-check.txt; cp stderr-source.sha256 /out/compiled-source.sha256; rustc --version >/out/rustc-version.txt; set +e; cargo build --locked --offline -p compute_tools --bin compute_ctl >/out/bin-build.log 2>&1; rc=$?; echo "$rc" >/out/bin-build.exitcode; if [ "$rc" = 0 ]; then sha256sum /home/nonroot/target/debug/compute_ctl >/out/unstripped-ELF.sha256; stat -c %s /home/nonroot/target/debug/compute_ctl >/out/unstripped-ELF.bytes; strip --strip-debug -o /out/compute_ctl.real /home/nonroot/target/debug/compute_ctl; echo "$?" >/out/strip.exitcode; sha256sum /out/compute_ctl.real >/out/ELF.sha256; readelf -h -l -d /out/compute_ctl.real >/out/ELF.readelf; fi; cp /sys/fs/cgroup/cpu.stat /out/cpu.stat; cp /sys/fs/cgroup/memory.peak /out/memory.peak; cp /sys/fs/cgroup/io.stat /out/io.stat; cp /sys/fs/cgroup/memory.max /out/memory.max; cp /sys/fs/cgroup/cpu.max /out/cpu.max; exit 0
FROM scratch
COPY --from=build /out/ /
