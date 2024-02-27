variable "RUST_VERSION" {
  default = "1.76"
}

variable "PACKAGE_NAME" {
  default = RELEASE_TAG_NAME == "nightly" ? "lapce-nightly" : "lapce"
}

variable "RELEASE_TAG_NAME" {

}

variable "XX_VERSION" {
  default = "latest"
}

target "_common" {
  context   = "."
  platforms = ["local"]
  output    = ["target"]
  args = {
    PACKAGE_NAME     = PACKAGE_NAME
    RELEASE_TAG_NAME = RELEASE_TAG_NAME

    RUST_VERSION = RUST_VERSION

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
}

target "cross-binary" {
  inherits = ["binary", "_platforms"]
  target   = "cross-binary"
}

target "package" {
  inherits = ["_common"]
  target   = "package"
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
