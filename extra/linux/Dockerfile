# syntax=docker/dockerfile:1

# Image ARGs go here
ARG BASE_VARIANT=debian
ARG ALPINE_VERSION=3.17
ARG DEBIAN_VERSION=bullseye
ARG RUST_VERSION=latest
ARG XX_VERSION=1.2.1
#

# FROM --platform=$BUILDPLATFORM ubuntu:18.04 AS ubuntu18

FROM --platform=$BUILDPLATFORM tonistiigi/xx:${XX_VERSION} AS xx

FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS build-base-alpine
COPY --from=xx / /
# install sccache
RUN cargo install --locked sccache
# install host dependencies
RUN apk add --no-cache bash clang lld llvm file git
WORKDIR /build

FROM build-base-alpine AS build-alpine
# ENV RUSTC_WRAPPER='/root/.cargo/bin/sccache'
RUN --mount=type=bind,target=.,ro \
    --mount=type=cache,target=/root/.cargo/git/db \
    --mount=type=cache,target=/root/.cargo/registry/cache \
    --mount=type=cache,target=/root/.cargo/registry/index \
    --mount=type=cache,target=/root/.cache \
    cargo fetch --locked
ARG TARGETPLATFORM
# gcc is installed for libgcc only
RUN xx-apk add --no-cache musl-dev gcc

FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-${DEBIAN_VERSION} AS build-base-debian
COPY --from=xx / /
# install sccache
RUN cargo install --locked sccache
# install host dependencies
ARG DEBIAN_DEPS
RUN apt-get update && \
    apt-get install --no-install-recommends -y \
    ${DEBIAN_DEPS}
WORKDIR /build

FROM build-base-debian AS build-debian
# ENV RUSTC_WRAPPER='/root/.cargo/bin/sccache'
RUN --mount=type=bind,target=.,ro \
    --mount=type=cache,target=/root/.cargo/git/db \
    --mount=type=cache,target=/root/.cargo/registry/cache \
    --mount=type=cache,target=/root/.cargo/registry/index \
    --mount=type=cache,target=/root/.cache \
    cargo fetch --locked
ARG TARGETPLATFORM
RUN xx-apt-get install --no-install-recommends -y libc6-dev libgcc-10-dev libfontconfig-dev libgtk-3-dev libstdc++-10-dev

FROM build-${BASE_VARIANT} AS build
# in bullseye arm64 target does not link with lld so configure it to use ld instead
RUN [ ! -f /etc/alpine-release ] && xx-info is-cross && [ "$(xx-info arch)" = "arm64" ] && XX_CC_PREFER_LINKER=ld xx-clang --setup-target-triple || true
# ENV RUSTC_WRAPPER='/root/.cargo/bin/sccache'
ENV CARGO_BUILD_INCREMENTAL='false'
ENV CARGO_TARGET_DIR='/output'
ENV CC='xx-clang'
ENV CXX='xx-clang++'
RUN --mount=type=bind,target=.,ro \
    --mount=type=cache,target=/root/.cargo/git/db \
    --mount=type=cache,target=/root/.cargo/registry/cache \
    --mount=type=cache,target=/root/.cargo/registry/index \
    --mount=type=cache,target=/root/.cache \
    --mount=from=dockercore/golang-cross:xx-sdk-extras,target=/xx-sdk,src=/xx-sdk \
    # xx-clang --setup-target-triple \
    # xx-clang --wrap && \
    export RUSTFLAGS="-C linker=$(xx-clang --print-prog=lld)" && \
    export PKG_CONFIG="$(xx-clang --print-prog-name=pkg-config)" && \
    export CARGO_BUILD_TARGET="$(xx-cargo --print-target-triple)" && \
    cargo build \
        --profile ${CARGO_BUILD_PROFILE:-release}
        # ${CARGO_BUILD_ALL_FEATURES:-} \
        # $([ -z "${BUILD_BINS}" ] && echo "--bins ${BUILD_BINS}") \
        # $([ -z "${BUILD_FEATURES}" ] && echo "--bins ${BUILD_FEATURES}")
    # xx-verify /output/"$(xx-cargo --print-target-triple)"/release-lto/lapce \
    # xx-verify /output/"$(xx-cargo --print-target-triple)"/release-lto/lapce-proxy

FROM build-base-${BASE_VARIANT} AS dev
COPY . .

FROM scratch AS binary
COPY --from=build /output .
