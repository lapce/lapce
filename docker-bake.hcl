variable "RUST_VERSION" {
  default = "1"
}

variable "PACKAGE_NAME" {
  default = "lapce-nightly"
}

variable "PACKAGE_VERSION" {
  default = "nightly"
}

variable "RELEASE_TAG_NAME" {
  default = ""
}

variable "XX_VERSION" {
  default = "master"
}

target "_common" {
  output = ["target/"]
  args = {
    PACKAGE_NAME    = PACKAGE_NAME
    PACKAGE_VERSION = PACKAGE_VERSION

    RUST_VERSION = RUST_VERSION

    RELEASE_TAG_NAME = RELEASE_TAG_NAME

    BUILDKIT_CONTEXT_KEEP_GIT_DIR = 1

    OUTPUT_DIR = "/output"
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
  inherits  = ["_common"]
  target    = "binary"
  platforms = ["local"]
  output    = ["target"]
}

target "cross-binary" {
  inherits = ["binary", "_platforms"]
}

target "package" {
  inherits  = ["_common"]
  target    = "package"
  platforms = ["local"]
  output    = ["target"]
}

target "cross-package" {
  inherits = ["package", "_platforms"]
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
  inherits   = ["cross-package"]
  name       = "${os_name}-${build.os_version}"
  target     = "cross-package"
  context    = "."
  dockerfile = "extra/linux/docker/${os_name}/Dockerfile"
  args = {
    DISTRIBUTION_NAME     = os_name
    DISTRIBUTION_VERSION  = build.os_version
    DISTRIBUTION_PACKAGES = join(" ", coalesce(build.packages, DPKG_FAMILY_PACKAGES))
  }
  matrix = {
    os_name = ["debian"]
    build = [
      { packages = null, os_version = "bullseye" }, # 11
      { packages = null, os_version = "bookworm" }, # 12
    ]
  }
}

target "ubuntu" {
  inherits   = ["cross-package"]
  name       = "${os_name}-${build.os_version}"
  target     = "cross-package"
  context    = "."
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
      { packages = null, platforms = null,            os_version = "bionic"  }, # 18.04
      { packages = null, platforms = null,            os_version = "focal"   }, # 20.04
      { packages = null, platforms = ["linux/amd64"], os_version = "jammy"   }, # 22.04
      { packages = null, platforms = null,            os_version = "kinetic" }, # 22.10
      { packages = null, platforms = null,            os_version = "lunar"   }, # 23.04
      { packages = null, platforms = null,            os_version = "mantic"  }, # 23.10
      { packages = null, platforms = null,            os_version = "noble"   }, # 24.04
    ]
  }
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
  inherits   = ["cross-package"]
  name       = "${os_name}-${build.os_version}"
  target     = "cross-package"
  context    = "."
  dockerfile = "extra/linux/docker/${os_name}/Dockerfile"
  args = {
    XX_VERSION = "test"

    DISTRIBUTION_NAME     = os_name
    DISTRIBUTION_VERSION  = build.os_version
    DISTRIBUTION_PACKAGES = join(" ", coalesce(build.packages, RHEL_FAMILY_PACKAGES))
  }
  platforms = coalesce(build.platforms, platforms)
  matrix = {
    os_name = ["fedora"]
    build = [
      { os_version = "39",      packages = null, platforms = null },
      { os_version = "rawhide", packages = null, platforms = null },
    ]
  }
}
