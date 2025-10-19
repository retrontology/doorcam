use doorcam::{
    MotionAnalyzer, MotionAnalyzerIntegration, MotionAnalyzerIntegrationBuilder,
    EventBus, RingBufferBuilder, FrameData, FrameFormat,
    config::AnalyzerConfig,
};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::time::sleep;
use tracing::{info, Level};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    info!("Starting motion analyzer demo");

    // Create configuration
    let config = AnalyzerConfig {
        max_fps: 5,
        delta_threshold: 25,
        contour_minimum_area: 1000.0,
    };

    // Create ring buffer
    let ring_buffer = Arc::new(
        RingBufferBuilder::new()
            .capacity(30)
            .preroll_duration(Duration::from_secs(5))
            .build()?
    );

    // Create event bus
    let event_bus = Arc::new(EventBus::new(100));

    // Create motion analyzer integration
    let mut integration = MotionAnalyzerIntegrationBuilder::new()
        .with_config(config)
        .with_ring_buffer(Arc::clone(&ring_buffer))
        .with_event_bus(Arc::clone(&event_bus))
        .build()
        .await?;

    // Subscribe to events to see motion detection
    let mut event_receiver = event_bus.subscribe();
    tokio::spawn(async move {
        while let Ok(event) = event_receiver.recv().await {
            match event {
                doorcam::DoorcamEvent::MotionDetected { contour_area, timestamp } => {
                    info!("🎯 Motion detected! Area: {:.2}, Time: {:?}", contour_area, timestamp);
                }
                doorcam::DoorcamEvent::SystemError { component, error } => {
                    info!("❌ System error in {}: {}", component, error);
                }
                _ => {
                    // Ignore other events for this demo
                }
            }
        }
    });

    // Start the motion analyzer
    info!("Starting motion analyzer integration...");
    integration.start().await?;

    // Simulate adding frames to the ring buffer
    info!("Simulating camera frames...");
    for i in 0..20 {
        // Create a synthetic frame (in a real application, this would come from the camera)
        let frame_data = vec![0u8; 640 * 480 * 3]; // RGB24 frame
        let frame = FrameData::new(
            i,
            SystemTime::now(),
            frame_data,
            640,
            480,
            FrameFormat::Rgb24,
        );

        // Add frame to ring buffer
        ring_buffer.push_frame(frame).await;
        
        info!("Added frame {} to ring buffer", i);
        
        // Wait a bit between frames
        sleep(Duration::from_millis(200)).await;
    }

    // Let the analyzer run for a bit
    info!("Letting analyzer run for 2 seconds...");
    sleep(Duration::from_secs(2)).await;

    // Stop the integration
    info!("Stopping motion analyzer integration...");
    integration.stop().await?;

    info!("Motion analyzer demo completed!");
    Ok(())
}