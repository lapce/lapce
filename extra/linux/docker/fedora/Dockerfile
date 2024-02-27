# syntax=docker/dockerfile:1

ARG DISTRIBUTION_VERSION
ARG RUST_VERSION
ARG XX_VERSION=latest

FROM --platform=$BUILDPLATFORM ghcr.io/panekj/xx:${XX_VERSION} AS xx
FROM --platform=$BUILDPLATFORM fedora:${DISTRIBUTION_VERSION} AS build-base
COPY --from=xx --link / /

SHELL [ "/bin/bash", "-c" ]

# install host dependencies
ARG DISTRIBUTION_PACKAGES
RUN \
  --mount=type=cache,target=/var/cache/dnf,sharing=locked \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

printf "keepcache=True" >> /etc/dnf/dnf.conf
dnf update -y
dnf install -y \
  bash clang lld llvm file cmake pkg-config curl git rpm-build
EOF

ENV CARGO_HOME="/cargo"
ENV RUSTUP_HOME="/rustup"

RUN \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
EOF

ENV PATH="${CARGO_HOME}/bin:${PATH}"

WORKDIR /source

FROM build-base AS build-prep

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL='sparse'
ENV CARGO_TARGET_DIR='/target'
ENV CARGO_NET_GIT_FETCH_WITH_CLI="true"

COPY --link . .

RUN \
    --mount=type=cache,target=/cargo/git/db,sharing=locked \
    --mount=type=cache,target=/cargo/registry/cache,sharing=locked \
    --mount=type=cache,target=/cargo/registry/index,sharing=locked \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

cargo fetch --locked
EOF

# Install target dependencies
ARG TARGETPLATFORM
ARG DISTRIBUTION_PACKAGES
RUN \
  --mount=type=cache,target=/var/cache/dnf,sharing=locked \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

mkdir -p /run/lock
mkdir -p /var/run/lock
XX_DEBUG_DNF=1 \
xx-dnf install -y \
  "xx-cxx-essentials" \
  ${DISTRIBUTION_PACKAGES}
EOF

FROM build-prep AS build

ARG PACKAGE_NAME
ENV PACKAGE_NAME="${PACKAGE_NAME}"

ARG OUTPUT_DIR="/output"
ENV OUTPUT_DIR="${OUTPUT_DIR}"

ARG CARGO_BUILD_INCREMENTAL='false'

ENV CC='xx-clang'
ENV CXX='xx-clang++'

ENV OPENSSL_NO_VENDOR="1"
ENV ZSTD_SYS_USE_PKG_CONFIG="1"

ARG RELEASE_TAG_NAME
ENV RELEASE_TAG_NAME="${RELEASE_TAG_NAME}"

RUN \
    --mount=type=cache,target=/cargo/git/db,sharing=locked,readonly=true \
    --mount=type=cache,target=/cargo/registry/cache,sharing=locked,readonly=true \
    --mount=type=cache,target=/cargo/registry/index,sharing=locked,readonly=true \
    --mount=type=cache,target=/root/.cache,sharing=private \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

xx-clang --setup-target-triple
xx-clang --wrap

export RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=/usr/bin/ld.lld"
export PKG_CONFIG="$(xx-clang --print-prog-name=pkg-config)"
export CARGO_BUILD_TARGET="$(xx-cargo --print-target-triple)"

rustup target add "$(xx-cargo --print-target-triple)"
xx-cargo build --profile release-lto --bin lapce --frozen
xx-verify "${CARGO_TARGET_DIR}"/"$(xx-cargo --print-target-triple)"/release-lto/lapce

mkdir -p /target
mv -v "${CARGO_TARGET_DIR}"/"$(xx-cargo --print-target-triple)"/release-lto/lapce /target/
EOF

WORKDIR /output
RUN <<EOF
#!/usr/bin/env bash
set -euxo pipefail

export _PACKAGE_ARCHITECTURE=$(xx-info rhel-arch)

mkdir -v -p ${PACKAGE_NAME}/{etc,usr/bin}
cd ${PACKAGE_NAME}

mv /target/lapce ./usr/bin/

if [ "${PACKAGE_NAME}" = "lapce" ]; then
  conflicts="lapce-nightly"
else
  conflicts="lapce"
fi

if [ "${RELEASE_TAG_NAME}" = "nightly" ]; then
  RELEASE_TAG_NAME="v0.0.1"
fi

EOF

FROM build-base AS dev
COPY . ./dev

FROM scratch AS binary
COPY --from=build /output/*.deb .

FROM scratch AS cross-binary
COPY --from=build /output/*.deb .
