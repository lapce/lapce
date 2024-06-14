# syntax=docker/dockerfile:1

ARG DISTRIBUTION_VERSION
ARG RUST_VERSION
ARG XX_VERSION

FROM --platform=$BUILDPLATFORM tonistiigi/xx:${XX_VERSION} AS xx
FROM --platform=$BUILDPLATFORM ubuntu:${DISTRIBUTION_VERSION} AS build-base
COPY --from=xx / /

SHELL [ "/bin/bash", "-c" ]

ENV DEBIAN_FRONTEND=noninteractive

RUN \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

rm -f /etc/apt/apt.conf.d/docker-clean
echo 'Binary::apt::APT::Keep-Downloaded-Packages "true";' > /etc/apt/apt.conf.d/keep-cache
EOF

# install host dependencies
ARG DISTRIBUTION_PACKAGES
RUN \
  --mount=type=cache,target=/var/cache/apt,sharing=private \
  --mount=type=cache,target=/var/lib/apt,sharing=private \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

apt-get update -y
apt-get upgrade -y
apt-get install -y \
  bash clang lld llvm file cmake pkg-config curl git dpkg-dev \
  ${DISTRIBUTION_PACKAGES}
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
COPY --link . .

FROM build-base AS build-prep

ENV CARGO_TARGET_DIR='/target'

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

ARG OUTPUT_DIR="/output"

ARG CARGO_BUILD_INCREMENTAL='false'

ENV CC='xx-clang'
ENV CXX='xx-clang++'

ARG ZSTD_SYS_USE_PKG_CONFIG
ARG LIBGIT2_STATIC
ARG LIBSSH2_STATIC
ARG LIBZ_SYS_STATIC
ARG OPENSSL_STATIC
ARG OPENSSL_NO_VENDOR
ARG PKG_CONFIG_ALL_STATIC

ARG RELEASE_TAG_NAME

RUN \
    --mount=type=cache,target=/cargo/git/db,sharing=shared,readonly \
    --mount=type=cache,target=/cargo/registry/cache,sharing=shared,readonly \
    --mount=type=cache,target=/cargo/registry/index,sharing=shared,readonly \
    --mount=type=cache,target=/root/.cache,sharing=private \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

xx-clang --setup-target-triple
xx-clang --wrap

export RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=/usr/bin/ld.lld"
export PKG_CONFIG="$(xx-clang --print-prog-name=pkg-config)"
export CARGO_BUILD_TARGET="$(xx-cargo --print-target-triple)"

env

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

FROM build-base AS dev
COPY . ./dev

FROM scratch AS binary
COPY --from=build /target/lapce .

FROM scratch AS cross-binary
COPY --from=build /target/lapce .

FROM build AS package-prepare
WORKDIR /output
COPY --from=build /target /target
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

FROM scratch AS package
COPY --from=package-prepare /output/*.deb .

FROM scratch AS cross-package
COPY --from=package-prepare /output/*.deb .
