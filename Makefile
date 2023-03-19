RELEASE_TAG_NAME = $(shell git describe --tags --match 'v*')

TARGET  = lapce
PROFILE = release-lto

STATIC = 0
LOCKED = 0
FROZEN = 1

ASSETS_DIR  = extra
BUILD_DIR   = target/$(CARGO_BUILD_TARGET)/$(PROFILE)
RELEASE_DIR = target/$(PROFILE)

VENDOR_NAME = $(TARGET)-vendor-$(RELEASE_TAG_NAME).tar.gz

vpath $(TARGET) $(RELEASE_DIR)

TARGET_ARCH = $(firstword $(subst -, ,$(CARGO_BUILD_TARGET)))
ifeq ($(CARGO_BUILD_TARGET),)
	TARGET_ARCH = $(subst host: ,,$(shell rustc -vV | grep host:))
endif

ifeq ($(STATIC),1)
	LIBZ_SYS_STATIC = 1
	LIBSSH2_STATIC  = 1
	LIBGIT2_STATIC  = 1
	OPENSSL_STATIC  = 1
	RUSTFLAGS       = "-C target-feature=+crt-static"
endif

ifeq ($(FROZEN),1)
	FROZEN = --frozen
	LOCKED =
endif
ifeq ($(LOCKED),1)
	FROZEN =
	LOCKED = --locked
endif

ifeq ($(OS),Windows_NT)
	PLATFORM = windows
endif
ifneq ($(OS),Windows_NT)
	UNAME_S := $(shell uname -s)
	ifeq ($(UNAME_S),Linux)
		PLATFORM = linux

		TAR_NAME        = Lapce-linux.tar.gz
		TAR_DIR         = $(RELEASE_DIR)/$(PLATFORM)

		DEB_NAME        = lapce.deb
		DEB_DIR         = $(RELEASE_DIR)/debian
		DEB_PACKAGE_DIR = $(DEB_DIR)/$(TARGET)

		RPM_NAME        = lapce.rpm
		RPM_DIR         = $(RELEASE_DIR)/fedora
		RPM_PACKAGE_DIR = $(RPM_DIR)/$(TARGET)
	endif
	ifeq ($(UNAME_S),Darwin)
		PLATFORM = macos

		CODESIGN_IDENTITY = FAC8FBEA99169DC1980731029648F110628D6A32

		MACOSX_DEPLOYMENT_TARGET = 10.11

		APP_NAME       = Lapce.app
		APP_TEMPLATE   = $(ASSETS_DIR)/macos/$(APP_NAME)
		APP_DIR        = $(RELEASE_DIR)/macos
		APP_BINARY     = $(RELEASE_DIR)/$(TARGET)
		APP_BINARY_DIR = $(APP_DIR)/$(APP_NAME)/Contents/MacOS
		APP_EXTRAS_DIR = $(APP_DIR)/$(APP_NAME)/Contents/Resources

		DMG_NAME = Lapce.dmg
		DMG_DIR  = $(RELEASE_DIR)/macos

		vpath $(APP_NAME) $(APP_DIR)
		vpath $(DMG_NAME) $(APP_DIR)
	endif

	UNAME_P := $(shell uname -p)
	ifeq ($(UNAME_P),x86_64)
		ARCH = amd64
	endif
	ifneq ($(filter %86,$(UNAME_P)),)
		ARCH = x86
	endif
	ifneq ($(filter arm%,$(UNAME_P)),)
		ARCH = $(UNAME_P)
	endif
endif

all: help

help: ## Print this help message
	@grep -E '^[a-zA-Z._-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

release-dir:
	@mkdir -p $(RELEASE_DIR)
	@echo $(RELEASE_DIR)

dependencies: $(ID)-dependencies ## Install OS dependencies required to build Lapce
$(ID)-dependencies: $(ID)$(VERSION_ID)-dependencies
windows-dependencies:
	@echo "Installing Windows dependencies"
	@nuget install WiX
fedora-dependencies:
	@echo "Installing Fedora dependencies"
	@dnf install \
		gcc-c++ perl-FindBin perl-File-Compare gtk3-devel
ubuntu-dependencies:
	@echo "Installing Ubuntu dependencies"
	@apt-get -y update
	@apt-get -y install \
		cmake pkg-config libfontconfig-dev libgtk-3-dev g++

rustup: ## Update/install rustup
	@rustup update || $(MAKE) rustup-install

rustup-install:
	@curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable

fetch: ## Fetch Rust dependencies
	@echo "Fetching dependencies"
	@cargo fetch --locked

vendor: fetch ## Create vendor tarball
	@echo "Creating $(VENDOR_NAME)"
	@cargo vendor --frozen
	@tar -zcvf ./$(RELEASE_DIR)/$(VENDOR_NAME) ./vendor/
	@echo "Created '$(VENDOR_NAME)' in '$(RELEASE_DIR)'"

check: ## Run check
	@echo "Checking codebase"
	@cargo check $(LOCKED) $(FROZEN) --workspace
	@cargo clippy $(LOCKED) $(FROZEN) --workspace

test: ## Run tests
	@echo "Running tests"
	@cargo test $(LOCKED) $(FROZEN) --workspace

build: $(TARGET)-build ## Build all executables
$(TARGET)-build: fetch
	cargo build $(LOCKED) $(FROZEN) --profile $(PROFILE) --bin $(TARGET)

tarball: $(TARGET)-tarball ## Create tarball
$(TARGET)-tarball: release-dir $(TARGET)-build
	@echo "Creating tarball"
	@mkdir -p $(TAR_DIR)/Lapce
	@cp $(RELEASE_DIR)/$(TARGET) $(TAR_DIR)/Lapce
	@tar -zcvf $(TAR_DIR)/$(TAR_NAME) $(TAR_DIR)/Lapce
	@rm -rf $(TAR_DIR)/Lapce
	@echo "Created '$(TAR_NAME)' in '$(TAR_DIR)'"

gz: $(TARGET)-gz ## Create gzipped binary
$(TARGET)-gz: release-dir $(TARGET)-build
	@echo "Creating gzipped binary"
	@gzip -c $(RELEASE_DIR)/$(CARGO_BUILD_TARGET)/$(TARGET) > $(RELEASE_DIR)/$(PLATFORM)/$(TARGET)-$(PLATFORM)-$(TARGET_ARCH)
	@echo "Created '$(TARGET)-$(PLATFORM)-$(TARGET_ARCH)' in '$(RELEASE_DIR)/$(PLATFORM)'"

### macOS

binary: $(TARGET)-native ## Build a macOS release binary
binary-universal: $(TARGET)-universal ## Build a macOS universal release binary
$(TARGET)-native: fetch
	cargo build $(LOCKED) $(FROZEN) --profile $(PROFILE)
	@lipo $(BUILD_DIR)/$(TARGET) -create -output $(APP_BINARY)
$(TARGET)-universal: fetch
	cargo build $(LOCKED) $(FROZEN) --profile $(PROFILE) --target=x86_64-apple-darwin
	cargo build $(LOCKED) $(FROZEN) --profile $(PROFILE) --target=aarch64-apple-darwin
	@lipo target/{x86_64,aarch64}-apple-darwin/$(PROFILE)/$(TARGET) -create -output $(APP_BINARY)
	/usr/bin/codesign -vvv --deep --entitlements $(ASSETS_DIR)/entitlements.plist --strict --options=runtime --force -s $(CODESIGN_IDENTITY) $(APP_BINARY)

app: $(APP_NAME)-native ## Create a Lapce.app
app-universal: $(APP_NAME)-universal ## Create a universal Lapce.app
$(APP_NAME)-%: $(TARGET)-%
	@mkdir -p $(APP_BINARY_DIR) $(APP_EXTRAS_DIR)
	@cp -fRp $(APP_TEMPLATE) $(APP_DIR)
	@cp -fp $(APP_BINARY) $(APP_BINARY_DIR)
	@touch -r "$(APP_BINARY)" "$(APP_DIR)/$(APP_NAME)"
	@echo "Created '$(APP_NAME)' in '$(APP_DIR)'"
	xattr -c $(APP_DIR)/$(APP_NAME)/Contents/Info.plist
	xattr -c $(APP_DIR)/$(APP_NAME)/Contents/Resources/lapce.icns
	/usr/bin/codesign -vvv --deep  --entitlements $(ASSETS_DIR)/entitlements.plist --strict --options=runtime --force -s $(CODESIGN_IDENTITY) $(APP_DIR)/$(APP_NAME)

dmg: $(DMG_NAME)-native ## Create a Lapce.dmg
dmg-universal: $(DMG_NAME)-universal ## Create a universal Lapce.dmg
$(DMG_NAME)-%: $(APP_NAME)-%
	@echo "Packing disk image..."
	@ln -sf /Applications $(DMG_DIR)/Applications
	@hdiutil create $(DMG_DIR)/$(DMG_NAME) \
		-volname "Lapce" \
		-fs HFS+ \
		-srcfolder $(APP_DIR) \
		-ov -format UDZO
	@echo "Packed '$(APP_NAME)' in '$(APP_DIR)'"
	/usr/bin/codesign -vvv --deep  --entitlements $(ASSETS_DIR)/entitlements.plist --strict --options=runtime --force -s $(CODESIGN_IDENTITY) $(DMG_DIR)/$(DMG_NAME)

### Debian

deb: $(TARGET)-deb ## Create lapce.deb
$(TARGET)-deb: $(TARGET)-build
	@echo "Creating lapce.deb"
	@mkdir -p $(DEB_PACKAGE_DIR)/DEBIAN $(DEB_PACKAGE_DIR)/usr/bin
	@cp $(ASSETS_DIR)/linux/debian/control $(DEB_PACKAGE_DIR)/DEBIAN/control
	sed -i "s/%NAME%/$(TARGET)/g" $(DEB_PACKAGE_DIR)/DEBIAN/control
	sed -i "s/%ARCHITECTURE%/$(ARCH)/g" $(DEB_PACKAGE_DIR)/DEBIAN/control
	sed -i "s/%VERSION%/$(subst v,,$(RELEASE_TAG_NAME))/g" $(DEB_PACKAGE_DIR)/DEBIAN/control
	@cp $(BUILD_DIR)/$(TARGET) $(DEB_PACKAGE_DIR)/usr/bin/$(TARGET)
	@dpkg-deb --build $(DEB_PACKAGE_DIR) $(RELEASE_DIR)/$(PLATFORM)/$(DEB_NAME)
	@echo "Built '$(DEB_NAME)' in '$(RELEASE_DIR)/$(PLATFORM)'"

### Fedora

rpm: $(TARGET)-rpm ## Create lapce.rpm
$(TARGET)-rpm: $(TARGET)-build
	@echo "Creating lapce.rpm"

install: $(PLATFORM)-install ## Install app
install-native: $(PLATFORM)-install-native ## Mount disk image
install-universal: $(PLATFORM)-install-native ## Mount universal disk image

darwin-install: $(PLATFORM)-install-native
darwin-install-universal: $(PLATFORM)-install-native
darwin-install-native: $(DMG_NAME)-%
	@open $(DMG_DIR)/$(DMG_NAME)

linux-install:
	@cp $(RELEASE_DIR)/$(TARGET) /usr/local/bin/$(TARGET)

.PHONY: build app binary clean dmg deb rpm install $(TARGET) $(TARGET)-universal $(TARGET)-build

clean: ## Remove all build artifacts
	@cargo clean
