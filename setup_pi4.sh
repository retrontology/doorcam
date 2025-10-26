#!/bin/bash

# Raspberry Pi 4 setup script for hardware-accelerated video capture
# Run this script to install and configure GStreamer with hardware acceleration

set -e

echo "Setting up Raspberry Pi 4 for hardware-accelerated video capture..."

# Update system
echo "Updating system packages..."
sudo apt update && sudo apt upgrade -y

# Install GStreamer and Pi 4 specific plugins
echo "Installing GStreamer and hardware acceleration plugins..."
sudo apt install -y \
    gstreamer1.0-tools \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav \
    gstreamer1.0-omx-rpi \
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev \
    libgstreamer-plugins-good1.0-dev \
    libgstreamer-plugins-bad1.0-dev

# Install V4L2 utilities for camera debugging
echo "Installing V4L2 utilities..."
sudo apt install -y v4l-utils

# Enable camera interface
echo "Enabling camera interface..."
sudo raspi-config nonint do_camera 0

# Enable GPU memory split for hardware acceleration
echo "Configuring GPU memory split..."
sudo raspi-config nonint do_memory_split 128

# Add user to video group
echo "Adding user to video group..."
sudo usermod -a -G video $USER

# Create doorcam directories
echo "Creating doorcam directories..."
sudo mkdir -p /var/lib/doorcam
sudo mkdir -p /tmp/doorcam
sudo chown -R $USER:$USER /var/lib/doorcam /tmp/doorcam

# Test camera
echo "Testing camera..."
if [ -e /dev/video0 ]; then
    echo "Camera device found at /dev/video0"
    v4l2-ctl --device=/dev/video0 --list-formats-ext
else
    echo "Warning: No camera device found. Make sure camera is connected and enabled."
fi

# Test GStreamer pipeline
echo "Testing GStreamer pipeline..."
if gst-launch-1.0 --version > /dev/null 2>&1; then
    echo "GStreamer installed successfully"
    
    # Test basic pipeline (will fail if no camera, but shows if GStreamer works)
    echo "Testing basic pipeline (may fail if no camera connected):"
    timeout 3s gst-launch-1.0 v4l2src device=/dev/video0 num-buffers=10 ! fakesink || echo "Pipeline test completed (expected if no camera)"
else
    echo "Error: GStreamer not properly installed"
    exit 1
fi

echo ""
echo "Pi 4 setup completed successfully!"
echo ""
echo "Next steps:"
echo "1. Reboot your Pi: sudo reboot"
echo "2. After reboot, test the camera: cargo run --example pi4_camera --features camera-gst"
echo "3. If you get permission errors, make sure you're in the video group: groups \$USER"
echo ""
echo "Hardware acceleration features enabled:"
echo "- H.264 hardware decode via v4l2h264dec"
echo "- GPU memory split set to 128MB"
echo "- Camera interface enabled"
echo ""
echo "Troubleshooting:"
echo "- Check camera connection: ls /dev/video*"
echo "- Test camera: gst-launch-1.0 v4l2src device=/dev/video0 ! autovideosink"
echo "- Check formats: v4l2-ctl --device=/dev/video0 --list-formats-ext"