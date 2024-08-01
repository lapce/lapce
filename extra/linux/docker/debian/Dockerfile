# syntax=docker/dockerfile:1

ARG DISTRIBUTION_VERSION
ARG RUST_VERSION
ARG XX_VERSION=latest

FROM --platform=$BUILDPLATFORM tonistiigi/xx:${XX_VERSION} AS xx
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-${DISTRIBUTION_VERSION} AS build-base
COPY --from=xx / /

SHELL [ "/bin/bash", "-c" ]

ENV DEBIAN_FRONTEND=noninteractive

# install host dependencies
ARG DISTRIBUTION_PACKAGES
RUN \
  --mount=type=cache,target=/var/cache/apt,sharing=private \
  --mount=type=cache,target=/var/lib/apt,sharing=private \
<<EOF

apt-get update -y
apt-get install -y \
  bash clang lld llvm file cmake pkg-config dpkg-dev \
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

cargo fetch --locked
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

xx-apt-get install -y \
  "xx-cxx-essentials" \
  ${DISTRIBUTION_PACKAGES}
EOF

FROM build-prep AS build

ARG PACKAGE_NAME
ENV PACKAGE_NAME="${PACKAGE_NAME}"

ARG OUTPUT_DIR="/output"
ENV OUTPUT_DIR="${OUTPUT_DIR}"

# in bullseye arm64 target does not link with lld so configure it to use ld instead
# RUN [ ! -f /etc/alpine-release ] && xx-info is-cross && [ "$(xx-info arch)" = "arm64" ] && XX_CC_PREFER_LINKER=ld xx-clang --setup-target-triple || true

ARG CARGO_BUILD_INCREMENTAL='false'
ENV CARGO_TARGET_DIR='/root/.cache/rust'

ENV CC='xx-clang'
ENV CXX='xx-clang++'

ENV OPENSSL_NO_VENDOR="1"
ENV ZSTD_SYS_USE_PKG_CONFIG="1"

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

export RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=/usr/bin/ld.lld"
export PKG_CONFIG="$(xx-clang --print-prog-name=pkg-config)"
export CARGO_BUILD_TARGET="$(xx-cargo --print-target-triple)"

xx-cargo build \
  --frozen \
  --package lapce-app \
  --profile release-lto \
  --no-default-features

xx-verify "${CARGO_TARGET_DIR}"/"$(xx-cargo --print-target-triple)"/release-lto/lapce

mkdir -p /target
mv -v "${CARGO_TARGET_DIR}"/"$(xx-cargo --print-target-triple)"/release-lto/lapce /target/

cargo pkgid | cut -d'#' -f2 | cut -d'@' -f2 | cut -d':' -f2 | tee /target/lapce.version
EOF

WORKDIR /output
RUN <<EOF
#!/usr/bin/env bash
set -euxo pipefail

export _PACKAGE_ARCHITECTURE=$(xx-info debian-arch)

mkdir -v -p ${PACKAGE_NAME}/{etc,usr/{bin,share/{applications,metainfo,pixmaps}},debian}
cd ${PACKAGE_NAME}

cp '/source/extra/linux/dev.lapce.lapce.desktop' './usr/share/applications/dev.lapce.lapce.desktop'
cp '/source/extra/linux/dev.lapce.lapce.metainfo.xml' './usr/share/metainfo/dev.lapce.lapce.metainfo.xml'
cp '/source/extra/images/logo.png' './usr/share/pixmaps/dev.lapce.lapce.png'

mv '/target/lapce' './usr/bin/'

if [[ "${PACKAGE_NAME}" == "lapce" ]]; then
  conflicts="lapce-nightly"
else
  conflicts="lapce"
fi

case "${RELEASE_TAG_NAME}" in
  nightly-*)
    version=$(cat /target/lapce.version)
    commit=$(echo "${RELEASE_TAG_NAME}" | cut -d'-' -f2)
    # date=$(date +%Y%m%d%H%M)
    RELEASE_TAG_NAME="${version}+${commit}"
  ;;
  debug|nightly)
    version=$(cat /target/lapce.version)
    date=$(date +%Y%m%d%H%M)
    RELEASE_TAG_NAME="${version}+${date}"
  ;;
  *)
    RELEASE_TAG_NAME="${RELEASE_TAG_NAME//v/}"
  ;;
esac

cat <<- EOL > debian/control
	Package: ${PACKAGE_NAME}
	Version: ${RELEASE_TAG_NAME}
	Conflicts: ${conflicts}
	Maintainer: Jakub Panek <me+debian@panekj.dev>
	Architecture: ${_PACKAGE_ARCHITECTURE}
	Description: Lightning-fast and Powerful Code Editor
	Source: https://lapce.dev
EOL

depends=$(dpkg-shlibdeps -O -e usr/bin/lapce)
depends=$(echo "${depends}" | sed 's/shlibs:Depends=//')
echo "Depends: ${depends}" >> debian/control
mv debian DEBIAN

. /etc/os-release

dpkg-deb --root-owner-group --build . "${OUTPUT_DIR}"/"${PACKAGE_NAME}.${ID}.${VERSION_CODENAME}.${_PACKAGE_ARCHITECTURE}.deb"
EOF

FROM build-base AS dev
COPY . ./dev

FROM scratch AS binary
COPY --from=build /output/lapce .

FROM scratch AS cross-binary
COPY --from=build /output/lapce .

FROM scratch AS package
COPY --from=build /output/*.deb .

FROM scratch AS cross-package
COPY --from=build /output/*.deb .
