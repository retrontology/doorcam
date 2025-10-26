use doorcam::{DoorcamConfig, CameraInterface, RingBuffer};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, Level};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    info!("Starting Pi 4 hardware-accelerated camera demo");

    // Load configuration optimized for Pi 4
    let config = DoorcamConfig::load_from_file("config_pi4.toml").unwrap_or_else(|_| {
        info!("Using default Pi 4 configuration");
        create_pi4_config()
    });

    // Create ring buffer for frames
    let ring_buffer = Arc::new(RingBuffer::new(30, Duration::from_secs(2)));

    // Create GStreamer camera interface
    let camera = CameraInterface::new(config.camera).await?;

    info!("Testing camera connection...");
    camera.test_connection().await?;
    info!("Camera connection successful!");

    // Start capture
    info!("Starting hardware-accelerated capture...");
    camera.start_capture(Arc::clone(&ring_buffer)).await?;

    // Let it run for 10 seconds
    for i in 1..=10 {
        sleep(Duration::from_secs(1)).await;
        
        let frame_count = camera.frame_count();
        let latest_frame = ring_buffer.get_latest_frame().await;
        
        if let Some(frame) = latest_frame {
            info!(
                "Second {}: {} frames captured, latest: {}x{} ({} bytes)",
                i, frame_count, frame.width, frame.height, frame.data.len()
            );
        } else {
            info!("Second {}: {} frames captured, no frames available", i, frame_count);
        }
    }

    // Stop capture
    info!("Stopping capture...");
    camera.stop_capture().await?;

    info!("Pi 4 camera demo completed successfully!");
    Ok(())
}

fn create_pi4_config() -> DoorcamConfig {
    use doorcam::config::{CameraConfig, AnalyzerConfig, DisplayConfig, CaptureConfig, StreamConfig, SystemConfig};
    
    DoorcamConfig {
        camera: CameraConfig {
            index: 0,
            resolution: (1920, 1080), // Full HD for Pi 4
            max_fps: 30,
            format: "H264".to_string(), // Hardware-accelerated H.264
            rotation: None,
        },
        analyzer: AnalyzerConfig {
            max_fps: 5, // Process every 6th frame for performance
            delta_threshold: 25,
            contour_minimum_area: 1000.0,
        },
        display: DisplayConfig {
            framebuffer_device: "/dev/fb0".to_string(),
            backlight_device: "/sys/class/backlight/rpi_backlight/brightness".to_string(),
            touch_device: "/dev/input/event0".to_string(),
            activation_period_seconds: 30,
            resolution: (800, 480),
            rotation: None,
        },
        capture: CaptureConfig {
            preroll_seconds: 2,
            postroll_seconds: 5,
            path: "/tmp/doorcam".to_string(),
            timestamp_overlay: true,
            video_encoding: true,
            keep_images: true,
        },
        stream: StreamConfig {
            ip: "0.0.0.0".to_string(),
            port: 8080,
        },
        system: SystemConfig {
            trim_old: true,
            retention_days: 30,
            ring_buffer_capacity: 150,
            event_bus_capacity: 100,
        },
    }
}