# syntax=docker/dockerfile:1.7
ARG BASE_IMAGE
FROM ${BASE_IMAGE}
ARG BINARY
USER root
# Preserve runtime dependencies and copy a fixed executable to a common entrypoint.
RUN test -x "$BINARY" && cp "$BINARY" /usr/local/bin/component
USER neon
ENTRYPOINT ["/usr/local/bin/component"]
CMD []
