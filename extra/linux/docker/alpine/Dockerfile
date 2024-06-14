# syntax=docker/dockerfile:1

ARG DISTRIBUTION_VERSION
ARG RUST_VERSION
ARG XX_VERSION=latest

FROM --platform=$BUILDPLATFORM tonistiigi/xx:${XX_VERSION} AS xx
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-alpine${DISTRIBUTION_VERSION} AS build-base
COPY --from=xx / /

SHELL [ "/bin/ash", "-c" ]

ENV DEBIAN_FRONTEND=noninteractive

# install host dependencies
ARG DISTRIBUTION_PACKAGES
RUN \
  --mount=type=cache,target=/var/cache/apt,sharing=private \
  --mount=type=cache,target=/var/lib/apt,sharing=private \
<<EOF

apk upgrade -Ua
apk add --no-cache \
  bash clang lld llvm file git sccache \
  cmake pkgconf build-base abuild \
  ${DISTRIBUTION_PACKAGES}
EOF

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
  --mount=type=cache,target=/var/cache/apt,sharing=private \
  --mount=type=cache,target=/var/lib/apt,sharing=private \
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

ENV CC='xx-clang'
ENV CXX='xx-clang++'

ENV OPENSSL_NO_VENDOR="1"
ENV ZSTD_SYS_USE_PKG_CONFIG="1"

ARG RELEASE_TAG_NAME
ENV RELEASE_TAG_NAME="${RELEASE_TAG_NAME}"

ENV LIBZ_SYS_STATIC="1"
ENV LIBSSH2_STATIC="1"
ENV LIBGIT2_STATIC="1"
ENV OPENSSL_STATIC="1"
ENV PKG_CONFIG_ALL_STATIC="1"

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

export RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=/usr/bin/ld.lld"
export PKG_CONFIG="$(xx-clang --print-prog-name=pkg-config)"
export CARGO_BUILD_TARGET="$(xx-cargo --print-target-triple)"

xx-cargo build \
  --frozen \
  --package lapce-app \
  --profile release-lto \
  --no-default-features 
  
export RUSTFLAGS="${RUSTFLAGS} -C target-feature=+crt-static"

xx-cargo build

xx-verify "${CARGO_TARGET_DIR}"/"$(xx-cargo --print-target-triple)"/release-lto/lapce

mkdir -p /target
mv -v "${CARGO_TARGET_DIR}"/"$(xx-cargo --print-target-triple)"/release-lto/lapce /target/

cargo pkgid | cut -d'#' -f2 | cut -d'@' -f2 | cut -d':' -f2 | tee /target/lapce.version
EOF

WORKDIR /output
RUN <<EOF
#!/usr/bin/env bash
set -euxo pipefail

echo 'a'
EOF

FROM build-base AS dev
COPY . ./dev

FROM scratch AS binary
COPY --from=build /output/lapce .

FROM scratch AS cross-binary
COPY --from=build /output/lapce .

# FROM scratch AS package
# COPY --from=build /output/*.deb .

# FROM scratch AS cross-package
# COPY --from=build /output/*.deb .
