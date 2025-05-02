# This Makefile is intended *only* for building macOS binaries of Lapce.
# It uses macOS-specific tools like `lipo`, `codesign`, and `hdiutil`,
# and requires that a valid Apple Developer signing identity is installed
# and available in the system Keychain under the fingerprint set in 
# CODESIGN_IDENTITY.
#
# See `docs/building-from-source.md`.

TARGET = lapce

CODESIGN_IDENTITY = FAC8FBEA99169DC1980731029648F110628D6A32

ASSETS_DIR = extra
RELEASE_DIR = target/release-lto

APP_NAME = Lapce.app
APP_TEMPLATE = $(ASSETS_DIR)/macos/$(APP_NAME)
APP_DIR = $(RELEASE_DIR)/macos
APP_BINARY = $(RELEASE_DIR)/$(TARGET)
APP_BINARY_DIR = $(APP_DIR)/$(APP_NAME)/Contents/MacOS
APP_EXTRAS_DIR = $(APP_DIR)/$(APP_NAME)/Contents/Resources

DMG_NAME = Lapce.dmg
DMG_DIR = $(RELEASE_DIR)/macos

vpath $(TARGET) $(RELEASE_DIR)
vpath $(APP_NAME) $(APP_DIR)
vpath $(DMG_NAME) $(APP_DIR)

all: help

help: ## Print this help message
	@grep -E '^[a-zA-Z._-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

ubuntu-deps:
	apt-get update -y
	apt-get install -y clang libxkbcommon-x11-dev pkg-config libvulkan-dev libgtk-3-dev libwayland-dev xorg-dev libxcb-shape0-dev libxcb-xfixes0-dev

binary: $(TARGET)-native ## Build a release binary
binary-universal: $(TARGET)-universal ## Build a universal release binary
$(TARGET)-native:
	MACOSX_DEPLOYMENT_TARGET="10.11" cargo build --profile release-lto
	@lipo target/release-lto/$(TARGET) -create -output $(APP_BINARY)
$(TARGET)-universal:
	MACOSX_DEPLOYMENT_TARGET="10.11" cargo build --profile release-lto --target=x86_64-apple-darwin
	MACOSX_DEPLOYMENT_TARGET="10.11" cargo build --profile release-lto --target=aarch64-apple-darwin
	@lipo target/{x86_64,aarch64}-apple-darwin/release-lto/$(TARGET) -create -output $(APP_BINARY)
	/usr/bin/codesign -vvv --deep --entitlements $(ASSETS_DIR)/entitlements.plist --strict --options=runtime --force -s $(CODESIGN_IDENTITY) $(APP_BINARY)

app: $(APP_NAME)-native ## Create a Lapce.app
app-universal: $(APP_NAME)-universal ## Create a universal Lapce.app
$(APP_NAME)-%: $(TARGET)-%
	@mkdir -p $(APP_BINARY_DIR)
	@mkdir -p $(APP_EXTRAS_DIR)
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

install: $(INSTALL)-native ## Mount disk image
install-universal: $(INSTALL)-native ## Mount universal disk image
$(INSTALL)-%: $(DMG_NAME)-%
	@open $(DMG_DIR)/$(DMG_NAME)

.PHONY: app binary clean dmg install $(TARGET) $(TARGET)-universal

clean: ## Remove all build artifacts
	@cargo clean
