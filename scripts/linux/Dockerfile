# Melora's Linux build image. Ubuntu 22.04 (glibc 2.35) is the oldest base the packages
# support, so they also install on newer distributions. scripts/build-linux.mjs downloads
# and checks the Node and Rust archives in inputs/; this file installs Ubuntu's signed
# packages and unpacks those archives. The build itself runs without network access.
FROM ubuntu:22.04@sha256:0a9887931b0e38af2e3487ed39e29436b0535c4f3f52e57745baaf79d1898956

ARG ARCH
ARG NODE
ARG RUST
ENV DEBIAN_FRONTEND=noninteractive

# aarch64 is cross-compiled: its WebKitGTK headers conflict with the x86_64 ones, so each
# architecture has its own image.
RUN set -eu; \
    if [ "$ARCH" = aarch64 ]; then \
      sed -i 's|^deb http|deb [arch=amd64] http|' /etc/apt/sources.list; \
      for suite in jammy jammy-updates jammy-security; do \
        echo "deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports $suite main universe"; \
      done > /etc/apt/sources.list.d/arm64.list; \
      dpkg --add-architecture arm64; \
      packages="gcc-aarch64-linux-gnu libc6-dev-arm64-cross libwebkit2gtk-4.1-dev:arm64"; \
    else \
      packages="libwebkit2gtk-4.1-dev librsvg2-common"; \
    fi; \
    apt-get update; \
    apt-get install -y --no-install-recommends build-essential pkg-config file xz-utils \
      python3 rpm cpio $packages; \
    rm -rf /var/lib/apt/lists/*

# Mounted, not copied, so the archives stay out of the image's layers.
RUN --mount=type=bind,source=inputs,target=/tmp/inputs \
    set -eu; \
    mkdir /opt/node /tmp/rust; \
    tar -xJf "/tmp/inputs/node-v$NODE-linux-x64.tar.xz" -C /opt/node --strip-components=1; \
    tar -xJf "/tmp/inputs/rust-$RUST-x86_64-unknown-linux-gnu.tar.xz" -C /tmp/rust --strip-components=1; \
    /tmp/rust/install.sh --prefix=/opt/rust --components=rustc,cargo,rust-std-x86_64-unknown-linux-gnu; \
    if [ "$ARCH" = aarch64 ]; then \
      mkdir /tmp/std; \
      tar -xJf "/tmp/inputs/rust-std-$RUST-aarch64-unknown-linux-gnu.tar.xz" -C /tmp/std --strip-components=1; \
      /tmp/std/install.sh --prefix=/opt/rust; \
    fi; \
    rm -rf /tmp/rust /tmp/std

ENV PATH=/opt/rust/bin:/opt/node/bin:$PATH
