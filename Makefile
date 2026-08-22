.DEFAULT_GOAL := help

APP_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
WORKSPACE_DIR := $(abspath $(APP_DIR)/..)
DAEMON_DIR := $(WORKSPACE_DIR)/corbit-daemon
ARTIFACTS_DIR ?= $(WORKSPACE_DIR)/artifacts
CARGO_TARGET_DIR ?= $(APP_DIR)/target
CARGO ?= cargo
RUSTUP ?= rustup
NODE ?= node
MODE ?= release
MACOS_SIGN_IDENTITY ?= -

HOST_PLATFORM := $(shell $(NODE) -p "({darwin:'macos',linux:'linux',win32:'windows'})[process.platform] || process.platform" 2>/dev/null)
HOST_ARCH := $(shell $(NODE) -p "({arm64:'arm64',x64:'x64'})[process.arch] || process.arch" 2>/dev/null)
PLATFORM ?= $(HOST_PLATFORM)
ARCH ?= $(HOST_ARCH)

RUST_TARGET_macos_arm64 := aarch64-apple-darwin
RUST_TARGET_macos_x64 := x86_64-apple-darwin
RUST_TARGET_linux_arm64 := aarch64-unknown-linux-gnu
RUST_TARGET_linux_x64 := x86_64-unknown-linux-gnu
RUST_TARGET_windows_arm64 := aarch64-pc-windows-msvc
RUST_TARGET_windows_x64 := x86_64-pc-windows-msvc
RUST_TARGET := $(RUST_TARGET_$(PLATFORM)_$(ARCH))

ifeq ($(MODE),release)
CARGO_PROFILE_FLAG := --release
CARGO_PROFILE_DIR := release
else ifeq ($(MODE),debug)
CARGO_PROFILE_FLAG :=
CARGO_PROFILE_DIR := debug
else
CARGO_PROFILE_FLAG := __invalid_mode__
CARGO_PROFILE_DIR := __invalid_mode__
endif

ifeq ($(PLATFORM),windows)
BINARY_EXT := .exe
else
BINARY_EXT :=
endif

VERSION ?= $(shell sed -n 's/^version = "\([^"]*\)"/\1/p' "$(APP_DIR)/Cargo.toml" | head -1)
DAEMON_VERSION ?= $(shell sed -n 's/.*"version": "\([^"]*\)".*/\1/p' "$(DAEMON_DIR)/package.json" | head -1)
BINARY_PATH := $(CARGO_TARGET_DIR)/$(RUST_TARGET)/$(CARGO_PROFILE_DIR)/corbit-app$(BINARY_EXT)
DEV_BINARY_PATH ?= $(CARGO_TARGET_DIR)/debug/corbit-app$(BINARY_EXT)
DEV_PID_FILE ?= $(CARGO_TARGET_DIR)/corbit-app-dev.pid
DEV_ARTIFACTS_DIR ?= $(CARGO_TARGET_DIR)/dev-artifacts
DEV_APP_NAME ?= Corbit Dev
DEV_BUNDLE_IDENTIFIER ?= com.xiaoxi.corbit.desktop.dev
DEV_LAUNCH_BINARY := $(DEV_BINARY_PATH)

ifeq ($(HOST_PLATFORM),macos)
DEV_RUST_TARGET := $(RUST_TARGET_macos_$(HOST_ARCH))
DEV_APP_DIR := $(DEV_ARTIFACTS_DIR)/desktop/macos-$(HOST_ARCH)/$(DEV_APP_NAME).app
DEV_LAUNCH_BINARY := $(DEV_APP_DIR)/Contents/MacOS/corbit
endif

.PHONY: help doctor validate rust-target binary daemon-runtime package daemon-smoke build dev dev-verify dev-stop dev-restart run test lint check \
	macos-arm64 macos-x64 macos-universal linux-arm64 linux-x64 \
	windows-arm64 windows-x64

help:
	@echo "Corbit GPUI desktop build"
	@echo ""
	@echo "  make build PLATFORM=macos ARCH=arm64 MODE=release"
	@echo "  make dev                           Build and replace the tracked debug client"
	@echo "  make dev-verify                    Verify the generated debug app identity"
	@echo "  make daemon-smoke                  Package and start an isolated Daemon runtime"
	@echo "  make dev-stop                      Stop the tracked debug client"
	@echo "  make dev-restart                   Rebuild and restart the debug client"
	@echo "  make rust-target PLATFORM=macos ARCH=x64"
	@echo "  make macos-universal"
	@echo "  make check"
	@echo ""
	@echo "Supported: macos/{arm64,x64}, linux/{arm64,x64}, windows/{arm64,x64}"

doctor:
	@echo "[desktop] host=$(HOST_PLATFORM)/$(HOST_ARCH) target=$(PLATFORM)/$(ARCH)"
	@echo "[desktop] rust-target=$(RUST_TARGET) mode=$(MODE)"
	@$(CARGO) --version
	@$(RUSTUP) show active-toolchain

validate:
	@$(NODE) -e "const target=process.argv[1], mode=process.argv[2]; if (!target) { console.error('Unsupported desktop target: $(PLATFORM)/$(ARCH)'); process.exit(2); } if (!['release','debug'].includes(mode)) { console.error('MODE must be release or debug'); process.exit(2); }" "$(RUST_TARGET)" "$(MODE)"

rust-target: validate
	@$(RUSTUP) target add "$(RUST_TARGET)"

binary: validate
	@CORBIT_DAEMON_VERSION="$(DAEMON_VERSION)" $(CARGO) build --locked --manifest-path "$(APP_DIR)/Cargo.toml" \
		--package corbit-app --target "$(RUST_TARGET)" $(CARGO_PROFILE_FLAG) $(CARGO_FLAGS)

daemon-runtime: validate
	@test -d "$(DAEMON_DIR)" || (echo "Missing Corbit Daemon checkout: $(DAEMON_DIR)"; exit 2)
	@$(MAKE) --no-print-directory -C "$(DAEMON_DIR)" package \
		PLATFORM="$(PLATFORM)" ARCH="$(ARCH)" ARTIFACTS_DIR="$(ARTIFACTS_DIR)"

package: binary daemon-runtime
	@$(NODE) "$(APP_DIR)/scripts/package-desktop.mjs" \
		--platform "$(PLATFORM)" --arch "$(ARCH)" \
		--rust-target "$(RUST_TARGET)" --profile "$(CARGO_PROFILE_DIR)" \
		--version "$(VERSION)" --binary "$(BINARY_PATH)" \
		--assets "$(APP_DIR)/assets/brand" --output "$(ARTIFACTS_DIR)" \
		--daemon-runtime "$(ARTIFACTS_DIR)/daemon/$(PLATFORM)-$(ARCH)/corbit-daemon" \
		--daemon-version "$(DAEMON_VERSION)" \
		--sign-identity "$(MACOS_SIGN_IDENTITY)"
daemon-smoke: package
	@$(NODE) "$(APP_DIR)/scripts/smoke-daemon-runtime.mjs" \
		--runtime "$(ARTIFACTS_DIR)/daemon/$(PLATFORM)-$(ARCH)/corbit-daemon" \
		--version "$(DAEMON_VERSION)" --platform "$(PLATFORM)" --arch "$(ARCH)" \
		--node "$(shell command -v $(NODE))"

build: daemon-smoke

dev:
	@$(MAKE) --no-print-directory daemon-runtime PLATFORM="$(HOST_PLATFORM)" ARCH="$(HOST_ARCH)"
	@CORBIT_BUILD_CHANNEL=dev CORBIT_DAEMON_VERSION="$(DAEMON_VERSION)" CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --locked \
		--manifest-path "$(APP_DIR)/Cargo.toml" --package corbit-app $(CARGO_FLAGS)
ifeq ($(HOST_PLATFORM),macos)
	@$(NODE) "$(APP_DIR)/scripts/package-desktop.mjs" \
		--platform macos --arch "$(HOST_ARCH)" \
		--rust-target "$(DEV_RUST_TARGET)" --profile debug \
		--version "$(VERSION)" --binary "$(DEV_BINARY_PATH)" \
		--assets "$(APP_DIR)/assets/brand" --output "$(DEV_ARTIFACTS_DIR)" \
		--app-name "$(DEV_APP_NAME)" --bundle-identifier "$(DEV_BUNDLE_IDENTIFIER)" \
		--daemon-runtime "$(ARTIFACTS_DIR)/daemon/$(HOST_PLATFORM)-$(HOST_ARCH)/corbit-daemon" \
		--daemon-version "$(DAEMON_VERSION)" \
		--sign-identity "$(MACOS_SIGN_IDENTITY)"
	@$(MAKE) --no-print-directory dev-verify
endif
	@$(NODE) "$(APP_DIR)/scripts/dev.mjs" --action run \
		--binary "$(DEV_BINARY_PATH)" --launch-binary "$(DEV_LAUNCH_BINARY)" \
		--pid-file "$(DEV_PID_FILE)"

dev-verify:
ifeq ($(HOST_PLATFORM),macos)
	@$(NODE) "$(APP_DIR)/scripts/verify-desktop-bundle.mjs" \
		--app "$(DEV_APP_DIR)" --name "$(DEV_APP_NAME)" \
		--bundle-identifier "$(DEV_BUNDLE_IDENTIFIER)" --version "$(VERSION)" \
		--daemon-version "$(DAEMON_VERSION)" --arch "$(HOST_ARCH)"
	@$(NODE) "$(APP_DIR)/scripts/smoke-daemon-runtime.mjs" \
		--runtime "$(DEV_APP_DIR)/Contents/Resources/corbit-daemon" \
		--version "$(DAEMON_VERSION)" --platform macos --arch "$(HOST_ARCH)" \
		--node "$(shell command -v $(NODE))"
else
	@echo "dev-verify is only available for macOS app bundles"
endif

dev-stop:
	@$(NODE) "$(APP_DIR)/scripts/dev.mjs" --action stop \
		--binary "$(DEV_BINARY_PATH)" --pid-file "$(DEV_PID_FILE)"

dev-restart: dev

run: dev

test:
	@$(CARGO) test --locked --manifest-path "$(APP_DIR)/Cargo.toml" --workspace

lint:
	@$(CARGO) fmt --manifest-path "$(APP_DIR)/Cargo.toml" --all -- --check
	@$(CARGO) clippy --locked --manifest-path "$(APP_DIR)/Cargo.toml" \
		--workspace --all-targets -- -D warnings

check: lint test

macos-arm64:
	@$(MAKE) --no-print-directory build PLATFORM=macos ARCH=arm64 MODE="$(MODE)"

macos-x64:
	@$(MAKE) --no-print-directory build PLATFORM=macos ARCH=x64 MODE="$(MODE)"

linux-arm64:
	@$(MAKE) --no-print-directory build PLATFORM=linux ARCH=arm64 MODE="$(MODE)"

linux-x64:
	@$(MAKE) --no-print-directory build PLATFORM=linux ARCH=x64 MODE="$(MODE)"

windows-arm64:
	@$(MAKE) --no-print-directory build PLATFORM=windows ARCH=arm64 MODE="$(MODE)"

windows-x64:
	@$(MAKE) --no-print-directory build PLATFORM=windows ARCH=x64 MODE="$(MODE)"

macos-universal:
	@$(NODE) -e "if (process.platform !== 'darwin') { console.error('macOS universal builds require macOS'); process.exit(2); }"
	@$(MAKE) --no-print-directory binary PLATFORM=macos ARCH=arm64 MODE="$(MODE)"
	@$(MAKE) --no-print-directory binary PLATFORM=macos ARCH=x64 MODE="$(MODE)"
	@mkdir -p "$(CARGO_TARGET_DIR)/universal-apple-darwin/$(CARGO_PROFILE_DIR)"
	@lipo -create \
		"$(CARGO_TARGET_DIR)/aarch64-apple-darwin/$(CARGO_PROFILE_DIR)/corbit-app" \
		"$(CARGO_TARGET_DIR)/x86_64-apple-darwin/$(CARGO_PROFILE_DIR)/corbit-app" \
		-output "$(CARGO_TARGET_DIR)/universal-apple-darwin/$(CARGO_PROFILE_DIR)/corbit-app"
	@$(NODE) "$(APP_DIR)/scripts/package-desktop.mjs" \
		--platform macos --arch universal --rust-target universal-apple-darwin \
		--profile "$(CARGO_PROFILE_DIR)" --version "$(VERSION)" \
		--binary "$(CARGO_TARGET_DIR)/universal-apple-darwin/$(CARGO_PROFILE_DIR)/corbit-app" \
		--assets "$(APP_DIR)/assets/brand" --output "$(ARTIFACTS_DIR)" \
		--daemon-runtime-arm64 "$(ARTIFACTS_DIR)/daemon/macos-arm64/corbit-daemon" \
		--daemon-runtime-x64 "$(ARTIFACTS_DIR)/daemon/macos-x64/corbit-daemon" \
		--daemon-version "$(DAEMON_VERSION)" \
		--sign-identity "$(MACOS_SIGN_IDENTITY)"
	@$(NODE) "$(APP_DIR)/scripts/smoke-daemon-runtime.mjs" \
		--runtime "$(ARTIFACTS_DIR)/desktop/macos-universal/Corbit.app/Contents/Resources/corbit-daemon/$(HOST_ARCH)" \
		--version "$(DAEMON_VERSION)" --platform macos --arch "$(HOST_ARCH)" \
		--node "$(shell command -v $(NODE))"
