# syntax=docker/dockerfile:1.7
# Production build dependencies. Test/coverage/browser tooling stays out of this image.
FROM debian:bookworm-slim@sha256:40b107342c492725bc7aacbe93a49945445191ae364184a6d24fedb28172f6f7
SHELL ["/bin/bash", "-euo", "pipefail", "-c"]
RUN echo 'Acquire::Retries "5";' > /etc/apt/apt.conf.d/80-retries \
    && apt-get update && apt-get install -y --no-install-recommends \
       autoconf automake bison build-essential ca-certificates clang cmake curl flex \
       git jq libcurl4-openssl-dev libreadline-dev libseccomp-dev libssl-dev libtool \
       libxml2-dev libzstd-dev mold pkg-config unzip wget zlib1g-dev \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 1000 -s /bin/bash nonroot
ARG BUILD_JOBS=2
# Storage initdb must use the same ICU version as the PG14-16 computes.
RUN curl -fL --retry 5 https://github.com/unicode-org/icu/releases/download/release-67-1/icu4c-67_1-src.tgz -o /tmp/icu.tgz \
    && echo '94a80cd6f251a53bd2a997f6f1b5ac6653fe791dfab66e1eb0227740fb86d5dc /tmp/icu.tgz' | sha256sum -c - \
    && mkdir /tmp/icu && tar -xzf /tmp/icu.tgz -C /tmp/icu --strip-components=1 \
    && cd /tmp/icu/source \
    && ./configure --prefix=/usr/local/icu --enable-static --enable-shared=no CXXFLAGS=-fPIC CFLAGS=-fPIC \
    && make -j "$BUILD_JOBS" && make install && rm -rf /tmp/icu /tmp/icu.tgz
RUN curl -fL --retry 5 https://github.com/protocolbuffers/protobuf/releases/download/v25.1/protoc-25.1-linux-$(uname -m | sed 's/aarch64/aarch_64/').zip -o /tmp/protoc.zip \
    && unzip -q /tmp/protoc.zip -d /tmp/protoc \
    && cp /tmp/protoc/bin/protoc /usr/local/bin/ \
    && cp -r /tmp/protoc/include/* /usr/local/include/ \
    && rm -rf /tmp/protoc /tmp/protoc.zip
USER nonroot
WORKDIR /home/nonroot
ENV PATH=/home/nonroot/.cargo/bin:$PATH \
    RUSTUP_HOME=/home/nonroot/.rustup \
    CARGO_BUILD_JOBS=2 \
    CARGO_PROFILE_RELEASE_DEBUG=0
RUN curl -fL --retry 5 https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.88.0 \
    && cargo install --locked --version 0.1.71 cargo-chef \
    && cargo install --locked --version 0.7.0 cargo-auditable \
    && rm -rf /home/nonroot/.cargo/registry /home/nonroot/.cargo/git \
    && touch /home/nonroot/.docker_build
