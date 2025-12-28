use super::server::StreamServerBuilder;
use crate::streaming::prep::{encode_placeholder, prepare_frame_for_streaming};
use crate::{
    config::StreamConfig,
    events::EventBus,
    frame::{FrameData, FrameFormat},
    ring_buffer::RingBuffer,
};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

fn create_test_frame(id: u64, format: FrameFormat) -> FrameData {
    let data = match format {
        FrameFormat::Mjpeg => vec![0xFF, 0xD8, 0xFF, 0xD9],
        FrameFormat::Yuyv => vec![0u8; 640 * 480 * 2],
        FrameFormat::Rgb24 => vec![0u8; 640 * 480 * 3],
    };

    FrameData::new(id, SystemTime::now(), data, 640, 480, format)
}

#[tokio::test]
async fn test_stream_server_builder() {
    let config = StreamConfig {
        ip: "127.0.0.1".to_string(),
        port: 8080,
        rotation: None,
    };
    let ring_buffer = Arc::new(RingBuffer::new(10, Duration::from_secs(1)));
    let event_bus = Arc::new(EventBus::new());

    let server = StreamServerBuilder::new()
        .config(config)
        .ring_buffer(ring_buffer)
        .event_bus(event_bus)
        .target_fps(30)
        .build()
        .unwrap();

    assert_eq!(server.config.ip, "127.0.0.1");
    assert_eq!(server.config.port, 8080);
}

#[tokio::test]
async fn test_prepare_frame_for_streaming_mjpeg() {
    let frame = create_test_frame(1, FrameFormat::Mjpeg);
    let result = prepare_frame_for_streaming(&frame).await.unwrap();

    assert_eq!(result, frame.data.as_ref().clone());
}

#[tokio::test]
async fn test_prepare_frame_for_streaming_yuyv() {
    let frame = create_test_frame(1, FrameFormat::Yuyv);
    let result = prepare_frame_for_streaming(&frame).await.unwrap();

    assert!(!result.is_empty());
    assert_eq!(result[0], 0xFF);
    assert_eq!(result[1], 0xD8);
    let len = result.len();
    assert_eq!(result[len - 2], 0xFF);
    assert_eq!(result[len - 1], 0xD9);
}

#[tokio::test]
async fn test_prepare_frame_for_streaming_rgb24() {
    let frame = create_test_frame(1, FrameFormat::Rgb24);
    let result = prepare_frame_for_streaming(&frame).await.unwrap();

    assert!(!result.is_empty());
    assert_eq!(result[0], 0xFF);
    assert_eq!(result[1], 0xD8);
    let len = result.len();
    assert_eq!(result[len - 2], 0xFF);
    assert_eq!(result[len - 1], 0xD9);
}

#[tokio::test]
async fn test_create_placeholder_jpeg() {
    let jpeg = encode_placeholder(640, 480, "TEST").unwrap();

    assert_eq!(jpeg[0], 0xFF);
    assert_eq!(jpeg[1], 0xD8);

    let len = jpeg.len();
    assert_eq!(jpeg[len - 2], 0xFF);
    assert_eq!(jpeg[len - 1], 0xD9);

    assert!(jpeg.len() > 50);
}

#[tokio::test]
async fn test_builder_validation() {
    let result = StreamServerBuilder::new()
        .ring_buffer(Arc::new(RingBuffer::new(10, Duration::from_secs(1))))
        .event_bus(Arc::new(EventBus::new()))
        .build();
    assert!(result.is_err());

    let result = StreamServerBuilder::new()
        .config(StreamConfig {
            ip: "127.0.0.1".to_string(),
            port: 8080,
            rotation: None,
        })
        .event_bus(Arc::new(EventBus::new()))
        .target_fps(30)
        .build();
    assert!(result.is_err());

    let result = StreamServerBuilder::new()
        .config(StreamConfig {
            ip: "127.0.0.1".to_string(),
            port: 8080,
            rotation: None,
        })
        .ring_buffer(Arc::new(RingBuffer::new(10, Duration::from_secs(1))))
        .target_fps(30)
        .build();
    assert!(result.is_err());
}

#[tokio::test]
async fn test_streaming_integration_prepare() {
    let ring_buffer = Arc::new(RingBuffer::new(10, Duration::from_secs(1)));
    let _event_bus = Arc::new(EventBus::new());

    let frame = create_test_frame(1, FrameFormat::Mjpeg);
    ring_buffer.push_frame(frame).await;

    let latest_frame = ring_buffer.get_latest_frame().await.unwrap();
    let jpeg_data = prepare_frame_for_streaming(&latest_frame).await.unwrap();
    assert!(!jpeg_data.is_empty());
}
