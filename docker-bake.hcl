variable "RUST_VERSION" {
  default = "1.75"
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
  default = "latest"
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

target "ubuntu" {
  inherits   = ["cross-package"]
  name       = "${os_name}-${build.os_version}"
  target     = "cross-package"
  context    = "."
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
      { os_version = "bionic", packages = distinct(concat(DPKG_FAMILY_PACKAGES, [])), platforms = null },           # 18.04
      { os_version = "focal", packages = distinct(concat(DPKG_FAMILY_PACKAGES, [])), platforms = null },            # 20.04
      { os_version = "jammy", packages = distinct(concat(DPKG_FAMILY_PACKAGES, [])), platforms = ["linux/amd64"] }, # 22.04
      { os_version = "kinetic", packages = distinct(concat(DPKG_FAMILY_PACKAGES, [])), platforms = null },          # 22.10
      { os_version = "lunar", packages = distinct(concat(DPKG_FAMILY_PACKAGES, [])), platforms = null },            # 23.04
      { os_version = "mantic", packages = distinct(concat(DPKG_FAMILY_PACKAGES, [])), platforms = null },           # 23.10
      { os_version = "noble", packages = distinct(concat(DPKG_FAMILY_PACKAGES, [])), platforms = null },            # 24.04
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
