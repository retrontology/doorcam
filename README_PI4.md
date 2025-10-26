# Raspberry Pi 4 Hardware Acceleration Guide

This guide explains how to use doorcam with hardware acceleration on Raspberry Pi 4.

## Why GStreamer over V4L2?

The Pi 4 has dedicated hardware for video encoding/decoding that standard V4L2 libraries can't access effectively. GStreamer provides:

- **Hardware H.264 decode** via `v4l2h264dec`
- **GPU-accelerated video processing** 
- **Better performance** with lower CPU usage
- **More reliable streaming** at higher resolutions

## Quick Start

### 1. Setup Pi 4
```bash
# Run the setup script
./setup_pi4.sh

# Reboot after setup
sudo reboot
```

### 2. Build with GStreamer Support
```bash
# Build with GStreamer camera support (now default)
cargo build

# Run the Pi 4 example
cargo run --example pi4_camera
```

### 3. Use in Your Code
```rust
use doorcam::{CameraInterface, RingBuffer};
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create camera with H.264 hardware acceleration
    let config = CameraConfig {
        index: 0,
        resolution: (1920, 1080),
        max_fps: 30,
        format: "H264".to_string(), // Hardware accelerated!
        rotation: None,
    };
    
    let camera = CameraInterface::new(config).await?;
    let ring_buffer = Arc::new(RingBuffer::new(30, Duration::from_secs(2)));
    
    camera.start_capture(ring_buffer).await?;
    // Camera now using hardware acceleration!
    
    Ok(())
}
```

## Supported Format

### MJPEG Only (Simplified)
- **MJPEG**: High-quality JPEG frames captured directly from camera
- No decoding/encoding overhead - JPEG data stored directly in ring buffer
- Best quality with efficient storage and streaming

## Performance Benefits

| Metric | V4L2 (Old) | GStreamer MJPEG (New) |
|--------|------------|----------------------|
| CPU Usage | ~40% | ~5% |
| Memory Usage | High (RGB24) | Low (JPEG) |
| Storage Efficiency | Poor | Excellent |
| Streaming Ready | No | Yes |
| Max Resolution | 720p@15fps | 1920x1080@30fps |

## Configuration

### Pi 4 Optimized Config
```toml
[camera]
index = 0
resolution = [1920, 1080]  # Full HD with hardware acceleration
max_fps = 30
format = "MJPEG"  # High-quality JPEG frames
```

### Memory Configuration
The setup script configures:
- **GPU memory split**: 128MB (minimum for hardware decode)
- **Camera interface**: Enabled via raspi-config

## Troubleshooting

### Camera Not Found
```bash
# Check if camera is detected
ls /dev/video*

# Should show: /dev/video0 (and possibly /dev/video1 for codec)
```

### Permission Issues
```bash
# Add user to video group
sudo usermod -a -G video $USER

# Logout and login again, then check:
groups $USER
```

### Test GStreamer Pipeline
```bash
# Test basic camera access
gst-launch-1.0 v4l2src device=/dev/video0 ! autovideosink

# Test H.264 hardware decode
gst-launch-1.0 v4l2src device=/dev/video0 ! \
  video/x-h264,width=1920,height=1080,framerate=30/1 ! \
  v4l2h264dec ! autovideosink
```

### Check Available Formats
```bash
# List supported formats and resolutions
v4l2-ctl --device=/dev/video0 --list-formats-ext
```

### Performance Monitoring
```bash
# Monitor CPU/GPU usage while running
htop  # CPU usage
vcgencmd measure_temp  # GPU temperature
vcgencmd get_mem gpu  # GPU memory usage
```

## Hardware Requirements

- **Raspberry Pi 4** (any RAM variant)
- **Camera Module v2** or **USB camera with H.264 support**
- **SD Card**: Class 10 or better (for video storage)
- **Power Supply**: Official Pi 4 power supply recommended

## Limitations

- **Pi 3 and older**: Not supported (no hardware H.264 decode)
- **Multiple cameras**: Limited by USB bandwidth
- **4K video**: Possible but may require reduced frame rates

## Advanced Usage

### Custom Pipeline
```rust
// You can customize the GStreamer pipeline in camera_gst.rs
// Example: Add noise reduction
let pipeline = format!(
    "v4l2src device=/dev/video{} ! \
     video/x-h264,width={},height={},framerate={}/1 ! \
     v4l2h264dec ! \
     videoconvert ! \
     videoscale ! \
     video/x-raw,format=RGB ! \
     appsink name=sink",
    device_index, width, height, fps
);
```

### Multiple Cameras
```rust
// Create multiple camera interfaces
let camera1 = CameraInterface::new(config1).await?;
let camera2 = CameraInterface::new(config2).await?;

// Use separate ring buffers
let buffer1 = Arc::new(RingBuffer::new(30, Duration::from_secs(2)));
let buffer2 = Arc::new(RingBuffer::new(30, Duration::from_secs(2)));
```

## Migration from V4L2

If you're currently using the V4L2 camera interface:

1. **Install GStreamer**: Run `./setup_pi4.sh`
2. **Update dependencies**: GStreamer is now the default camera interface
3. **No code changes**: `CameraInterface` now uses GStreamer by default
4. **Update config**: Set format to "H264" for best performance

The API is nearly identical, so migration should be straightforward!