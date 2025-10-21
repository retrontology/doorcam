use crate::config::DoorcamConfig;
use crate::error::{DoorcamError, Result};
use crate::events::EventBus;
use crate::ring_buffer::RingBuffer;

#[cfg(feature = "streaming")]
use crate::streaming::StreamServer;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{oneshot, Mutex};
use tokio::time::timeout;
use tracing::{debug, error, info};

/// Component lifecycle states
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

/// System shutdown reason
#[derive(Debug, Clone)]
pub enum ShutdownReason {
    Signal(String),
    Error(String),
    UserRequest,
    HealthCheck,
}

/// Main application coordinator that manages all system components
pub struct DoorcamOrchestrator {
    config: DoorcamConfig,
    event_bus: Arc<EventBus>,
    ring_buffer: Arc<RingBuffer>,
    
    // Components
    #[cfg(feature = "streaming")]
    stream_server: Option<StreamServer>,
    
    // Lifecycle management
    component_states: Arc<Mutex<HashMap<String, ComponentState>>>,
    shutdown_sender: Option<oneshot::Sender<ShutdownReason>>,
    shutdown_receiver: Option<oneshot::Receiver<ShutdownReason>>,
}

impl DoorcamOrchestrator {
    /// Create a new orchestrator with the given configuration
    pub fn new(config: DoorcamConfig) -> Result<Self> {
        let event_bus = Arc::new(EventBus::new(config.system.event_bus_capacity));
        let ring_buffer = Arc::new(RingBuffer::new(
            config.system.ring_buffer_capacity,
            Duration::from_secs(config.capture.preroll_seconds as u64),
        ));
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        
        #[cfg(feature = "streaming")]
        let stream_server = Some(StreamServer::new(
            config.stream.clone(),
            Arc::clone(&ring_buffer),
            Arc::clone(&event_bus),
        ));
        
        Ok(Self {
            config,
            event_bus,
            ring_buffer,
            #[cfg(feature = "streaming")]
            stream_server,
            component_states: Arc::new(Mutex::new(HashMap::new())),
            shutdown_sender: Some(shutdown_sender),
            shutdown_receiver: Some(shutdown_receiver),
        })
    }
    
    /// Initialize all system components
    pub async fn initialize(&mut self) -> Result<()> {
        info!("Initializing Doorcam system components");
        
        // Set initial component states
        let mut states = self.component_states.lock().await;
        states.insert("camera".to_string(), ComponentState::Stopped);
        states.insert("analyzer".to_string(), ComponentState::Stopped);
        states.insert("display".to_string(), ComponentState::Stopped);
        states.insert("capture".to_string(), ComponentState::Stopped);
        states.insert("storage".to_string(), ComponentState::Stopped);
        
        #[cfg(feature = "streaming")]
        states.insert("streaming".to_string(), ComponentState::Stopped);
        
        drop(states);
        
        info!("All components initialized successfully");
        Ok(())
    }
    
    /// Start all system components
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting Doorcam system");
        
        // Start streaming server if enabled
        #[cfg(feature = "streaming")]
        if let Some(_stream_server) = &self.stream_server {
            self.set_component_state("streaming", ComponentState::Starting).await;
            
            let server = StreamServer::new(
                self.config.stream.clone(),
                Arc::clone(&self.ring_buffer),
                Arc::clone(&self.event_bus),
            );
            
            // Start the server in a background task
            tokio::spawn(async move {
                if let Err(e) = server.start().await {
                    error!("Stream server error: {}", e);
                }
            });
            
            self.set_component_state("streaming", ComponentState::Running).await;
            info!("Streaming server started on {}:{}", self.config.stream.ip, self.config.stream.port);
        }
        
        // For now, just mark other components as running
        // TODO: Initialize actual components when integration builders are available
        self.set_component_state("camera", ComponentState::Running).await;
        self.set_component_state("analyzer", ComponentState::Running).await;
        self.set_component_state("display", ComponentState::Running).await;
        self.set_component_state("capture", ComponentState::Running).await;
        self.set_component_state("storage", ComponentState::Running).await;
        
        info!("Doorcam system started successfully");
        Ok(())
    }
    
    /// Run the main application loop with signal handling
    pub async fn run(&mut self) -> Result<i32> {
        info!("Doorcam system is running");
        
        // Set up signal handling for graceful shutdown
        let shutdown_sender = self.shutdown_sender.take()
            .ok_or_else(|| DoorcamError::System { message: "Shutdown sender already taken".to_string() })?;
        
        let shutdown_receiver = self.shutdown_receiver.take()
            .ok_or_else(|| DoorcamError::System { message: "Shutdown receiver already taken".to_string() })?;
        
        // Spawn signal handlers
        self.setup_signal_handlers(shutdown_sender).await;
        
        // Wait for shutdown signal
        let shutdown_reason = shutdown_receiver.await
            .map_err(|_| DoorcamError::System { message: "Shutdown channel closed unexpectedly".to_string() })?;
        
        info!("Shutdown initiated: {:?}", shutdown_reason);
        
        // Perform graceful shutdown
        let exit_code = self.shutdown().await?;
        
        info!("Doorcam system shutdown complete");
        Ok(exit_code)
    }
    
    /// Set up signal handlers for graceful shutdown
    async fn setup_signal_handlers(&self, shutdown_sender: oneshot::Sender<ShutdownReason>) {
        let shutdown_sender = Arc::new(Mutex::new(Some(shutdown_sender)));
        
        // Handle SIGTERM (systemd stop) - Unix only
        #[cfg(unix)]
        {
            let shutdown_sender_sigterm = Arc::clone(&shutdown_sender);
            tokio::spawn(async move {
                if let Some(()) = signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("Failed to register SIGTERM handler")
                    .recv()
                    .await
                {
                    info!("Received SIGTERM signal");
                    if let Some(sender) = shutdown_sender_sigterm.lock().await.take() {
                        let _ = sender.send(ShutdownReason::Signal("SIGTERM".to_string()));
                    }
                }
            });
        }
        
        // Handle SIGINT (Ctrl+C) - Cross-platform
        let shutdown_sender_sigint = Arc::clone(&shutdown_sender);
        tokio::spawn(async move {
            if let Ok(()) = tokio::signal::ctrl_c().await {
                info!("Received SIGINT signal (Ctrl+C)");
                if let Some(sender) = shutdown_sender_sigint.lock().await.take() {
                    let _ = sender.send(ShutdownReason::Signal("SIGINT".to_string()));
                }
            }
        });
    }
    
    /// Perform graceful shutdown of all components
    async fn shutdown(&mut self) -> Result<i32> {
        info!("Beginning graceful shutdown");
        
        let mut exit_code = 0;
        
        // Stop components in reverse dependency order
        #[cfg(feature = "streaming")]
        if let Err(e) = self.stop_component("streaming").await {
            error!("Error stopping streaming: {}", e);
            exit_code = 1;
        }
        
        if let Err(e) = self.stop_component("capture").await {
            error!("Error stopping capture: {}", e);
            exit_code = 1;
        }
        
        if let Err(e) = self.stop_component("display").await {
            error!("Error stopping display: {}", e);
            exit_code = 1;
        }
        
        if let Err(e) = self.stop_component("analyzer").await {
            error!("Error stopping analyzer: {}", e);
            exit_code = 1;
        }
        
        if let Err(e) = self.stop_component("camera").await {
            error!("Error stopping camera: {}", e);
            exit_code = 1;
        }
        
        if let Err(e) = self.stop_component("storage").await {
            error!("Error stopping storage: {}", e);
            exit_code = 1;
        }
        
        info!("Graceful shutdown completed with exit code: {}", exit_code);
        Ok(exit_code)
    }
    
    /// Stop a specific component
    async fn stop_component(&mut self, component: &str) -> Result<()> {
        info!("Stopping {} component", component);
        self.set_component_state(component, ComponentState::Stopping).await;
        
        // TODO: Implement actual component stopping when integrations are available
        // For now, just simulate a graceful stop with timeout
        match timeout(Duration::from_secs(5), async {
            // Simulate component shutdown work
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(())
        }).await {
            Ok(Ok(())) => {
                self.set_component_state(component, ComponentState::Stopped).await;
                info!("{} component stopped", component);
                Ok(())
            }
            Ok(Err(e)) => {
                self.set_component_state(component, ComponentState::Failed).await;
                error!("Error stopping {} component: {}", component, e);
                Err(e)
            }
            Err(_) => {
                self.set_component_state(component, ComponentState::Failed).await;
                let err = DoorcamError::System { message: format!("{} component stop timeout", component) };
                error!("{} component stop timeout", component);
                Err(err)
            }
        }
    }
    
    /// Update component state
    async fn set_component_state(&self, component: &str, state: ComponentState) {
        let mut states = self.component_states.lock().await;
        states.insert(component.to_string(), state.clone());
        debug!("Component '{}' state changed to: {:?}", component, state);
    }
    
    /// Get component state
    pub async fn get_component_state(&self, component: &str) -> Option<ComponentState> {
        let states = self.component_states.lock().await;
        states.get(component).cloned()
    }
    
    /// Get all component states
    pub async fn get_all_component_states(&self) -> HashMap<String, ComponentState> {
        let states = self.component_states.lock().await;
        states.clone()
    }
}
#[cfg(test)]
mod tests {
    use super::*;


    fn create_test_config() -> DoorcamConfig {
        DoorcamConfig {
            camera: crate::config::CameraConfig {
                index: 0,
                resolution: (640, 480),
                max_fps: 30,
                format: "MJPG".to_string(),
                rotation: None,
            },
            analyzer: crate::config::AnalyzerConfig {
                max_fps: 5,
                delta_threshold: 25,
                contour_minimum_area: 1000.0,
            },
            capture: crate::config::CaptureConfig {
                preroll_seconds: 5,
                postroll_seconds: 10,
                path: "/tmp/doorcam_test".to_string(),
                timestamp_overlay: true,
                video_encoding: false,
                keep_images: true,
            },
            system: crate::config::SystemConfig {
                trim_old: true,
                retention_days: 7,
                ring_buffer_capacity: 300,
                event_bus_capacity: 100,
            },
            stream: crate::config::StreamConfig {
                ip: "127.0.0.1".to_string(),
                port: 8080,
            },
            display: crate::config::DisplayConfig {
                framebuffer_device: "/dev/fb0".to_string(),
                backlight_device: "/sys/class/backlight/rpi_backlight".to_string(),
                touch_device: "/dev/input/event0".to_string(),
                activation_period_seconds: 30,
                rotation: None,
            },
        }
    }

    #[tokio::test]
    async fn test_orchestrator_creation() {
        let config = create_test_config();
        let orchestrator = DoorcamOrchestrator::new(config);
        
        assert!(orchestrator.is_ok());
        let orchestrator = orchestrator.unwrap();
        
        // Check initial component states
        let states = orchestrator.get_all_component_states().await;
        assert!(states.is_empty()); // No components started yet
    }

    #[tokio::test]
    async fn test_component_state_management() {
        let config = create_test_config();
        let orchestrator = DoorcamOrchestrator::new(config).unwrap();
        
        // Test setting and getting component states
        orchestrator.set_component_state("camera", ComponentState::Starting).await;
        let state = orchestrator.get_component_state("camera").await;
        assert_eq!(state, Some(ComponentState::Starting));
        
        orchestrator.set_component_state("camera", ComponentState::Running).await;
        let state = orchestrator.get_component_state("camera").await;
        assert_eq!(state, Some(ComponentState::Running));
        
        // Test multiple components
        orchestrator.set_component_state("analyzer", ComponentState::Running).await;
        orchestrator.set_component_state("streaming", ComponentState::Failed).await;
        
        let all_states = orchestrator.get_all_component_states().await;
        assert_eq!(all_states.len(), 3);
        assert_eq!(all_states.get("camera"), Some(&ComponentState::Running));
        assert_eq!(all_states.get("analyzer"), Some(&ComponentState::Running));
        assert_eq!(all_states.get("streaming"), Some(&ComponentState::Failed));
    }

    #[tokio::test]
    async fn test_component_state_transitions() {
        let config = create_test_config();
        let orchestrator = DoorcamOrchestrator::new(config).unwrap();
        
        // Test typical component lifecycle
        let component = "test_component";
        
        // Initial state should be None
        assert_eq!(orchestrator.get_component_state(component).await, None);
        
        // Starting -> Running -> Stopping -> Stopped
        orchestrator.set_component_state(component, ComponentState::Starting).await;
        assert_eq!(orchestrator.get_component_state(component).await, Some(ComponentState::Starting));
        
        orchestrator.set_component_state(component, ComponentState::Running).await;
        assert_eq!(orchestrator.get_component_state(component).await, Some(ComponentState::Running));
        
        orchestrator.set_component_state(component, ComponentState::Stopping).await;
        assert_eq!(orchestrator.get_component_state(component).await, Some(ComponentState::Stopping));
        
        orchestrator.set_component_state(component, ComponentState::Stopped).await;
        assert_eq!(orchestrator.get_component_state(component).await, Some(ComponentState::Stopped));
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
            ShutdownReason::UserRequest => {},
            _ => panic!("Expected UserRequest shutdown reason"),
        }
        
        let health_reason = ShutdownReason::HealthCheck;
        match health_reason {
            ShutdownReason::HealthCheck => {},
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
        let orchestrator = Arc::new(DoorcamOrchestrator::new(config).unwrap());
        
        // Test concurrent access to component states
        let mut handles = Vec::new();
        
        for i in 0..10 {
            let orchestrator_clone = Arc::clone(&orchestrator);
            let handle = tokio::spawn(async move {
                let component_name = format!("component_{}", i);
                orchestrator_clone.set_component_state(&component_name, ComponentState::Running).await;
                orchestrator_clone.get_component_state(&component_name).await
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
        let original_camera_index = config.camera.index;
        let original_analyzer_fps = config.analyzer.max_fps;
        
        let orchestrator = DoorcamOrchestrator::new(config).unwrap();
        
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
        let orchestrator = DoorcamOrchestrator::new(config).unwrap();
        
        // Test that component state management handles errors gracefully
        orchestrator.set_component_state("test", ComponentState::Failed).await;
        let state = orchestrator.get_component_state("test").await;
        assert_eq!(state, Some(ComponentState::Failed));
        
        // Test recovery scenario
        orchestrator.set_component_state("test", ComponentState::Starting).await;
        orchestrator.set_component_state("test", ComponentState::Running).await;
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
}