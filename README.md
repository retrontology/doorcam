# Doorcam - Rust Door Camera System

A high-performance door camera system written in Rust, featuring motion detection, video capture, live streaming, and display functionality for Raspberry Pi.

## Features

- **Motion Detection**: Background subtraction with configurable sensitivity
- **Video Capture**: Motion-triggered recording with preroll/postroll
- **Live Streaming**: MJPEG HTTP streaming for remote viewing
- **Local Display**: HyperPixel 4.0 support with touch activation
- **Configuration**: TOML-based config with environment variable overrides
- **Logging**: Structured logging with tracing
- **Performance**: Lock-free ring buffer and async architecture

## Quick Start

### Prerequisites

- Rust 1.70+ 
- Raspberry Pi with camera module
- Optional: HyperPixel 4.0 display

### Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd doorcam
```

2. Build the project:
```bash
cargo build --release
```

3. Copy and customize the configuration:
```bash
cp doorcam.toml my-doorcam.toml
# Edit my-doorcam.toml as needed
```

4. Run the application:
```bash
cargo run -- --config my-doorcam.toml
```

## Configuration

The system uses TOML configuration files with environment variable overrides. See `doorcam.toml` for all available options.

### Environment Variables

All configuration options can be overridden using environment variables with the `DOORCAM_` prefix:

```bash
export DOORCAM_CAMERA_INDEX=1
export DOORCAM_STREAM_PORT=9090
export DOORCAM_CAPTURE_PATH="/var/lib/doorcam"
```

### Key Configuration Sections

- **camera**: Video device settings (resolution, FPS, format)
- **analyzer**: Motion detection parameters
- **capture**: Recording settings and storage paths
- **stream**: MJPEG server configuration
- **display**: HyperPixel display and touch settings
- **system**: Buffer sizes and cleanup settings

## Usage

### Command Line Options

```bash
doorcam [OPTIONS]

Options:
  -c, --config <FILE>    Configuration file path [default: doorcam.toml]
  -d, --debug           Enable debug logging
  -v, --verbose         Enable verbose logging
  -h, --help            Print help
  -V, --version         Print version
```

### Logging

The system uses structured logging with different levels:

- **Error**: Critical issues requiring attention
- **Warn**: Important events and recoverable errors  
- **Info**: General operational information
- **Debug**: Detailed diagnostic information

Set log levels via environment:
```bash
export RUST_LOG=doorcam=debug
```

## Development

### Project Structure

```
src/
├── main.rs              # Application entry point and CLI
├── lib.rs               # Module wiring and public re-exports
├── app/                 # Orchestrator, keyboard/debug utilities
├── core/                # Config, error, events, frames, ring buffer, recovery
├── features/            # Camera, analyzer, capture, display, touch, streaming
├── integrations/        # Integration layers (camera/buffer, capture, display, storage, streaming)
├── infrastructure/      # Storage subsystem and WAL implementation
└── bin/
    └── wal_tool.rs      # WAL maintenance utility
```

### Building

```bash
# Development build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Check code
cargo clippy
cargo fmt
```

## License

This project is licensed under the MIT License - see the LICENSE file for details.