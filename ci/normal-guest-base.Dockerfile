# Reuse the locked ordinary Guest system carrier and install this release's executables.
ARG CARRIER_IMAGE
ARG COMPUTE_IMAGE
ARG DAEMON_IMAGE
FROM ${COMPUTE_IMAGE} AS compute
FROM ${DAEMON_IMAGE} AS daemon
FROM ${CARRIER_IMAGE}

ARG CARRIER_IMAGE
ARG COMPUTE_IMAGE
ARG DAEMON_IMAGE
ARG NEON_SOURCE
ARG AUTOSCALING_SOURCE
ARG POSTGRES_SOURCE
ARG POSTGRES_MAJOR
ARG ORDINARY_PACKAGE_LOCK_SHA256

USER root
SHELL ["/neonvm/bin/busybox", "sh", "-c"]
RUN --network=none set -eu; \
    for path in /var/db/postgres/compute/PG_VERSION /run/neon-dsga/vmstart.allowed \
                /neonvm/config/dsga-normal-system-source.json; do \
        if [ -e "$path" ] || [ -L "$path" ]; then \
            printf 'Ordinary Guest carrier has pre-existing state at %s\n' "$path" >&2; \
            exit 1; \
        fi; \
    done; \
    seed=/usr/share/neon-dsga/native-build.json; \
    test -f "$seed"; \
    test ! -L "$seed"; \
    /neonvm/bin/busybox rm -rf /usr/local
# The Builder independently reads the root and replaces the original NativeBirth seed.
# /usr/local belongs to the exact runtime image for the selected PG major.
# The old carrier contributes the locked ordinary system and launcher only.
COPY --chown=0:0 --from=compute /usr/local/ /usr/local/
COPY --chown=0:0 --chmod=0755 --from=daemon /neonvmd /neonvm/bin/neonvmd
COPY --chown=0:0 --chmod=4755 --from=daemon /dsga-resource-guard /neonvm/bin/dsga-resource-guard

LABEL org.opencontainers.image.revision="${NEON_SOURCE}" \
      io.neon.pg.major="${POSTGRES_MAJOR}" \
      io.neon.pg.source="${POSTGRES_SOURCE}" \
      io.neon.guest.autoscaling.source="${AUTOSCALING_SOURCE}" \
      io.neon.ordinary.package-lock.sha256="${ORDINARY_PACKAGE_LOCK_SHA256}" \
      io.neon.guest.carrier="${CARRIER_IMAGE}" \
      io.neon.guest.compute="${COMPUTE_IMAGE}" \
      io.neon.guest.daemon="${DAEMON_IMAGE}"
