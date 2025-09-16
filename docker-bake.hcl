variable "RUST_VERSION" {
  default = "1"
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
  }
}

variable "platforms" {
  default = [
    "linux/amd64",
    "linux/arm64",
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
  inherits   = [build.type]
  name       = "${os_name}-${build.os_version}-${build.type}"
  dockerfile = "extra/linux/docker/${os_name}/Dockerfile"
  args = {
    DISTRIBUTION_NAME     = os_name
    DISTRIBUTION_VERSION  = build.os_version
    DISTRIBUTION_PACKAGES = join(" ", coalesce(build.packages, DPKG_FAMILY_PACKAGES))
  }
  platforms = coalesce(build.platforms, platforms)
  matrix = {
    os_name = ["debian"]
    build = [
      { packages = null, platforms = null, type = "package", os_version = "bullseye" }, # 11
      { packages = null, platforms = null, type = "package", os_version = "bookworm" }, # 12
    ]
  }
}

target "cross-debian" {
  inherits = ["debian", "cross-package"]
}

target "ubuntu" {
  inherits   = [build.type]
  name       = "${os_name}-${build.os_version}-${build.type}"
  dockerfile = "extra/linux/docker/${os_name}/Dockerfile"
  args = {
    DISTRIBUTION_NAME     = os_name
    DISTRIBUTION_VERSION  = build.os_version
    DISTRIBUTION_PACKAGES = join(" ", coalesce(build.packages, DPKG_FAMILY_PACKAGES))
  }
  platforms = coalesce(build.platforms, platforms)
  matrix = {
    os_name = ["ubuntu"]
    build = [
      { packages = null, platforms = null, type = "package", os_version = "bionic"   }, # 18.04
      { packages = null, platforms = null, type = "package", os_version = "focal"    }, # 20.04
      { packages = null, platforms = null, type = "package", os_version = "jammy"    }, # 22.04
      { packages = null, platforms = null, type = "package", os_version = "noble"    }, # 24.04
      { packages = null, platforms = null, type = "package", os_version = "oracular" }, # 24.10
      { packages = null, platforms = null, type = "package", os_version = "plucky"   }, # 25.04
      # static binary, it looks ugly to define the target this way
      # but I don't have a better way to make it more friendly on CLI side without
      # more terrible code-wise way to implement it
      { packages = null, platforms = null, type = "binary", os_version = "focal"   }, # 20.04
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
  inherits   = [build.type]
  name       = "${name}-${build.version}-${build.type}"
  dockerfile = "extra/linux/docker/${name}/Dockerfile"
  args = {
    XX_VERSION = "test"

    DISTRIBUTION_NAME     = name
    DISTRIBUTION_VERSION  = build.version
    DISTRIBUTION_PACKAGES = join(" ", coalesce(build.packages, RHEL_FAMILY_PACKAGES))
  }
  // platforms = coalesce(build.platforms, platforms)
  platforms = ["linux/amd64"]
  matrix = {
    name = ["fedora"]
    build = [
      { packages = null, platforms = null, type = "package", version = "39" },
      { packages = null, platforms = null, type = "package", version = "40" },
      { packages = null, platforms = null, type = "package", version = "41" },
      { packages = null, platforms = null, type = "package", version = "42" },
      { packages = null, platforms = null, type = "package", version = "rawhide" },
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
    "openssl-dev",
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
    DISTRIBUTION_PACKAGES = join(" ", coalesce(build.packages, APK_FAMILY_PACKAGES))
  }
  platforms = coalesce(build.platforms, platforms)
  matrix = {
    os_name = ["alpine"]
    build = [
      { os_version = "",     packages = null, platforms = null },
      { os_version = "3.22", packages = null, platforms = null },
      { os_version = "3.20", packages = null, platforms = null },
      { os_version = "3.18", packages = null, platforms = null },
    ]
  }
}

target "cross-alpine" {
  inherits = ["alpine-3-20", "cross-binary"]
}

target "alpine-dev" {
  inherits = ["alpine-3-20"]
  target   = "dev"
  tags     = ["lapce/lapce:dev"]
  output   = ["type=docker"]
}
