use super::*;
use crate::{
    config::{CaptureConfig, EventConfig},
    events::EventBus,
    frame::{FrameData, FrameFormat},
    ring_buffer::RingBuffer,
};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

fn create_test_configs() -> (CaptureConfig, EventConfig) {
    let capture_config = CaptureConfig {
        path: "./test_captures".to_string(),
        timestamp_overlay: true,
        timestamp_font_path: "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".to_string(),
        timestamp_font_size: 24.0,
        timestamp_timezone: "UTC".to_string(),
        video_encoding: false,
        keep_images: true,
        save_metadata: true,
        rotation: None,
    };

    let event_config = EventConfig {
        preroll_seconds: 2,
        postroll_seconds: 3,
    };

    (capture_config, event_config)
}

fn create_test_frame(id: u64, timestamp: SystemTime) -> FrameData {
    FrameData::new(id, timestamp, vec![0u8; 1024], 640, 480, FrameFormat::Mjpeg)
}

#[tokio::test]
async fn test_video_capture_creation() {
    let (capture_config, event_config) = create_test_configs();
    let event_bus = Arc::new(EventBus::new());
    let ring_buffer = Arc::new(RingBuffer::new(50, Duration::from_secs(5)));

    let capture = VideoCapture::new(capture_config, event_config, event_bus, ring_buffer);

    let stats = capture.get_capture_stats().await;
    assert_eq!(stats.active_captures, 0);
}

#[tokio::test]
async fn test_motion_triggered_capture() {
    let (capture_config, event_config) = create_test_configs();
    let event_bus = Arc::new(EventBus::new());
    let ring_buffer = Arc::new(RingBuffer::new(50, Duration::from_secs(5)));

    let _receiver = event_bus.subscribe();

    let now = SystemTime::now();
    for i in 0..10 {
        let frame = create_test_frame(i, now - Duration::from_millis(100 * (10 - i)));
        ring_buffer.push_frame(frame).await;
    }

    let capture = VideoCapture::new(
        capture_config,
        event_config,
        Arc::clone(&event_bus),
        ring_buffer,
    );

    let motion_time = SystemTime::now();
    capture.handle_motion_detected(motion_time).await.unwrap();

    let stats = capture.get_capture_stats().await;
    assert_eq!(stats.active_captures, 1);
}

#[tokio::test]
async fn test_capture_completion() {
    let (mut capture_config, mut event_config) = create_test_configs();
    event_config.postroll_seconds = 1;
    capture_config.save_metadata = false;
    capture_config.keep_images = false;

    let event_bus = Arc::new(EventBus::new());
    let ring_buffer = Arc::new(RingBuffer::new(50, Duration::from_secs(5)));

    let _receiver = event_bus.subscribe();

    let now = SystemTime::now();
    for i in 0..20 {
        let frame = create_test_frame(i, now + Duration::from_millis(50 * i));
        ring_buffer.push_frame(frame).await;
    }

    let capture = VideoCapture::new(
        capture_config,
        event_config,
        Arc::clone(&event_bus),
        ring_buffer,
    );

    let motion_time = now + Duration::from_millis(500);
    capture.handle_motion_detected(motion_time).await.unwrap();

    tokio::time::sleep(Duration::from_millis(2000)).await;

    let stats = capture.get_capture_stats().await;
    assert_eq!(stats.active_captures, 0);
}
