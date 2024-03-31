variable "RUST_VERSION" {
  default = "1.76"
}

variable "XX_VERSION" {
  default = "master"
}

variable "PACKAGE_NAME" {
  default = RELEASE_TAG_NAME == "nightly" ? "lapce-nightly" : "lapce"
}

variable "RELEASE_TAG_NAME" {}

target "_common" {
  context   = "."
  platforms = ["local"]
  output    = ["target"]
  args = {
    PACKAGE_NAME     = PACKAGE_NAME
    RELEASE_TAG_NAME = RELEASE_TAG_NAME
    RUST_VERSION     = RUST_VERSION
    XX_VERSION       = XX_VERSION
    OPENSSL_STATIC   = OPENSSL_STATIC

    BUILDKIT_CONTEXT_KEEP_GIT_DIR = 1
  }
}

variable "platforms" {
  default = [
    "linux/amd64",
    // "linux/arm/v6",
    // "linux/arm/v7",
    "linux/arm64",
    // "linux/ppc64le",
    // "linux/riscv64",
    // "linux/s390x",
  ]
}

target "_platforms" {
  platforms = platforms
}

group "default" {
  targets = ["binary"]
}

target "binary" {
  inherits = ["_common"]
  target   = "binary"
  args = {
    ZSTD_SYS_USE_PKG_CONFIG = "1"
    LIBGIT2_STATIC          = "1"
    LIBSSH2_STATIC          = "1"
    LIBZ_SYS_STATIC         = "1"
    OPENSSL_STATIC          = "1"
    OPENSSL_NO_VENDOR       = "0"
    PKG_CONFIG_ALL_STATIC   = "1"
  }
}

target "cross-binary" {
  inherits = ["binary", "_platforms"]
  target   = "cross-binary"
}

target "package" {
  inherits = ["_common"]
  target   = "package"
  args = {
    ZSTD_SYS_USE_PKG_CONFIG = "1"
    LIBGIT2_STATIC          = "0"
    LIBSSH2_STATIC          = "0"
    LIBZ_SYS_STATIC         = "0"
    OPENSSL_STATIC          = "0"
    OPENSSL_NO_VENDOR       = "1"
    PKG_CONFIG_ALL_STATIC   = "0"
  }
}

target "cross-package" {
  inherits = ["package", "_platforms"]
  target   = "cross-package"
}

// OS

variable "DPKG_FAMILY_PACKAGES" {
  default = [
    "libc6-dev",
    "libssl-dev",
    "zlib1g-dev",
    "libzstd-dev",
    "libvulkan-dev",
    "libwayland-dev",
    "libxcb-shape0-dev",
    "libxcb-xfixes0-dev",
    "libxkbcommon-x11-dev",
  ]
}

target "debian" {
  inherits   = ["package"]
  name       = "${os_name}-${build.os_version}"
  dockerfile = "extra/linux/docker/${os_name}/Dockerfile"
  args = {
    DISTRIBUTION_NAME     = os_name
    DISTRIBUTION_VERSION  = build.os_version
    DISTRIBUTION_PACKAGES = join(" ", build.packages)
  }
  matrix = {
    os_name = ["debian"]
    build = [
      { os_version = "bullseye", packages = DPKG_FAMILY_PACKAGES },
      { os_version = "bookworm", packages = DPKG_FAMILY_PACKAGES },
    ]
  }
}

target "cross-debian" {
  inherits = ["debian", "cross-package"]
}

target "ubuntu" {
  inherits   = ["package"]
  name       = "${os_name}-${build.os_version}"
  dockerfile = "extra/linux/docker/${os_name}/Dockerfile"
  args = {
    DISTRIBUTION_NAME     = os_name
    DISTRIBUTION_VERSION  = build.os_version
    DISTRIBUTION_PACKAGES = join(" ", build.packages)
  }
  platforms = coalesce(build.platforms, platforms)
  matrix = {
    os_name = ["ubuntu"]
    build = [
      { os_version = "focal", packages = distinct(concat(DPKG_FAMILY_PACKAGES, [])), platforms = null },            # 20.04
      { os_version = "jammy", packages = distinct(concat(DPKG_FAMILY_PACKAGES, [])), platforms = ["linux/amd64"] }, # 22.04
      { os_version = "lunar", packages = distinct(concat(DPKG_FAMILY_PACKAGES, [])), platforms = null },            # 23.04
      { os_version = "mantic", packages = distinct(concat(DPKG_FAMILY_PACKAGES, [])), platforms = null },           # 23.10
      { os_version = "noble", packages = distinct(concat(DPKG_FAMILY_PACKAGES, [])), platforms = null },            # 24.04
    ]
  }
}

target "cross-ubuntu" {
  inherits = ["ubuntu", "cross-package"]
}

variable "RHEL_FAMILY_PACKAGES" {
  default = [
    "openssl-devel",
    "wayland-devel",
    "vulkan-loader-devel",
    "libzstd-devel",
    "libxcb-devel",
    "libxkbcommon-x11-devel",
  ]
}

target "fedora" {
  inherits   = ["package"]
  name       = "${os_name}-${build.os_version}"
  dockerfile = "extra/linux/docker/${os_name}/Dockerfile"
  args = {
    XX_VERSION = "test"

    DISTRIBUTION_NAME     = os_name
    DISTRIBUTION_VERSION  = build.os_version
    DISTRIBUTION_PACKAGES = join(" ", build.packages)
  }
  platforms = coalesce(build.platforms, platforms)
  matrix = {
    os_name = ["fedora"]
    build = [
      { os_version = "39", packages = distinct(concat(RHEL_FAMILY_PACKAGES, [])), platforms = null },
    ]
  }
}

target "cross-fedora" {
  inherits = ["fedora", "cross-package"]
}

variable "APK_FAMILY_PACKAGES" {
  default = [
    "make",
    "clang",
    "git",
    "lld",
    "build-base",
    "rustup",
    "openssl-libs-static",
    "libssh2-static",
    "libgit2-static",
    "fontconfig-static",
    "freetype-static",
    "zlib-static",
    "gcc",
    "zstd-static",
    "libxcb-static",
    "libxkbcommon-static",
    "vulkan-loader-dev",
  ]
}

target "alpine" {
  inherits   = ["binary"]
  name       = format("${os_name}-%s", replace(build.os_version, ".", "-"))
  dockerfile = "extra/linux/docker/${os_name}/Dockerfile"
  args = {
    DISTRIBUTION_NAME     = os_name
    DISTRIBUTION_VERSION  = build.os_version
    DISTRIBUTION_PACKAGES = join(" ", build.packages)
  }
  platforms = coalesce(build.platforms, platforms)
  matrix = {
    os_name = ["alpine"]
    build = [
      { os_version = "3.18", packages = distinct(concat(APK_FAMILY_PACKAGES, [])), platforms = null },
    ]
  }
}

target "cross-alpine" {
  inherits = ["alpine-3-18", "cross-binary"]
}
