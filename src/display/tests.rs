use super::*;
use crate::config::DisplayConfig;
use crate::frame::{FrameData, FrameFormat};
use std::time::SystemTime;

fn create_test_config() -> DisplayConfig {
    DisplayConfig {
        framebuffer_device: "/tmp/test_fb".to_string(),
        backlight_device: "/tmp/test_backlight".to_string(),
        touch_device: "/tmp/test_touch".to_string(),
        activation_period_seconds: 5,
        resolution: (800, 480),
        rotation: None,
    }
}

#[tokio::test]
async fn test_display_controller_creation() {
    let config = create_test_config();

    let result = DisplayController::new(config).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_display_activation_state() {
    let config = create_test_config();
    let controller = DisplayController::new(config).await.unwrap();

    assert!(!controller.is_active());

    controller
        .is_active
        .store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(controller.is_active());

    controller
        .is_active
        .store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(!controller.is_active());
}

#[tokio::test]
async fn test_placeholder_display_data() {
    let config = create_test_config();
    let _controller = DisplayController::new(config).await.unwrap();

    let data = DisplayConverter::create_placeholder_rgb565(320, 240).unwrap();
    assert_eq!(data.len(), 320 * 240 * 2);
}

#[test]
fn test_rgb24_to_rgb565_conversion() {
    let rgb24_data = vec![
        255, 0, 0, // Red
        0, 255, 0, // Green
        0, 0, 255, // Blue
    ];

    let rgb565_data = DisplayConverter::rgb24_to_rgb565(&rgb24_data, 3, 1).unwrap();

    assert_eq!(rgb565_data.len(), 6);

    let red_pixel = ((rgb565_data[1] as u16) << 8) | (rgb565_data[0] as u16);
    assert_eq!(red_pixel & 0xF800, 0xF800);
}

#[test]
fn test_rgb24_to_rgb565_invalid_size() {
    let invalid_data = vec![255, 0];
    let result = DisplayConverter::rgb24_to_rgb565(&invalid_data, 1, 1);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_frame_conversion() {
    let config = create_test_config();
    let _controller = DisplayController::new(config).await.unwrap();

    let _frame = FrameData::new(
        1,
        SystemTime::now(),
        vec![0u8; 100],
        320,
        240,
        FrameFormat::Mjpeg,
    );

    let rgb24_data = vec![255u8; 320 * 240 * 3];
    let display_data = DisplayConverter::rgb24_to_rgb565(&rgb24_data, 320, 240).unwrap();

    assert_eq!(display_data.len(), 320 * 240 * 2);
}

#[test]
fn test_rgb565_scaling() {
    let src_data = vec![
        0x00, 0xF8, // Red pixel
        0xE0, 0x07, // Green pixel
        0x1F, 0x00, // Blue pixel
        0xFF, 0xFF, // White pixel
    ];

    let scaled_data = DisplayConverter::scale_rgb565(&src_data, 2, 2, 4, 4).unwrap();
    assert_eq!(scaled_data.len(), 32);
}

#[test]
fn test_rgb565_cropping() {
    let src_data = vec![0u8; 4 * 4 * 2];

    let cropped_data = DisplayConverter::crop_rgb565(&src_data, 4, 4, 2, 2).unwrap();

    assert_eq!(cropped_data.len(), 8);
}
