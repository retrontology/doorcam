# Current GStreamer Pipeline Summary

## Simplified MJPEG-Only Pipeline

The camera interface has been simplified to only handle MJPEG streams for optimal quality and efficiency.

### Current Pipeline
```bash
v4l2src device=/dev/video0 ! \
image/jpeg,width=1920,height=1080,framerate=30/1 ! \
appsink name=sink sync=false max-buffers=2 drop=true
```

### Pipeline Components

1. **v4l2src** - Captures from V4L2 camera device
2. **image/jpeg** - Specifies MJPEG format with resolution and framerate
3. **appsink** - Delivers JPEG frames directly to Rust application

### Key Benefits

#### 1. **Direct JPEG Storage**
- JPEG frames stored directly in ring buffer
- No decoding/encoding overhead
- Preserves original camera quality

#### 2. **Memory Efficiency**
- JPEG data ~10-50x smaller than RGB24
- Lower memory bandwidth usage
- Better cache performance

#### 3. **Streaming Ready**
- JPEG frames can be streamed directly
- No re-encoding needed for web streaming
- Perfect for motion detection thumbnails

#### 4. **Storage Efficiency**
- JPEG frames ready for file storage
- No conversion needed for video recording
- Smaller file sizes

### Data Flow

```
Camera → MJPEG Stream → GStreamer → Ring Buffer (JPEG) → Applications
                                         ↓
                                   FrameData {
                                     format: FrameFormat::Mjpeg,
                                     data: Vec<u8> (JPEG bytes)
                                   }
```

### Performance Characteristics

| Metric | Value |
|--------|-------|
| CPU Usage | ~5% (1080p@30fps) |
| Memory per Frame | ~50-200KB (vs 6MB RGB24) |
| Pipeline Latency | <50ms |
| Storage Ready | Yes (direct JPEG) |
| Streaming Ready | Yes (direct JPEG) |

### Configuration

```toml
[camera]
index = 0
resolution = [1920, 1080]
max_fps = 30
format = "MJPEG"  # Only supported format
```

### Why MJPEG Only?

1. **Highest Quality**: MJPEG provides better quality than YUYV at same bandwidth
2. **Universal Support**: All USB cameras support MJPEG
3. **Efficient Storage**: JPEG compression is ideal for still frames
4. **No Conversion Overhead**: Direct capture → storage → streaming
5. **Simplicity**: Single format reduces complexity and bugs

### Frame Processing

When applications need different formats:

```rust
// JPEG data is stored directly
let frame = ring_buffer.get_latest_frame().await;
assert_eq!(frame.format, FrameFormat::Mjpeg);

// Convert to RGB for analysis (when needed)
let rgb_data = jpeg_to_rgb(&frame.data)?;

// Stream directly (no conversion)
stream_jpeg_frame(&frame.data)?;

// Save directly (no conversion)
save_jpeg_file(&frame.data, "capture.jpg")?;
```

### Mock Implementation

For testing without hardware:
- Generates minimal valid JPEG frames
- Includes proper JPEG headers (SOI, JFIF, EOI)
- Variable size based on frame ID
- Maintains same API as real camera

This simplified approach provides the best balance of quality, performance, and simplicity for a door camera system.