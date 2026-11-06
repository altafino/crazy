# The export build environment (PRD F22, eng review D2/D3/D12). Ubuntu 22.04 keeps the Linux
# binaries on glibc 2.35, so they run on current distributions. The image tag is
# `ai-app-studio-export:<short hash of this file>`, so changing this file builds a new image.
#
# The container runs as the host user (`docker run --user uid:gid`), so everything the build
# needs at run time must be readable by any uid, and the cache folders below must be writable
# by any uid (named volumes copy the permissions of these folders on first mount).
FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates gnupg \
        build-essential curl file pkg-config git \
        libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev \
        patchelf xdg-utils nsis lld llvm clang \
    && rm -rf /var/lib/apt/lists/*

# Node 22 with corepack (pnpm, yarn; npm comes with Node).
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/* \
    && corepack enable

# Rust (stable) and the Windows target, installed where any uid can read them. At run time
# CARGO_HOME points at the cache volume (registry and git downloads only); the toolchain and
# cargo-xwin stay in /usr/local.
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
    && rustup target add x86_64-pc-windows-msvc \
    && cargo install --locked cargo-xwin \
    && rm -rf /usr/local/cargo/registry /usr/local/cargo/git \
    && chmod -R a+rX /usr/local/rustup /usr/local/cargo

# Download caches (named volumes mount here) and a home for the unprivileged uid.
RUN mkdir -p /cache/cargo /cache/pnpm /cache/npm /cache/xwin /tmp/home \
    && chmod 1777 /cache/cargo /cache/pnpm /cache/npm /cache/xwin /tmp/home

ENV HOME=/tmp/home \
    CARGO_HOME=/cache/cargo \
    npm_config_cache=/cache/npm \
    npm_config_store_dir=/cache/pnpm \
    YARN_CACHE_FOLDER=/cache/npm/yarn \
    COREPACK_HOME=/cache/npm/corepack \
    COREPACK_ENABLE_DOWNLOAD_PROMPT=0 \
    XWIN_CACHE_DIR=/cache/xwin \
    APPIMAGE_EXTRACT_AND_RUN=1 \
    NO_STRIP=true

WORKDIR /src
