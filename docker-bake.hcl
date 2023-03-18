variable "RUST_VERSION" {
    default = "1.68.0"
}
variable "VERSION" {
    default = ""
}
variable "USE_GLIBC" {
    default = "true"
}
variable "STRIP_TARGET" {
    default = ""
}
variable "DEBIAN_DEPS" {
    default = "bash clang lld llvm file cmake pkg-config"
}
# Sets the name of the company that produced the windows binary.
variable "PACKAGER_NAME" {
    default = ""
}

target "_common" {
    dockerfile = "./extra/linux/Dockerfile"
    args = {
        RUST_VERSION = RUST_VERSION
        BUILDKIT_CONTEXT_KEEP_GIT_DIR = 1
    }
}

target "_platforms" {
    platforms = [
        // "darwin/amd64",
        // "darwin/arm64",
        "linux/amd64",
        // "linux/arm/v6",
        // "linux/arm/v7",
        "linux/arm64",
        // "linux/ppc64le",
        // "linux/riscv64",
        // "linux/s390x",
        // "windows/amd64",
        // "windows/arm64"
    ]
}

target "_proxy_platforms" {
    platforms = [
        // "darwin/amd64",
        // "darwin/arm64",
        "linux/amd64",
        // "linux/arm/v6",
        // "linux/arm/v7",
        "linux/arm64",
        // "linux/ppc64le",
        // "linux/riscv64",
        // "linux/s390x",
        // "windows/amd64",
        // "windows/arm64"
    ]
}

group "default" {
  targets = ["binary"]
}

target "binary" {
    inherits  = ["_common"]
    target    = "binary"
    platforms = ["local"]
    output    = ["build"]
    args = {
        BASE_VARIANT = USE_GLIBC != "" ? "debian" : "alpine"
        VERSION      = VERSION
        DEBIAN_DEPS  = DEBIAN_DEPS
    }
    env = {
        CARGO_BUILD_PROFILE = "release"
    }
}

target "cross-binary" {
    inherits = ["binary", "_platforms"]
}

target "proxy" {
    inherits = ["binary"]
    target   = "proxy"
    output   = ["proxy"]
    args = {
        BASE_VARIANT = "alpine"
        VERSION      = VERSION
    }
}

target "cross-proxy" {
    inherits = ["proxy", "_proxy_platforms"]
}
