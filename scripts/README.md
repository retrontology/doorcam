# Build and Installation Scripts

This directory contains scripts for building, installing, and managing the Doorcam Rust application.

## Scripts

### build.sh
Cross-platform build script with support for ARM targets.

**Features:**
- Native and cross-compilation builds
- Release and debug modes
- Feature selection
- Package creation
- Verbose output options

**Usage:**
```bash
# Make executable (Linux/macOS)
chmod +x scripts/build.sh

# Basic usage
./scripts/build.sh --release armv7-pi
./scripts/build.sh --package --all-features native

# See all options
./scripts/build.sh --help
```

### install.sh
System installation script for Linux systems.

**Features:**
- Creates system user and directories
- Installs binary and configuration
- Sets up systemd service
- Configures permissions and security

**Usage:**
```bash
# Make executable
chmod +x scripts/install.sh

# Standard installation
sudo ./scripts/install.sh

# Custom installation
sudo ./scripts/install.sh --prefix /opt/doorcam --user mycam

# See all options
./scripts/install.sh --help
```

### uninstall.sh
System removal script.

**Features:**
- Removes binary and service
- Optional preservation of config/data
- Safe removal with confirmations
- Dry-run mode for testing

**Usage:**
```bash
# Make executable
chmod +x scripts/uninstall.sh

# Standard removal
sudo ./scripts/uninstall.sh

# Keep configuration and data
sudo ./scripts/uninstall.sh --keep-config --keep-data

# See all options
./scripts/uninstall.sh --help
```

## Prerequisites

### For Building
- Rust 1.70+ toolchain
- System dependencies (see DEPENDENCIES.md)
- Cross-compilation toolchains (for ARM targets)

### For Installation
- Linux system with systemd
- Root privileges (sudo)
- Required system libraries

## Quick Start

### 1. Build for Current Platform
```bash
./scripts/build.sh --release --all-features native
```

### 2. Build for Raspberry Pi
```bash
# Install cross-compilation target
rustup target add armv7-unknown-linux-gnueabihf

# Build
./scripts/build.sh --release --all-features armv7-pi
```

### 3. Create Distribution Package
```bash
./scripts/build.sh --release --package --all-features armv7-pi
```

### 4. Install on Target System
```bash
# Extract package
tar -xzf dist/doorcam-*-raspberry-pi-armv7.tar.gz
cd doorcam-*-raspberry-pi-armv7/

# Install
sudo ./scripts/install.sh
```

## Cross-Compilation Setup

### ARMv7 (Raspberry Pi 2/3/Zero 2)
```bash
# Install Rust target
rustup target add armv7-unknown-linux-gnueabihf

# Install cross-compiler (Ubuntu/Debian)
sudo apt-get install gcc-arm-linux-gnueabihf

# Install cross-compilation libraries
sudo apt-get install libv4l-dev:armhf libopencv-dev:armhf libudev-dev:armhf
```

### AArch64 (Raspberry Pi 4/5)
```bash
# Install Rust target
rustup target add aarch64-unknown-linux-gnu

# Install cross-compiler (Ubuntu/Debian)
sudo apt-get install gcc-aarch64-linux-gnu

# Install cross-compilation libraries
sudo apt-get install libv4l-dev:arm64 libopencv-dev:arm64 libudev-dev:arm64
```

## Troubleshooting

### Build Issues

#### Missing Cross-Compiler
```bash
# Error: linker `arm-linux-gnueabihf-gcc` not found
sudo apt-get install gcc-arm-linux-gnueabihf
```

#### Missing Libraries
```bash
# Error: could not find system library 'opencv'
sudo apt-get install libopencv-dev

# For cross-compilation
sudo apt-get install libopencv-dev:armhf  # ARMv7
sudo apt-get install libopencv-dev:arm64  # AArch64
```

#### Permission Errors
```bash
# Make scripts executable
chmod +x scripts/*.sh

# Run with appropriate privileges
sudo ./scripts/install.sh
```

### Installation Issues

#### Service User Creation Failed
```bash
# Check if user already exists
id doorcam

# Manually create if needed
sudo useradd --system --home-dir /var/lib/doorcam --create-home --shell /bin/false doorcam
```

#### Device Access Issues
```bash
# Check device permissions
ls -la /dev/video* /dev/fb* /dev/input/event*

# Add user to required groups
sudo usermod -a -G video,input doorcam
```

#### Systemd Service Issues
```bash
# Check service status
sudo systemctl status doorcam.service

# View logs
sudo journalctl -u doorcam.service

# Reload systemd configuration
sudo systemctl daemon-reload
```

## Integration with Makefile

The scripts are integrated with the project Makefile for convenience:

```bash
# Build targets
make build-pi          # Cross-compile for Raspberry Pi
make package-all       # Create all distribution packages

# Installation targets
make install           # Build and install locally
make service-install   # Install and start systemd service

# Development targets
make setup-cross       # Set up cross-compilation
make check-deps        # Verify dependencies
```

## Security Considerations

### Installation Security
- Scripts create dedicated system user
- Minimal file system permissions
- Systemd security hardening
- Device access restrictions

### Build Security
- No network access during build
- Reproducible builds
- Dependency verification
- Static linking where possible

## Customization

### Custom Installation Paths
Modify the installation script variables:
```bash
INSTALL_PREFIX="/opt/doorcam"
CONFIG_DIR="/opt/doorcam/etc"
DATA_DIR="/opt/doorcam/var"
```

### Custom Service Configuration
Edit the systemd service template before installation:
```bash
# Modify systemd/doorcam.service.template
# Then run installation
```

### Custom Build Features
Select specific features for smaller binaries:
```bash
./scripts/build.sh --release --features camera,streaming armv7-pi
```