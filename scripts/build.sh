#!/bin/bash
# Doorcam Build Script
# Supports cross-compilation for ARM targets (Raspberry Pi)

set -euo pipefail

# Configuration
PROJECT_NAME="doorcam"
VERSION=$(grep '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')
BUILD_DIR="target"
DIST_DIR="dist"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Help function
show_help() {
    cat << EOF
Doorcam Build Script

Usage: $0 [OPTIONS] [TARGET]

OPTIONS:
    -h, --help          Show this help message
    -r, --release       Build in release mode (default: debug)
    -c, --clean         Clean build artifacts before building
    -f, --features      Comma-separated list of features to enable
    -a, --all-features  Enable all features
    -p, --package       Create distribution package
    -v, --verbose       Verbose output

TARGETS:
    native              Build for current platform (default)
    armv7-pi            Build for Raspberry Pi (ARMv7)
    aarch64-pi          Build for Raspberry Pi 4/5 (ARM64)
    x86_64-linux        Build for x86_64 Linux

EXAMPLES:
    $0                                  # Debug build for current platform
    $0 --release armv7-pi              # Release build for Raspberry Pi
    $0 --release --all-features native # Release build with all features
    $0 --package --release armv7-pi    # Create distribution package for Pi

EOF
}

# Default values
TARGET="native"
BUILD_MODE="debug"
CLEAN=false
FEATURES=""
ALL_FEATURES=false
PACKAGE=false
VERBOSE=false

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -r|--release)
            BUILD_MODE="release"
            shift
            ;;
        -c|--clean)
            CLEAN=true
            shift
            ;;
        -f|--features)
            FEATURES="$2"
            shift 2
            ;;
        -a|--all-features)
            ALL_FEATURES=true
            shift
            ;;
        -p|--package)
            PACKAGE=true
            shift
            ;;
        -v|--verbose)
            VERBOSE=true
            shift
            ;;
        native|armv7-pi|aarch64-pi|x86_64-linux)
            TARGET="$1"
            shift
            ;;
        *)
            log_error "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
done

# Set target triple based on target name
case $TARGET in
    native)
        TARGET_TRIPLE=""
        TARGET_NAME="native"
        ;;
    armv7-pi)
        TARGET_TRIPLE="armv7-unknown-linux-gnueabihf"
        TARGET_NAME="raspberry-pi-armv7"
        ;;
    aarch64-pi)
        TARGET_TRIPLE="aarch64-unknown-linux-gnu"
        TARGET_NAME="raspberry-pi-aarch64"
        ;;
    x86_64-linux)
        TARGET_TRIPLE="x86_64-unknown-linux-gnu"
        TARGET_NAME="x86_64-linux"
        ;;
    *)
        log_error "Unknown target: $TARGET"
        exit 1
        ;;
esac

log_info "Building $PROJECT_NAME v$VERSION"
log_info "Target: $TARGET_NAME"
log_info "Mode: $BUILD_MODE"

# Clean if requested
if [[ "$CLEAN" == "true" ]]; then
    log_info "Cleaning build artifacts..."
    cargo clean
fi

# Install target if cross-compiling
if [[ -n "$TARGET_TRIPLE" ]]; then
    log_info "Installing target $TARGET_TRIPLE..."
    rustup target add "$TARGET_TRIPLE"
fi

# Build cargo command
CARGO_CMD="cargo build"

if [[ "$BUILD_MODE" == "release" ]]; then
    CARGO_CMD="$CARGO_CMD --release"
fi

if [[ -n "$TARGET_TRIPLE" ]]; then
    CARGO_CMD="$CARGO_CMD --target $TARGET_TRIPLE"
fi

if [[ "$ALL_FEATURES" == "true" ]]; then
    CARGO_CMD="$CARGO_CMD --all-features"
elif [[ -n "$FEATURES" ]]; then
    CARGO_CMD="$CARGO_CMD --features $FEATURES"
else
    # Default features for production builds
    if [[ "$BUILD_MODE" == "release" ]]; then
        CARGO_CMD="$CARGO_CMD --features camera,motion_analysis,streaming,display"
    fi
fi

if [[ "$VERBOSE" == "true" ]]; then
    CARGO_CMD="$CARGO_CMD --verbose"
fi

# Execute build
log_info "Executing: $CARGO_CMD"
$CARGO_CMD

# Determine binary path
if [[ -n "$TARGET_TRIPLE" ]]; then
    BINARY_PATH="$BUILD_DIR/$TARGET_TRIPLE/$BUILD_MODE/$PROJECT_NAME"
else
    BINARY_PATH="$BUILD_DIR/$BUILD_MODE/$PROJECT_NAME"
fi

if [[ ! -f "$BINARY_PATH" ]]; then
    log_error "Build failed: binary not found at $BINARY_PATH"
    exit 1
fi

log_success "Build completed successfully!"
log_info "Binary location: $BINARY_PATH"

# Get binary size
BINARY_SIZE=$(du -h "$BINARY_PATH" | cut -f1)
log_info "Binary size: $BINARY_SIZE"

# Create package if requested
if [[ "$PACKAGE" == "true" ]]; then
    log_info "Creating distribution package..."
    
    # Create dist directory
    mkdir -p "$DIST_DIR"
    
    # Package name
    PACKAGE_NAME="${PROJECT_NAME}-${VERSION}-${TARGET_NAME}"
    PACKAGE_DIR="$DIST_DIR/$PACKAGE_NAME"
    
    # Clean and create package directory
    rm -rf "$PACKAGE_DIR"
    mkdir -p "$PACKAGE_DIR"
    
    # Copy binary
    cp "$BINARY_PATH" "$PACKAGE_DIR/"
    
    # Copy configuration files
    if [[ -f "doorcam.toml" ]]; then
        cp "doorcam.toml" "$PACKAGE_DIR/doorcam.toml.example"
    fi
    
    # Copy systemd service files
    if [[ -d "systemd" ]]; then
        cp -r systemd "$PACKAGE_DIR/"
    fi
    
    # Copy installation scripts
    if [[ -d "scripts" ]]; then
        mkdir -p "$PACKAGE_DIR/scripts"
        cp scripts/install.sh "$PACKAGE_DIR/scripts/" 2>/dev/null || true
        cp scripts/uninstall.sh "$PACKAGE_DIR/scripts/" 2>/dev/null || true
    fi
    
    # Copy documentation
    cp README.md "$PACKAGE_DIR/" 2>/dev/null || true
    cp LICENSE "$PACKAGE_DIR/" 2>/dev/null || true
    
    # Create archive
    cd "$DIST_DIR"
    tar -czf "${PACKAGE_NAME}.tar.gz" "$PACKAGE_NAME"
    cd ..
    
    log_success "Package created: $DIST_DIR/${PACKAGE_NAME}.tar.gz"
    
    # Show package contents
    log_info "Package contents:"
    tar -tzf "$DIST_DIR/${PACKAGE_NAME}.tar.gz" | sed 's/^/  /'
fi

log_success "Build process completed!"