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

ARG CARGO_BUILD_INCREMENTAL='false'

ENV CC='xx-clang'
ENV CXX='xx-clang++'

ENV OPENSSL_NO_VENDOR="1"
ENV ZSTD_SYS_USE_PKG_CONFIG="1"

ARG RELEASE_TAG_NAME
ENV RELEASE_TAG_NAME="${RELEASE_TAG_NAME}"

RUN \
    --mount=type=cache,target=/cargo/git/db,readonly=true \
    --mount=type=cache,target=/cargo/registry/cache,readonly=true \
    --mount=type=cache,target=/cargo/registry/index,readonly=true \
    --mount=type=cache,target=/root/.cache,sharing=private \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

LAPCE_VERSION=$(cargo pkgid | cut -d'#' -f2 | cut -d'@' -f2 | cut -d':' -f2)

case "${RELEASE_TAG_NAME}" in
  nightly-*)
    commit=$(echo "${RELEASE_TAG_NAME}" | cut -d'-' -f2)
    RELEASE_TAG_NAME="${LAPCE_VERSION}+${commit}"
  ;;
  debug|nightly)
    date=$(date +%Y%m%d%H%M)
    RELEASE_TAG_NAME="${LAPCE_VERSION}+${date}"
  ;;
  *)
    RELEASE_TAG_NAME="${RELEASE_TAG_NAME//v/}"
  ;;
esac

. /etc/os-release
export DISTRO_ID="$(echo $ID | tr - _)"
export DISTRO_VERSION="$(echo $VERSION_ID | tr - _)"

cat > lapce.spec <<EOL
Name:     ${PACKAGE_NAME}
Version:  ${RELEASE_TAG_NAME}
Release:  1.${DISTRO_ID}${DISTRO_VERSION}
Summary:  Lightning-fast and Powerful Code Editor written in Rust
License:  Apache-2.0
URL:      https://github.com/lapce/lapce
Packager: Jakub Panek

%description
Lapce is written in pure Rust with a UI in Floem (which is also written in Rust).
It is designed with Rope Science from the Xi-Editor which makes for lightning-fast computation, and leverages OpenGL for rendering.

%build
xx-clang --setup-target-triple
xx-clang --wrap
export RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=/usr/bin/ld.lld"
export PKG_CONFIG="$(xx-clang --print-prog-name=pkg-config)"
rustup target add "$(xx-cargo --print-target-triple)"
export RELEASE_TAG_NAME="${RELEASE_TAG_NAME}"
xx-cargo build --profile release-lto --bin lapce --frozen
xx-verify "${CARGO_TARGET_DIR}"/"$(xx-cargo --print-target-triple)"/release-lto/lapce

%install
install -Dm755 "${CARGO_TARGET_DIR}"/"$(xx-cargo --print-target-triple)"/release-lto/lapce %{buildroot}%{_bindir}/lapce
install -Dm644 extra/linux/dev.lapce.lapce.desktop %{buildroot}/usr/share/applications/dev.lapce.lapce.desktop
install -Dm644 extra/linux/dev.lapce.lapce.metainfo.xml %{buildroot}/usr/share/metainfo/dev.lapce.lapce.metainfo.xml
install -Dm644 extra/images/logo.png %{buildroot}/usr/share/pixmaps/dev.lapce.lapce.png

%files
%license LICENSE*
%doc *.md
%{_bindir}/lapce
/usr/share/applications/dev.lapce.lapce.desktop
/usr/share/metainfo/dev.lapce.lapce.metainfo.xml
/usr/share/pixmaps/dev.lapce.lapce.png

%changelog
* Thu Apr 25 2024 Jakub Panek
- See GitHub for full changelog
EOL

/usr/bin/rpmbuild --build-in-place --noclean --noprep -bb --rmspec lapce.spec --verbose -D 'debug_package %{nil}'
ls -lahR $HOME/rpmbuild
EOF

RUN \
<<EOF
#!/usr/bin/env bash
set -euxo pipefail

mv -v $HOME/rpmbuild/RPMS/* /output/
EOF

FROM build-base AS dev
COPY . ./dev

FROM scratch AS binary
COPY --from=build /output/lapce .

FROM scratch AS cross-binary
COPY --from=build /output/lapce .

FROM scratch AS package
COPY --from=build /output/*.rpm .

FROM scratch AS cross-package
COPY --from=build /output/*.rpm .
