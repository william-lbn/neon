# 最终 OCI 保留锁定内核，并只叠加已验证的 Guest 磁盘。
ARG BOOT_KERNEL_IMAGE
FROM ${BOOT_KERNEL_IMAGE}

ARG NEON_SOURCE
ARG AUTOSCALING_SOURCE
ARG POSTGRES_SOURCE
ARG POSTGRES_MAJOR
ARG ORDINARY_PACKAGE_LOCK_SHA256

COPY --chown=0:0 --chmod=0644 disk.qcow2 /disk.qcow2
LABEL org.opencontainers.image.revision="${NEON_SOURCE}" \
      io.neon.pg.major="${POSTGRES_MAJOR}" \
      io.neon.pg.source="${POSTGRES_SOURCE}" \
      io.neon.guest.autoscaling.source="${AUTOSCALING_SOURCE}" \
      io.neon.ordinary.package-lock.sha256="${ORDINARY_PACKAGE_LOCK_SHA256}"
