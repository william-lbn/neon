FROM 127.0.0.1:5001/neon/compute-v17-stage@sha256:ece1e02faadce8c6c0c949e8314226ccfa958cef74902b89f790d89a0e28c020 AS test
USER root
RUN mkdir -p /workspace /out && chown -R 1000:1000 /workspace /out
WORKDIR /workspace
COPY --chown=1000:1000 source/ ./
COPY test-worker.py /test-worker.py
COPY test-commands.json /test-commands.json
USER nonroot
ENV CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/home/nonroot/target CARGO_NET_OFFLINE=true GIT_VERSION=f89c75e5b6e83eafb1c34352a67fa60ca5940bc3 BUILD_TAG=TEST_ONLY_ARRAY_AUTHORITY
ARG NONCE
RUN --network=none --mount=type=cache,id=neon-cargo-registry-amd64,target=/home/nonroot/.cargo/registry,uid=1000,gid=1000,sharing=locked --mount=type=cache,id=neon-cargo-git-amd64,target=/home/nonroot/.cargo/git,uid=1000,gid=1000,sharing=locked --mount=type=cache,id=neon-core-target-amd64,target=/home/nonroot/target,uid=1000,gid=1000,sharing=locked set -eu; test "$(cat /sys/fs/cgroup/memory.max)" = 8589934592; sha256sum -c cohort.sha256 > /out/source-check.txt; echo 0 > /out/source.rc; cp cohort.sha256 /out/actual-source.sha256; printf '%s\n' "$NONCE" > /out/nonce.txt; cargo --version > /out/cargo-version.txt; rustc --version > /out/rustc-version.txt; set +e; python3 /test-worker.py > /out/driver.log 2>&1; echo $? > /out/test.rc
FROM scratch
COPY --from=test /out/ /
