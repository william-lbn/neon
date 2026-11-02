FROM 127.0.0.1:5001/neon/neon-build-tools@sha256:234cfd4ca295d55f27ceebd101229d5f0ebf2b8e7503f44e5cf5fb1d5d119a5b AS build
USER root
WORKDIR /home/nonroot
COPY . .
RUN --network=none set +e; bash /home/nonroot/headers.sh >/tmp/headers-launcher.log 2>&1; rc=$?; mkdir -p /out; cp /tmp/headers-launcher.log /out/launcher.log; echo $rc >/out/headers.exitcode; for metric in cpu.stat memory.peak memory.events io.stat; do cat /sys/fs/cgroup/$metric >/out/$metric;done
FROM scratch
COPY --from=build /out/ /
