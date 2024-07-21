# syntax=docker/dockerfile:1

ARG DISTRIBUTION_VERSION
ARG RUST_VERSION
ARG XX_VERSION=latest

FROM --platform=$BUILDPLATFORM tonistiigi/xx:${XX_VERSION} AS xx
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-alpine${DISTRIBUTION_VERSION} AS build-base
COPY --from=xx / /

SHELL [ "/bin/ash", "-c" ]

# install host dependencies
ARG DISTRIBUTION_PACKAGES
RUN \
  --mount=type=cache,target=/var/cache/apk,sharing=private \
  --mount=type=cache,target=/etc/apk/cache,sharing=private \
<<EOF

apk upgrade -Ua
apk add --no-cache \
  bash clang lld llvm file git sccache \
  cmake pkgconf build-base abuild curl mold \
  ${DISTRIBUTION_PACKAGES}
EOF

SHELL [ "/bin/bash", "-c" ]

WORKDIR /source
COPY --link . .

FROM build-base AS build-prep

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL='sparse'
ENV CARGO_TARGET_DIR='/root/.cache/rust'
RUN \
    --mount=type=cache,target=/cargo/git/db,sharing=locked \
    --mount=type=cache,target=/cargo/registry/cache,sharing=locked \
    --mount=type=cache,target=/cargo/registry/index,sharing=locked \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

xx-cargo fetch --locked
EOF

# Install target dependencies
ARG TARGETPLATFORM
ARG DISTRIBUTION_PACKAGES
RUN \
  --mount=type=cache,target=/var/cache/apk,sharing=private \
  --mount=type=cache,target=/etc/apk/cache,sharing=private \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

xx-apk add \
  "xx-cxx-essentials" \
  ${DISTRIBUTION_PACKAGES}
EOF

FROM build-prep AS build

ARG PACKAGE_NAME
ENV PACKAGE_NAME="${PACKAGE_NAME}"

ARG OUTPUT_DIR="/output"
ENV OUTPUT_DIR="${OUTPUT_DIR}"

ARG CARGO_BUILD_INCREMENTAL='false'
ENV CARGO_TARGET_DIR='/root/.cache/rust'

ARG RELEASE_TAG_NAME
ENV RELEASE_TAG_NAME="${RELEASE_TAG_NAME}"

RUN \
    --mount=type=cache,target=/cargo/git/db,sharing=locked \
    --mount=type=cache,target=/cargo/registry/cache,sharing=locked \
    --mount=type=cache,target=/cargo/registry/index,sharing=locked \
    --mount=type=cache,target=/root/.cache,sharing=private \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

xx-clang --setup-target-triple
xx-clang --wrap

if ! xx-info is-cross; then
  export OPENSSL_DIR="/usr"
else
  export OPENSSL_DIR="/$(xx-info triple)/usr"
fi

export LIBZ_SYS_STATIC="1"
export LIBSSH2_STATIC="1"
export LIBGIT2_STATIC="1"
export OPENSSL_STATIC="1"
export PKG_CONFIG_ALL_STATIC="1"
export OPENSSL_NO_VENDOR="1"
export ZSTD_SYS_USE_PKG_CONFIG="1"
export RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold -C target-feature=+crt-static"
export PKG_CONFIG="$(xx-clang --print-prog-name=pkg-config)"

xx-cargo build \
  --verbose \
  --frozen \
  --bin lapce-proxy \
  --profile release-lto \
  --no-default-features

xx-verify "${CARGO_TARGET_DIR}"/"$(xx-cargo --print-target-triple)"/release-lto/lapce-proxy

mkdir -p /target
mv -v "${CARGO_TARGET_DIR}"/"$(xx-cargo --print-target-triple)"/release-lto/lapce-proxy /usr/local/bin/
EOF

FROM build-prep AS dev

FROM scratch AS binary
COPY --from=build /usr/local/bin/lapce-proxy .

FROM scratch AS cross-binary
COPY --from=build /usr/local/bin/lapce-proxy .
