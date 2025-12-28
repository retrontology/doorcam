use super::*;
use crate::config::DoorcamConfig;
use std::sync::Arc;

fn create_test_config() -> DoorcamConfig {
    DoorcamConfig {
        camera: crate::config::CameraConfig {
            index: 0,
            resolution: (640, 480),
            fps: 30,
            format: "MJPG".to_string(),
        },
        analyzer: crate::config::AnalyzerConfig {
            fps: 5,
            delta_threshold: 25,
            contour_minimum_area: 1000.0,
            jpeg_decode_scale: 4,
        },
        event: crate::config::EventConfig {
            preroll_seconds: 5,
            postroll_seconds: 10,
        },
        capture: crate::config::CaptureConfig {
            path: "/tmp/doorcam_test".to_string(),
            timestamp_overlay: true,
            timestamp_font_path: "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".to_string(),
            timestamp_font_size: 24.0,
            timestamp_timezone: "UTC".to_string(),
            video_encoding: false,
            keep_images: true,
            save_metadata: true,
            rotation: None,
        },
        system: crate::config::SystemConfig {
            trim_old: true,
            retention_days: 7,
        },
        stream: crate::config::StreamConfig {
            ip: "127.0.0.1".to_string(),
            port: 8080,
            rotation: None,
        },
        display: crate::config::DisplayConfig {
            framebuffer_device: "/dev/fb0".to_string(),
            backlight_device: "/sys/class/backlight/rpi_backlight".to_string(),
            touch_device: "/dev/input/event0".to_string(),
            activation_period_seconds: 30,
            resolution: (800, 480),
            rotation: None,
        },
    }
}

#[tokio::test]
async fn test_orchestrator_creation() {
    let config = create_test_config();
    let orchestrator = DoorcamOrchestrator::new(config).await;

    // Orchestrator creation may fail if no camera hardware is available
    let orchestrator = match orchestrator {
        Ok(orchestrator) => orchestrator,
        Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen {
            ..
        }))
        | Err(crate::error::DoorcamError::Camera(crate::error::CameraError::Configuration {
            ..
        })) => {
            println!(
                "Camera hardware not available for testing - skipping orchestrator creation test"
            );
            return;
        }
        Err(e) => panic!("Unexpected orchestrator error: {}", e),
    };

    // Check initial component states
    let states = orchestrator.get_all_component_states().await;
    assert!(states.is_empty()); // No components started yet
}

#[tokio::test]
async fn test_component_state_management() {
    let config = create_test_config();
    let orchestrator = match DoorcamOrchestrator::new(config).await {
        Ok(orchestrator) => orchestrator,
        Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen {
            ..
        }))
        | Err(crate::error::DoorcamError::Camera(crate::error::CameraError::Configuration {
            ..
        })) => {
            println!("Camera hardware not available for testing - skipping component state test");
            return;
        }
        Err(e) => panic!("Unexpected orchestrator error: {}", e),
    };

    // Test setting and getting component states
    orchestrator
        .set_component_state("camera", ComponentState::Starting)
        .await;
    let state = orchestrator.get_component_state("camera").await;
    assert_eq!(state, Some(ComponentState::Starting));

    orchestrator
        .set_component_state("camera", ComponentState::Running)
        .await;
    let state = orchestrator.get_component_state("camera").await;
    assert_eq!(state, Some(ComponentState::Running));

    // Test multiple components
    orchestrator
        .set_component_state("analyzer", ComponentState::Running)
        .await;
    orchestrator
        .set_component_state("streaming", ComponentState::Failed)
        .await;

    let all_states = orchestrator.get_all_component_states().await;
    assert_eq!(all_states.len(), 3);
    assert_eq!(all_states.get("camera"), Some(&ComponentState::Running));
    assert_eq!(all_states.get("analyzer"), Some(&ComponentState::Running));
    assert_eq!(all_states.get("streaming"), Some(&ComponentState::Failed));
}

#[tokio::test]
async fn test_component_state_transitions() {
    let config = create_test_config();
    let orchestrator = match DoorcamOrchestrator::new(config).await {
        Ok(orchestrator) => orchestrator,
        Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen {
            ..
        }))
        | Err(crate::error::DoorcamError::Camera(crate::error::CameraError::Configuration {
            ..
        })) => {
            println!(
                "Camera hardware not available for testing - skipping component state transitions test"
            );
            return;
        }
        Err(e) => panic!("Unexpected orchestrator error: {}", e),
    };

    // Test typical component lifecycle
    let component = "test_component";

    // Initial state should be None
    assert_eq!(orchestrator.get_component_state(component).await, None);

    // Starting -> Running -> Stopping -> Stopped
    orchestrator
        .set_component_state(component, ComponentState::Starting)
        .await;
    assert_eq!(
        orchestrator.get_component_state(component).await,
        Some(ComponentState::Starting)
    );

    orchestrator
        .set_component_state(component, ComponentState::Running)
        .await;
    assert_eq!(
        orchestrator.get_component_state(component).await,
        Some(ComponentState::Running)
    );

    orchestrator
        .set_component_state(component, ComponentState::Stopping)
        .await;
    assert_eq!(
        orchestrator.get_component_state(component).await,
        Some(ComponentState::Stopping)
    );

    orchestrator
        .set_component_state(component, ComponentState::Stopped)
        .await;
    assert_eq!(
        orchestrator.get_component_state(component).await,
        Some(ComponentState::Stopped)
    );
}

#[tokio::test]
async fn test_shutdown_reason_types() {
    // Test different shutdown reason types
    let signal_reason = ShutdownReason::Signal("SIGTERM".to_string());
    match signal_reason {
        ShutdownReason::Signal(sig) => assert_eq!(sig, "SIGTERM"),
        _ => panic!("Expected Signal shutdown reason"),
    }

    let error_reason = ShutdownReason::Error("Critical failure".to_string());
    match error_reason {
        ShutdownReason::Error(msg) => assert_eq!(msg, "Critical failure"),
        _ => panic!("Expected Error shutdown reason"),
    }

    let user_reason = ShutdownReason::UserRequest;
    match user_reason {
        ShutdownReason::UserRequest => {}
        _ => panic!("Expected UserRequest shutdown reason"),
    }

    let health_reason = ShutdownReason::HealthCheck;
    match health_reason {
        ShutdownReason::HealthCheck => {}
        _ => panic!("Expected HealthCheck shutdown reason"),
    }
}

#[tokio::test]
async fn test_component_state_enum() {
    // Test ComponentState enum variants
    let _states = vec![
        ComponentState::Stopped,
        ComponentState::Starting,
        ComponentState::Running,
        ComponentState::Stopping,
        ComponentState::Failed,
    ];

    // Test Debug formatting
    assert_eq!(format!("{:?}", ComponentState::Running), "Running");
    assert_eq!(format!("{:?}", ComponentState::Failed), "Failed");

    // Test Clone
    let running_state = ComponentState::Running;
    let cloned_state = running_state.clone();
    assert_eq!(running_state, cloned_state);

    // Test PartialEq
    assert_eq!(ComponentState::Running, ComponentState::Running);
    assert_ne!(ComponentState::Running, ComponentState::Failed);
}

#[tokio::test]
async fn test_concurrent_component_state_access() {
    let config = create_test_config();
    let orchestrator = match DoorcamOrchestrator::new(config).await {
        Ok(orchestrator) => Arc::new(orchestrator),
        Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen {
            ..
        }))
        | Err(crate::error::DoorcamError::Camera(crate::error::CameraError::Configuration {
            ..
        })) => {
            println!("Camera hardware not available for testing - skipping concurrent access test");
            return;
        }
        Err(e) => panic!("Unexpected orchestrator error: {}", e),
    };

    // Test concurrent access to component states
    let mut handles = Vec::new();

    for i in 0..10 {
        let orchestrator_clone = Arc::clone(&orchestrator);
        let handle = tokio::spawn(async move {
            let component_name = format!("component_{}", i);
            orchestrator_clone
                .set_component_state(&component_name, ComponentState::Running)
                .await;
            orchestrator_clone
                .get_component_state(&component_name)
                .await
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert_eq!(result, Some(ComponentState::Running));
    }

    // Verify all components were created
    let all_states = orchestrator.get_all_component_states().await;
    assert_eq!(all_states.len(), 10);
}

#[tokio::test]
async fn test_orchestrator_configuration_access() {
    let config = create_test_config();
    let _original_camera_index = config.camera.index;
    let _original_analyzer_fps = config.analyzer.fps;

    let orchestrator = match DoorcamOrchestrator::new(config).await {
        Ok(orchestrator) => orchestrator,
        Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen {
            ..
        }))
        | Err(crate::error::DoorcamError::Camera(crate::error::CameraError::Configuration {
            ..
        })) => {
            println!(
                "Camera hardware not available for testing - skipping configuration access test"
            );
            return;
        }
        Err(e) => panic!("Unexpected orchestrator error: {}", e),
    };

    // The orchestrator should maintain access to configuration
    // (This test verifies the orchestrator was created with the config)
    // In a real implementation, you might want to add a config() method

    // For now, we test that the orchestrator was created successfully
    // with the provided configuration
    let states = orchestrator.get_all_component_states().await;
    assert!(states.is_empty()); // Initial state
}

#[tokio::test]
async fn test_error_handling_in_orchestrator() {
    let config = create_test_config();
    let orchestrator = match DoorcamOrchestrator::new(config).await {
        Ok(orchestrator) => orchestrator,
        Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen {
            ..
        }))
        | Err(crate::error::DoorcamError::Camera(crate::error::CameraError::Configuration {
            ..
        })) => {
            println!("Camera hardware not available for testing - skipping error handling test");
            return;
        }
        Err(e) => panic!("Unexpected orchestrator error: {}", e),
    };

    // Test that component state management handles errors gracefully
    orchestrator
        .set_component_state("test", ComponentState::Failed)
        .await;
    let state = orchestrator.get_component_state("test").await;
    assert_eq!(state, Some(ComponentState::Failed));

    // Test recovery scenario
    orchestrator
        .set_component_state("test", ComponentState::Starting)
        .await;
    orchestrator
        .set_component_state("test", ComponentState::Running)
        .await;
    let state = orchestrator.get_component_state("test").await;
    assert_eq!(state, Some(ComponentState::Running));
}

#[tokio::test]
async fn test_shutdown_reason_debug_formatting() {
    let reasons = vec![
        ShutdownReason::Signal("SIGTERM".to_string()),
        ShutdownReason::Error("Test error".to_string()),
        ShutdownReason::UserRequest,
        ShutdownReason::HealthCheck,
    ];

    for reason in reasons {
        let debug_str = format!("{:?}", reason);
        assert!(!debug_str.is_empty());

        // Test that the debug string contains expected content
        match reason {
            ShutdownReason::Signal(ref sig) => assert!(debug_str.contains(sig)),
            ShutdownReason::Error(ref msg) => assert!(debug_str.contains(msg)),
            ShutdownReason::UserRequest => assert!(debug_str.contains("UserRequest")),
            ShutdownReason::HealthCheck => assert!(debug_str.contains("HealthCheck")),
        }
    }
}
