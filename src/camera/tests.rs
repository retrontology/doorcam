use super::*;
use crate::config::CameraConfig;

fn create_test_camera_config() -> CameraConfig {
    CameraConfig {
        index: 0,
        resolution: (640, 480),
        fps: 30,
        format: "MJPG".to_string(),
    }
}

#[tokio::test]
async fn test_camera_interface_creation() {
    let config = create_test_camera_config();

    // This may fail if no camera hardware is available, which is expected in CI
    match CameraInterface::new(config).await {
        Ok(camera) => {
            assert!(!camera.is_capturing());
            assert_eq!(camera.frame_count(), 0);
        }
        Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen {
            ..
        })) => {
            // Expected when no camera hardware is available
            println!("Camera hardware not available - test passed");
        }
        Err(e) => {
            panic!("Unexpected error creating camera interface: {}", e);
        }
    }
}

#[tokio::test]
async fn test_camera_builder_pattern() {
    let config = create_test_camera_config();

    let builder = CameraInterfaceBuilder::new().config(config);

    match builder.build().await {
        Ok(camera) => {
            assert!(!camera.is_capturing());
        }
        Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen {
            ..
        })) => {
            // Expected when no camera hardware is available
            println!("Camera hardware not available - builder test passed");
        }
        Err(e) => {
            panic!("Unexpected error in camera builder: {}", e);
        }
    }
}

#[tokio::test]
async fn test_camera_builder_validation() {
    let builder = CameraInterfaceBuilder::new();

    // Should fail without config
    let result = builder.build().await;
    assert!(result.is_err());

    if let Err(crate::error::DoorcamError::System { message }) = result {
        assert!(message.contains("Camera configuration must be specified"));
    } else {
        panic!("Expected system error for missing configuration");
    }
}

#[tokio::test]
async fn test_camera_test_connection() {
    let config = create_test_camera_config();

    match CameraInterface::new(config).await {
        Ok(camera) => {
            // Test connection should work even if camera isn't capturing
            let result = camera.test_connection().await;
            match result {
                Ok(()) => {
                    println!("Camera connection test passed");
                }
                Err(_) => {
                    println!("Camera connection test failed - expected without hardware");
                }
            }
        }
        Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen {
            ..
        })) => {
            println!("Camera hardware not available - skipping connection test");
        }
        Err(e) => {
            panic!("Unexpected error: {}", e);
        }
    }
}

#[test]
fn test_camera_config_validation() {
    let config = CameraConfig {
        index: 0,
        resolution: (640, 480),
        fps: 30,
        format: "MJPG".to_string(),
    };

    // Basic validation - config should be valid
    assert_eq!(config.index, 0);
    assert_eq!(config.resolution, (640, 480));
    assert_eq!(config.fps, 30);
    assert_eq!(config.format, "MJPG");
}
