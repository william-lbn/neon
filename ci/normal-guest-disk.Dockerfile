# 将已锁定的普通系统根与原始包输入组装成 normal Guest 磁盘。
# 构建阶段只使用两个映像摘要和当前构建上下文，全程不安装或下载包。
ARG GUEST_BASE_IMAGE
ARG DISK_TOOLS_IMAGE
FROM ${GUEST_BASE_IMAGE} AS guest-base
FROM ${DISK_TOOLS_IMAGE} AS disk-build

ARG GUEST_BASE_IMAGE
ARG DISK_TOOLS_IMAGE
ARG BUILD_PLAN_SHA256
ARG PACKAGE_LOCK_SHA256
ARG VM_BUILDER_SHA256
ARG POSTGRES_MAJOR
ARG POSTGRES_SHA256

USER root
COPY --from=guest-base / /rootdisk/
COPY --chmod=0555 vm-builder /input/vm-builder
COPY build-plan.json package-lock.json /input/
COPY ordinary-sources/ /input/ordinary-sources/

RUN --network=none set -eu; \
    for ref in "$GUEST_BASE_IMAGE" "$DISK_TOOLS_IMAGE"; do \
        printf '%s\n' "$ref" | grep -Eq '^[^[:space:]@]+@sha256:[a-f0-9]{64}$'; \
    done; \
    case "$POSTGRES_MAJOR" in 17|18) ;; *) exit 1 ;; esac; \
    printf '%s  %s\n' \
        "$BUILD_PLAN_SHA256" /input/build-plan.json \
        "$PACKAGE_LOCK_SHA256" /input/package-lock.json \
        "$VM_BUILDER_SHA256" /input/vm-builder \
        "$POSTGRES_SHA256" /rootdisk/usr/local/bin/postgres | sha256sum -c -; \
    test "! -e /rootdisk/var/db/postgres/compute/PG_VERSION"; \
    mkdir /out; \
    /input/vm-builder --system-mode=normal-split-v3 --normal-ordinary-controls-version=2 \
        --normal-ordinary-package-lock=/input/package-lock.json \
        --normal-ordinary-package-lock-sha256="$PACKAGE_LOCK_SHA256" \
        --normal-ordinary-input-root=/input/ordinary-sources > /out/ordinary-input-readback.json; \
    /input/vm-builder --target-arch=linux/amd64 --system-mode=normal-split-v3 \
        --normal-manifest-root=/rootdisk --normal-build-plan=/input/build-plan.json \
        --normal-ordinary-controls-version=2 --normal-ordinary-package-lock=/input/package-lock.json \
        --normal-native-birth-version=1 --normal-native-pg-major="$POSTGRES_MAJOR" \
        --normal-native-postgres-sha256="$POSTGRES_SHA256" \
        --normal-base-image-sha="${GUEST_BASE_IMAGE##*@sha256:}"; \
    truncate -s 4G /disk.raw; \
    mkfs.ext4 -q -F -L vmroot -d /rootdisk /disk.raw; \
    qemu-img convert -f raw -O qcow2 -o cluster_size=2M,lazy_refcounts=on /disk.raw /out/disk.qcow2; \
    qemu-img compare -q -f raw -F qcow2 /disk.raw /out/disk.qcow2; \
    qemu-img check -f qcow2 /out/disk.qcow2; \
    rm /disk.raw; \
    cd /out; \
    sha256sum disk.qcow2 ordinary-input-readback.json > SHA256SUMS

FROM scratch AS disk
COPY --from=disk-build /out/ /
