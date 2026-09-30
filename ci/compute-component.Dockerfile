# syntax=docker/dockerfile:1.7
ARG BASE_IMAGE
FROM ${BASE_IMAGE}
ARG BINARY
USER root
RUN test -x "$BINARY" && cp "$BINARY" /usr/local/bin/component
USER postgres
ENTRYPOINT ["/usr/local/bin/component"]
CMD []
