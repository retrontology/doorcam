# Doorcam

Rust-based door camera system with motion detection, capture, live streaming, and display/touch integration. This project is designed for a Raspberry Pi 4 running Raspbian OS with a HyperPixel 4.0 screen.

## Features
- Motion detection on a rolling ring buffer (pre/post-roll recording).
- MJPEG streaming server with a simple HTML viewer.
- Motion-triggered capture to WAL, optional JPEG frames, MP4 encoding, and metadata.
- Display output for HyperPixel-style framebuffers with backlight control.
- Touch input to wake/keep the display active.
- Event bus + orchestrator to coordinate components.
- CLI modes for config printing, validation, and dry runs.

## Quick start
1. Complete the Raspberry Pi setup steps below.
2. Review or copy `doorcam.toml`.
3. Build and run:

```
cargo build --release
./target/release/doorcam --config doorcam.toml
```

Common CLI options:
```
./target/release/doorcam --print-config
./target/release/doorcam --validate-config --config doorcam.toml
./target/release/doorcam --debug --enable-keyboard
```

Keyboard debug mode (optional):
- `SPACE` triggers a motion event
- `Q` or `ESC` shuts down

## Streaming
The MJPEG server exposes:
- Viewer: `http://<ip>:8080/`
- Stream: `http://<ip>:8080/stream.mjpg`
- Health: `http://<ip>:8080/health`

The bind address and port are controlled by `stream.ip` and `stream.port`.

## Capture output
By default, captures go to `./captures`.

Typical outputs:
- WAL files: `./captures/wal/<event_id>.wal`
- MP4 files: `./captures/<event_id>.mp4` (when `capture.video_encoding = true`)
- JPEG frames: `./captures/<event_id>/frames/*.jpg` (when `capture.keep_images = true`)
- Metadata: `./captures/metadata/<event_id>.json` (when `capture.save_metadata = true`)

## WAL tool
`waltool` converts WAL files into images/video/metadata.

```
cargo run --bin waltool -- --input ./captures/wal
cargo run --bin waltool -- --input ./captures/wal/20240101_120000_123.wal --video --images --metadata
```

## Configuration
Configuration is loaded from `doorcam.toml` (optional) and environment variables with the `DOORCAM_` prefix.

Examples:
```
DOORCAM_CAMERA_INDEX=1
DOORCAM_CAMERA_RESOLUTION="[1280, 720]"
DOORCAM_STREAM_PORT=9090
DOORCAM_CAPTURE_PATH="/var/lib/doorcam/captures"
DOORCAM_EVENT_PREROLL_SECONDS=3
DOORCAM_EVENT_POSTROLL_SECONDS=8
```

Key sections:
- `camera`: device index, resolution, fps, format.
- `analyzer`: motion detection fps and thresholds.
- `event`: pre/post-roll timing.
- `capture`: output path, timestamp overlay, encoding options.
- `stream`: bind ip/port and optional rotation.
- `display`: framebuffer/backlight/touch devices and activation period.
- `system`: retention and cleanup options.

Run `--print-config` for the built-in defaults.

## Systemd service
An example service file is provided at `systemd/doorcam.service`.

Typical setup:
1. Install the binary (example):
```
sudo install -m 755 ./target/release/doorcam /usr/local/bin/doorcam
```
2. Create config + data directories:
```
sudo install -d -m 755 /etc/doorcam /var/lib/doorcam
sudo cp doorcam.toml /etc/doorcam/doorcam.toml
```
3. Create a service user and grant device access:
```
sudo useradd --system --home /var/lib/doorcam --shell /usr/sbin/nologin doorcam
sudo usermod -aG video,input doorcam
```
4. Install the service file and start it:
```
sudo cp systemd/doorcam.service /etc/systemd/system/doorcam.service
sudo systemctl daemon-reload
sudo systemctl enable --now doorcam.service
```

Edit the service file if your binary or config paths differ.

## Raspberry Pi 4 setup (Raspbian + HyperPixel 4.0)
1. Start from Raspberry Pi OS (Bullseye or newer) and run system updates:
```
sudo apt update
sudo apt upgrade -y
```
2. Install build and runtime dependencies:
```
sudo apt install -y \
  build-essential pkg-config git \
  gstreamer1.0-tools gstreamer1.0-plugins-base \
  gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-ugly gstreamer1.0-libav \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  ffmpeg v4l-utils
```
3. Enable the HyperPixel 4.0 overlay in `/boot/config.txt`:
```
sudo sed -i '$a dtoverlay=hyperpixel4' /boot/config.txt
```
Reboot after adding the overlay:
```
sudo reboot
```
4. Verify devices:
```
ls /dev/video0
ls /dev/fb0
ls /dev/input/event0
```
Note: the camera should appear as a USB V4L2 device (e.g., `/dev/video0`).
5. Adjust `doorcam.toml` for device paths if they differ:
- `display.framebuffer_device` (default `/dev/fb0`)
- `display.backlight_device`
- `display.touch_device`
6. Build and run Doorcam (see Quick start).

## System requirements
Target platform:
- Raspberry Pi 4 running Raspbian OS.
- HyperPixel 4.0 display (framebuffer + touch input).
- USB V4L2 camera device (e.g., `/dev/video0`).
- GStreamer and FFmpeg with `h264_v4l2m2m` for hardware-accelerated encoding.

## Development
```
cargo test
```

Logging:
```
RUST_LOG=doorcam=debug ./target/release/doorcam --config doorcam.toml
```

## License
MIT. See `LICENSE`.

## Disclaimer
This project was created with the assistance of agentic AI.
