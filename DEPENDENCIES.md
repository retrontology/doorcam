# Dependencies Documentation

This document outlines the system dependencies required for building and running the Doorcam Rust application.

## Runtime Dependencies

### Required System Libraries

#### Video Capture (V4L2)
- **libv4l-dev** - Video4Linux2 development libraries
- **v4l-utils** - Video4Linux2 utilities for testing

```bash
# Debian/Ubuntu
sudo apt-get install libv4l-dev v4l-utils

# Arch Linux
sudo pacman -S v4l-utils

# CentOS/RHEL/Fedora
sudo dnf install libv4l-devel v4l-utils
```

#### OpenCV (Motion Analysis)
- **libopencv-dev** - OpenCV development libraries
- **libopencv-contrib-dev** - OpenCV contrib modules (optional)

```bash
# Debian/Ubuntu
sudo apt-get install libopencv-dev libopencv-contrib-dev

# Arch Linux
sudo pacman -S opencv

# CentOS/RHEL/Fedora
sudo dnf install opencv-devel
```

#### Input Device Handling
- **libudev-dev** - udev development libraries for device monitoring

```bash
# Debian/Ubuntu
sudo apt-get install libudev-dev

# Arch Linux
sudo pacman -S systemd-libs

# CentOS/RHEL/Fedora
sudo dnf install systemd-devel
```

### Hardware-Specific Dependencies

#### Raspberry Pi
- **libraspberrypi-dev** - Raspberry Pi GPU libraries (for hardware acceleration)
- **libdrm-dev** - Direct Rendering Manager libraries

```bash
# Raspberry Pi OS
sudo apt-get install libraspberrypi-dev libdrm-dev

# Enable GPU memory split (add to /boot/config.txt)
gpu_mem=128
```

#### Display Support (HyperPixel 4.0)
- **libdrm-dev** - DRM libraries for display management
- **libgbm-dev** - Generic Buffer Management

```bash
# Debian/Ubuntu/Raspberry Pi OS
sudo apt-get install libdrm-dev libgbm-dev
```

## Build Dependencies

### Rust Toolchain
- **Rust 1.70+** - Minimum supported Rust version
- **Cargo** - Rust package manager (included with Rust)

```bash
# Install via rustup (recommended)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Or via package manager
# Debian/Ubuntu
sudo apt-get install rustc cargo

# Arch Linux
sudo pacman -S rust

# CentOS/RHEL/Fedora
sudo dnf install rust cargo
```

### Cross-Compilation Dependencies

#### For ARM Targets (Raspberry Pi)

##### ARMv7 (Raspberry Pi 2/3/Zero 2)
```bash
# Install target
rustup target add armv7-unknown-linux-gnueabihf

# Install cross-compiler
# Debian/Ubuntu
sudo apt-get install gcc-arm-linux-gnueabihf

# Arch Linux
sudo pacman -S arm-linux-gnueabihf-gcc

# Cross-compilation libraries
sudo apt-get install libv4l-dev:armhf libopencv-dev:armhf libudev-dev:armhf
```

##### AArch64 (Raspberry Pi 4/5)
```bash
# Install target
rustup target add aarch64-unknown-linux-gnu

# Install cross-compiler
# Debian/Ubuntu
sudo apt-get install gcc-aarch64-linux-gnu

# Arch Linux
sudo pacman -S aarch64-linux-gnu-gcc

# Cross-compilation libraries
sudo apt-get install libv4l-dev:arm64 libopencv-dev:arm64 libudev-dev:arm64
```

### Development Dependencies

#### Additional Tools
- **pkg-config** - Package configuration tool
- **cmake** - Build system (required by some native dependencies)
- **git** - Version control

```bash
# Debian/Ubuntu
sudo apt-get install pkg-config cmake git

# Arch Linux
sudo pacman -S pkgconf cmake git

# CentOS/RHEL/Fedora
sudo dnf install pkgconfig cmake git
```

#### Optional Development Tools
- **gdb** - GNU Debugger
- **valgrind** - Memory debugging tool
- **strace** - System call tracer

```bash
# Debian/Ubuntu
sudo apt-get install gdb valgrind strace

# Arch Linux
sudo pacman -S gdb valgrind strace

# CentOS/RHEL/Fedora
sudo dnf install gdb valgrind strace
```

## Runtime System Requirements

### User and Groups
The application requires specific user and group memberships:

```bash
# Create service user
sudo useradd --system --home-dir /var/lib/doorcam --create-home --shell /bin/false doorcam

# Add to required groups
sudo usermod -a -G video,input doorcam
```

### Device Permissions
Ensure the service user has access to required devices:

```bash
# Video devices
sudo chmod 666 /dev/video*

# Framebuffer devices
sudo chmod 666 /dev/fb*

# Input devices
sudo chmod 644 /dev/input/event*

# Or use udev rules (recommended)
# Create /etc/udev/rules.d/99-doorcam.rules:
SUBSYSTEM=="video4linux", GROUP="video", MODE="0664"
SUBSYSTEM=="graphics", GROUP="video", MODE="0664"
SUBSYSTEM=="input", GROUP="input", MODE="0644"
```

### Directory Structure
Required directories with proper permissions:

```bash
sudo mkdir -p /etc/doorcam /var/lib/doorcam /var/log/doorcam
sudo chown root:doorcam /etc/doorcam
sudo chown doorcam:doorcam /var/lib/doorcam /var/log/doorcam
sudo chmod 755 /etc/doorcam /var/lib/doorcam /var/log/doorcam
```

## Feature-Specific Dependencies

### Camera Feature
- V4L2 libraries and drivers
- Camera device access permissions

### Motion Analysis Feature
- OpenCV libraries
- Sufficient CPU/GPU resources for real-time processing

### Streaming Feature
- Network connectivity
- Sufficient bandwidth for MJPEG streaming

### Display Feature
- Framebuffer or DRM display support
- Touch input device support
- Display hardware (HyperPixel 4.0 or compatible)

## Troubleshooting Dependencies

### Common Issues

#### OpenCV Not Found
```bash
# Check OpenCV installation
pkg-config --modversion opencv4

# If not found, install development packages
sudo apt-get install libopencv-dev

# For cross-compilation, ensure cross-arch packages are installed
sudo apt-get install libopencv-dev:armhf  # for ARMv7
sudo apt-get install libopencv-dev:arm64  # for AArch64
```

#### V4L2 Libraries Missing
```bash
# Check V4L2 installation
pkg-config --modversion libv4l2

# Install if missing
sudo apt-get install libv4l-dev
```

#### Cross-Compilation Linker Errors
```bash
# Ensure cross-compiler is installed
arm-linux-gnueabihf-gcc --version  # for ARMv7
aarch64-linux-gnu-gcc --version     # for AArch64

# Install if missing
sudo apt-get install gcc-arm-linux-gnueabihf gcc-aarch64-linux-gnu
```

#### Permission Denied Errors
```bash
# Check device permissions
ls -la /dev/video* /dev/fb* /dev/input/event*

# Fix permissions or add user to groups
sudo usermod -a -G video,input $USER
# Log out and back in for group changes to take effect
```

### Verification Commands

#### Test Camera Access
```bash
# List available cameras
v4l2-ctl --list-devices

# Test camera capture
v4l2-ctl --device=/dev/video0 --stream-mmap --stream-count=1
```

#### Test Display Access
```bash
# Check framebuffer devices
ls -la /dev/fb*

# Test framebuffer write (be careful!)
# cat /dev/urandom | head -c 1024 > /dev/fb0
```

#### Test Input Devices
```bash
# List input devices
cat /proc/bus/input/devices

# Test input events (requires root)
sudo evtest /dev/input/event0
```

## Minimum System Requirements

### Hardware
- **CPU**: ARM Cortex-A7 (Raspberry Pi 2) or better
- **RAM**: 512MB minimum, 1GB recommended
- **Storage**: 100MB for application, additional space for recordings
- **Camera**: USB UVC or CSI camera compatible with V4L2
- **Display**: Optional, HyperPixel 4.0 or compatible framebuffer display

### Software
- **OS**: Linux with kernel 4.4+ (for V4L2 support)
- **glibc**: 2.17+ (for Rust std library compatibility)
- **systemd**: Optional, for service management

### Network
- **Bandwidth**: 1Mbps+ for MJPEG streaming (depends on resolution and quality)
- **Connectivity**: Ethernet or Wi-Fi for remote access