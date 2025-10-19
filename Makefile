# Doorcam Makefile
# Provides convenient targets for building, testing, and packaging

.PHONY: help build build-release build-pi build-pi64 test clean install uninstall package lint format check-deps

# Default target
help:
	@echo "Doorcam Build System"
	@echo ""
	@echo "Available targets:"
	@echo "  build          - Build debug version for current platform"
	@echo "  build-release  - Build release version for current platform"
	@echo "  build-pi       - Cross-compile for Raspberry Pi (ARMv7)"
	@echo "  build-pi64     - Cross-compile for Raspberry Pi 4/5 (ARM64)"
	@echo "  test           - Run tests"
	@echo "  clean          - Clean build artifacts"
	@echo "  install        - Install to system (requires sudo)"
	@echo "  uninstall      - Remove from system (requires sudo)"
	@echo "  package        - Create distribution packages"
	@echo "  lint           - Run clippy linter"
	@echo "  format         - Format code with rustfmt"
	@echo "  check-deps     - Check system dependencies"
	@echo ""
	@echo "Environment variables:"
	@echo "  FEATURES       - Comma-separated list of features to enable"
	@echo "  INSTALL_PREFIX - Installation prefix (default: /usr/local)"
	@echo ""
	@echo "Examples:"
	@echo "  make build-release FEATURES=camera,motion_analysis,streaming,display"
	@echo "  make install INSTALL_PREFIX=/opt/doorcam"
	@echo "  make package"

# Configuration
PROJECT_NAME := doorcam
FEATURES ?= camera,motion_analysis,streaming,display
INSTALL_PREFIX ?= /usr/local

# Build targets
build:
	@echo "Building debug version..."
	cargo build --features $(FEATURES)

build-release:
	@echo "Building release version..."
	cargo build --release --features $(FEATURES)

build-pi:
	@echo "Cross-compiling for Raspberry Pi (ARMv7)..."
	./scripts/build.sh --release --features $(FEATURES) armv7-pi

build-pi64:
	@echo "Cross-compiling for Raspberry Pi 4/5 (ARM64)..."
	./scripts/build.sh --release --features $(FEATURES) aarch64-pi

# Test targets
test:
	@echo "Running tests..."
	cargo test --features $(FEATURES)

test-release:
	@echo "Running tests in release mode..."
	cargo test --release --features $(FEATURES)

# Clean target
clean:
	@echo "Cleaning build artifacts..."
	cargo clean
	rm -rf dist/

# Installation targets
install: build-release
	@echo "Installing $(PROJECT_NAME)..."
	@if [ ! -f "target/release/$(PROJECT_NAME)" ]; then \
		echo "Error: Release binary not found. Run 'make build-release' first."; \
		exit 1; \
	fi
	cp target/release/$(PROJECT_NAME) .
	sudo ./scripts/install.sh --prefix $(INSTALL_PREFIX)
	rm -f $(PROJECT_NAME)

uninstall:
	@echo "Uninstalling $(PROJECT_NAME)..."
	sudo ./scripts/uninstall.sh --prefix $(INSTALL_PREFIX)

# Package targets
package: build-release
	@echo "Creating distribution package..."
	./scripts/build.sh --release --package --features $(FEATURES) native

package-pi: build-pi
	@echo "Creating Raspberry Pi package..."
	./scripts/build.sh --release --package --features $(FEATURES) armv7-pi

package-pi64: build-pi64
	@echo "Creating Raspberry Pi 4/5 package..."
	./scripts/build.sh --release --package --features $(FEATURES) aarch64-pi

package-all: package package-pi package-pi64
	@echo "All packages created in dist/"

# Development targets
lint:
	@echo "Running clippy linter..."
	cargo clippy --features $(FEATURES) -- -D warnings

format:
	@echo "Formatting code..."
	cargo fmt

format-check:
	@echo "Checking code formatting..."
	cargo fmt -- --check

# Dependency checking
check-deps:
	@echo "Checking system dependencies..."
	@echo "Checking Rust toolchain..."
	@rustc --version || (echo "Error: Rust not installed" && exit 1)
	@cargo --version || (echo "Error: Cargo not installed" && exit 1)
	@echo "Checking pkg-config..."
	@pkg-config --version || (echo "Warning: pkg-config not found")
	@echo "Checking V4L2 libraries..."
	@pkg-config --exists libv4l2 || echo "Warning: libv4l2 not found"
	@echo "Checking OpenCV libraries..."
	@pkg-config --exists opencv4 || pkg-config --exists opencv || echo "Warning: OpenCV not found"
	@echo "Checking udev libraries..."
	@pkg-config --exists libudev || echo "Warning: libudev not found"
	@echo "Dependency check completed"

# Cross-compilation setup
setup-cross-armv7:
	@echo "Setting up ARMv7 cross-compilation..."
	rustup target add armv7-unknown-linux-gnueabihf
	@echo "Install cross-compiler with: sudo apt-get install gcc-arm-linux-gnueabihf"

setup-cross-aarch64:
	@echo "Setting up AArch64 cross-compilation..."
	rustup target add aarch64-unknown-linux-gnu
	@echo "Install cross-compiler with: sudo apt-get install gcc-aarch64-linux-gnu"

setup-cross: setup-cross-armv7 setup-cross-aarch64

# Development environment setup
setup-dev:
	@echo "Setting up development environment..."
	rustup component add clippy rustfmt
	@echo "Install system dependencies with your package manager:"
	@echo "  Debian/Ubuntu: sudo apt-get install libv4l-dev libopencv-dev libudev-dev pkg-config cmake"
	@echo "  Arch Linux: sudo pacman -S v4l-utils opencv systemd-libs pkgconf cmake"

# Service management (requires systemd)
service-install: install
	@echo "Enabling and starting systemd service..."
	sudo systemctl enable doorcam.service
	sudo systemctl start doorcam.service

service-status:
	@echo "Checking service status..."
	sudo systemctl status doorcam.service

service-logs:
	@echo "Showing service logs..."
	sudo journalctl -u doorcam.service -f

service-stop:
	@echo "Stopping service..."
	sudo systemctl stop doorcam.service

service-restart:
	@echo "Restarting service..."
	sudo systemctl restart doorcam.service

# Quick development cycle
dev: format lint test build

# Release preparation
release: format-check lint test build-release package

# CI/CD targets
ci-test: check-deps lint format-check test

ci-build: build-release package-all

# Documentation
docs:
	@echo "Generating documentation..."
	cargo doc --features $(FEATURES) --no-deps --open

# Benchmarks (if any)
bench:
	@echo "Running benchmarks..."
	cargo bench --features $(FEATURES)

# Security audit
audit:
	@echo "Running security audit..."
	cargo audit

# Update dependencies
update:
	@echo "Updating dependencies..."
	cargo update

# Show build information
info:
	@echo "Project: $(PROJECT_NAME)"
	@echo "Features: $(FEATURES)"
	@echo "Install prefix: $(INSTALL_PREFIX)"
	@echo "Rust version: $$(rustc --version)"
	@echo "Cargo version: $$(cargo --version)"
	@echo "Target: $$(rustc -vV | grep host | cut -d' ' -f2)"