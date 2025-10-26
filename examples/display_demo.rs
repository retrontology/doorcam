use doorcam::{
    DisplayIntegrationBuilder, EventBus, RingBufferBuilder, FrameData, FrameFormat,
};
use doorcam::config::DisplayConfig;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::time::sleep;
use tracing::{info, Level};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    info!("Starting display demo");

    // Create event bus
    let event_bus = Arc::new(EventBus::new(100));

    // Create display configuration
    let display_config = DisplayConfig {
        framebuffer_device: "/tmp/demo_fb".to_string(),
        backlight_device: "/tmp/demo_backlight".to_string(),
        touch_device: "/tmp/demo_touch".to_string(),
        activation_period_seconds: 10,
        resolution: (800, 480),
        rotation: None,
    };

    // Create ring buffer for frames
    let ring_buffer = Arc::new(
        RingBufferBuilder::new()
            .capacity(30)
            .preroll_duration(Duration::from_secs(5))
            .build()?
    );

    // Create display integration with mock touch
    let display_integration = DisplayIntegrationBuilder::new()
        .with_config(display_config)
        .with_event_bus(Arc::clone(&event_bus))
        .with_mock_touch(true)
        .build()
        .await?;

    // Start display integration
    display_integration.start(Arc::clone(&ring_buffer)).await?;

    // Add some demo frames to the ring buffer
    for i in 0..10 {
        let frame_data = vec![i as u8; 640 * 480 * 3]; // RGB24 data
        let frame = FrameData::new(
            i,
            SystemTime::now(),
            frame_data,
            640,
            480,
            FrameFormat::Rgb24,
        );
        
        ring_buffer.push_frame(frame).await;
        info!("Added demo frame {}", i);
        
        sleep(Duration::from_millis(100)).await;
    }

    // Manually activate display
    info!("Activating display manually");
    display_integration.activate_display().await?;

    // Wait for display to be active
    sleep(Duration::from_secs(2)).await;

    if display_integration.is_display_active() {
        info!("Display is active!");
    } else {
        info!("Display is not active");
    }

    // Wait for auto-deactivation
    info!("Waiting for auto-deactivation...");
    sleep(Duration::from_secs(12)).await;

    if !display_integration.is_display_active() {
        info!("Display auto-deactivated successfully!");
    } else {
        info!("Display is still active");
    }

    info!("Display demo completed");
    Ok(())
}