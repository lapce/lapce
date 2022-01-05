TARGET = lapce

ASSETS_DIR = extra
RELEASE_DIR = target/release

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
	@grep -E '^[a-zA-Z._-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

binary: $(TARGET)-native ## Build a release binary
binary-universal: $(TARGET)-universal ## Build a universal release binary
$(TARGET)-native:
	MACOSX_DEPLOYMENT_TARGET="10.11" cargo build --release
	@lipo target/release/$(TARGET) -create -output $(APP_BINARY)
	@lipo target/release/$(TARGET)-proxy -create -output $(APP_BINARY)-proxy
$(TARGET)-universal:
	MACOSX_DEPLOYMENT_TARGET="10.11" cargo build --release --target=x86_64-apple-darwin
	MACOSX_DEPLOYMENT_TARGET="10.11" cargo build --release --target=aarch64-apple-darwin
	@lipo target/{x86_64,aarch64}-apple-darwin/release/$(TARGET) -create -output $(APP_BINARY)
	@lipo target/{x86_64,aarch64}-apple-darwin/release/$(TARGET)-proxy -create -output $(APP_BINARY)-proxy
	/usr/bin/codesign -vvv --deep --strict --options=runtime --force -s FAC8FBEA99169DC1980731029648F110628D6A32 $(APP_BINARY)
	/usr/bin/codesign -vvv --deep --strict --options=runtime --force -s FAC8FBEA99169DC1980731029648F110628D6A32 $(APP_BINARY)-proxy

app: $(APP_NAME)-native ## Create a Lapce.app
app-universal: $(APP_NAME)-universal ## Create a universal Lapce.app
$(APP_NAME)-%: $(TARGET)-%
	@mkdir -p $(APP_BINARY_DIR)
	@mkdir -p $(APP_EXTRAS_DIR)
	@cp -fRp $(APP_TEMPLATE) $(APP_DIR)
	@cp -fp $(APP_BINARY) $(APP_BINARY_DIR)
	@cp -fp $(APP_BINARY)-proxy $(APP_BINARY_DIR)
	@touch -r "$(APP_BINARY)" "$(APP_DIR)/$(APP_NAME)"
	@echo "Created '$(APP_NAME)' in '$(APP_DIR)'"
	xattr -c $(APP_DIR)/$(APP_NAME)/Contents/Info.plist
	xattr -c $(APP_DIR)/$(APP_NAME)/Contents/Resources/lapce.icns
	/usr/bin/codesign -vvv --deep --strict --options=runtime --force -s FAC8FBEA99169DC1980731029648F110628D6A32 $(APP_DIR)/$(APP_NAME)

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
	/usr/bin/codesign -vvv --deep --strict --options=runtime --force -s FAC8FBEA99169DC1980731029648F110628D6A32 $(DMG_DIR)/$(DMG_NAME)

install: $(INSTALL)-native ## Mount disk image
install-universal: $(INSTALL)-native ## Mount universal disk image
$(INSTALL)-%: $(DMG_NAME)-%
	@open $(DMG_DIR)/$(DMG_NAME)

.PHONY: app binary clean dmg install $(TARGET) $(TARGET)-universal

clean: ## Remove all build artifacts
	@cargo clean
