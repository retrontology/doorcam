use crate::config::DoorcamConfig;
use crate::error::{DoorcamError, Result};
use crate::events::EventBus;
use crate::ring_buffer::RingBuffer;
use crate::integration::CameraRingBufferIntegration;
use crate::analyzer_integration::MotionAnalyzerIntegration;
use crate::display_integration::DisplayIntegration;
use crate::capture_integration::VideoCaptureIntegration;
use crate::storage_integration::EventStorageIntegration;
use crate::keyboard_input::KeyboardInputHandler;

#[cfg(feature = "streaming")]
use crate::streaming::StreamServer;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{oneshot, Mutex};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
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
    camera_integration: Option<CameraRingBufferIntegration>,
    analyzer_integration: Option<Arc<Mutex<MotionAnalyzerIntegration>>>,
    display_integration: Option<DisplayIntegration>,
    capture_integration: Option<VideoCaptureIntegration>,
    storage_integration: Option<EventStorageIntegration>,
    keyboard_handler: Option<KeyboardInputHandler>,
    keyboard_enabled: bool,
    #[cfg(feature = "streaming")]
    stream_server: Option<StreamServer>,
    
    // Lifecycle management
    component_states: Arc<Mutex<HashMap<String, ComponentState>>>,
    shutdown_sender: Option<oneshot::Sender<ShutdownReason>>,
    shutdown_receiver: Option<oneshot::Receiver<ShutdownReason>>,
    cancellation_token: CancellationToken,
}

impl DoorcamOrchestrator {
    /// Create a new orchestrator with the given configuration
    pub async fn new(config: DoorcamConfig) -> Result<Self> {
        let event_bus = Arc::new(EventBus::new(config.system.event_bus_capacity));
        let ring_buffer = Arc::new(RingBuffer::new(
            config.system.ring_buffer_capacity,
            Duration::from_secs(config.capture.preroll_seconds as u64),
        ));
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        
        // Initialize camera integration
        let camera_integration = Some(CameraRingBufferIntegration::new(config.clone()).await?);
        
        // Initialize analyzer integration with camera's ring buffer
        let camera_ring_buffer = if let Some(ref camera_integration) = camera_integration {
            camera_integration.ring_buffer()
        } else {
            Arc::clone(&ring_buffer)
        };
        
        let analyzer_integration = Some(Arc::new(Mutex::new(MotionAnalyzerIntegration::new(
            config.analyzer.clone(),
            camera_ring_buffer,
            Arc::clone(&event_bus),
        ).await?)));
        
        // Initialize display integration
        let display_integration = Some(DisplayIntegration::new(
            config.display.clone(),
            Arc::clone(&event_bus),
        ).await?);
        
        // Initialize capture integration with camera's ring buffer
        let capture_ring_buffer = if let Some(camera_integration) = &camera_integration {
            camera_integration.ring_buffer()
        } else {
            Arc::clone(&ring_buffer)
        };
        
        let capture_integration = Some(VideoCaptureIntegration::new(
            config.capture.clone(),
            Arc::clone(&event_bus),
            capture_ring_buffer,
        ));
        
        // Initialize storage integration
        let storage_integration = Some(EventStorageIntegration::builder()
            .with_capture_config(config.capture.clone())
            .with_system_config(config.system.clone())
            .with_event_bus(Arc::clone(&event_bus))
            .build()?);
        
        #[cfg(feature = "streaming")]
        let stream_server = Some(StreamServer::new(
            config.stream.clone(),
            Arc::clone(&ring_buffer),
            Arc::clone(&event_bus),
            config.camera.fps,
        ));
        
        // Initialize keyboard input handler for debugging (disabled by default)
        let keyboard_handler = Some(KeyboardInputHandler::new(Arc::clone(&event_bus)));
        
        Ok(Self {
            config,
            event_bus,
            ring_buffer,
            camera_integration,
            analyzer_integration,
            display_integration,
            capture_integration,
            storage_integration,
            keyboard_handler,
            keyboard_enabled: false, // Disabled by default, enable via set_keyboard_enabled()
            #[cfg(feature = "streaming")]
            stream_server,
            component_states: Arc::new(Mutex::new(HashMap::new())),
            shutdown_sender: Some(shutdown_sender),
            shutdown_receiver: Some(shutdown_receiver),
            cancellation_token: CancellationToken::new(),
        })
    }
    
    /// Enable or disable the keyboard input handler
    pub fn set_keyboard_enabled(&mut self, enabled: bool) {
        self.keyboard_enabled = enabled;
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
        
        // Only register keyboard component if enabled
        if self.keyboard_enabled {
            states.insert("keyboard".to_string(), ComponentState::Stopped);
        }
        
        #[cfg(feature = "streaming")]
        states.insert("streaming".to_string(), ComponentState::Stopped);
        
        drop(states);
        
        info!("All components initialized successfully");
        Ok(())
    }
    
    /// Start all system components
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting Doorcam system");
        
        // Start camera integration first
        if let Some(camera_integration) = &self.camera_integration {
            self.set_component_state("camera", ComponentState::Starting).await;
            
            camera_integration.start().await.map_err(|e| {
                error!("Failed to start camera integration: {}", e);
                e
            })?;
            
            // Wait for frames to start flowing
            camera_integration.wait_for_frames(Duration::from_secs(5)).await.map_err(|e| {
                error!("Camera failed to produce frames: {}", e);
                e
            })?;
            
            self.set_component_state("camera", ComponentState::Running).await;
            info!("Camera integration started successfully");
        }
        
        // Start streaming server if enabled
        #[cfg(feature = "streaming")]
        if let Some(_stream_server) = &self.stream_server {
            self.set_component_state("streaming", ComponentState::Starting).await;
            
            // Use the ring buffer from camera integration if available
            let ring_buffer = if let Some(camera_integration) = &self.camera_integration {
                camera_integration.ring_buffer()
            } else {
                Arc::clone(&self.ring_buffer)
            };
            
            let server = StreamServer::new(
                self.config.stream.clone(),
                ring_buffer,
                Arc::clone(&self.event_bus),
                self.config.camera.fps,
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
        
        // Start analyzer integration
        if let Some(analyzer_integration) = &self.analyzer_integration {
            self.set_component_state("analyzer", ComponentState::Starting).await;
            
            let mut analyzer = analyzer_integration.lock().await;
            analyzer.start().await.map_err(|e| {
                error!("Failed to start analyzer integration: {}", e);
                e
            })?;
            
            self.set_component_state("analyzer", ComponentState::Running).await;
            info!("Analyzer integration started successfully");
        }
        
        // Start display integration
        if let Some(display_integration) = &self.display_integration {
            self.set_component_state("display", ComponentState::Starting).await;
            
            // Use the ring buffer from camera integration if available
            let ring_buffer = if let Some(camera_integration) = &self.camera_integration {
                camera_integration.ring_buffer()
            } else {
                Arc::clone(&self.ring_buffer)
            };
            
            display_integration.start(ring_buffer).await.map_err(|e| {
                error!("Failed to start display integration: {}", e);
                e
            })?;
            
            self.set_component_state("display", ComponentState::Running).await;
            info!("Display integration started successfully");
        }
        
        // Start capture integration
        if let Some(capture_integration) = &self.capture_integration {
            self.set_component_state("capture", ComponentState::Starting).await;
            
            capture_integration.start().await.map_err(|e| {
                error!("Failed to start capture integration: {}", e);
                e
            })?;
            
            self.set_component_state("capture", ComponentState::Running).await;
            info!("Capture integration started successfully");
        }
        
        // Start storage integration
        if let Some(storage_integration) = &self.storage_integration {
            self.set_component_state("storage", ComponentState::Starting).await;
            
            storage_integration.start().await.map_err(|e| {
                error!("Failed to start storage integration: {}", e);
                e
            })?;
            
            self.set_component_state("storage", ComponentState::Running).await;
            info!("Storage integration started successfully");
        }
        
        // Start keyboard input handler for debugging (only if enabled)
        if self.keyboard_enabled {
            if let Some(keyboard_handler) = &self.keyboard_handler {
                self.set_component_state("keyboard", ComponentState::Starting).await;
                
                keyboard_handler.start().await.map_err(|e| {
                    error!("Failed to start keyboard handler: {}", e);
                    e
                })?;
                
                self.set_component_state("keyboard", ComponentState::Running).await;
                info!("Keyboard input handler started - press SPACE to trigger motion");
            }
        }
        
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
        
        // Cancel all background tasks
        self.cancellation_token.cancel();
        
        let mut exit_code = 0;
        
        // Stop components in reverse dependency order
        if self.keyboard_enabled {
            if let Err(e) = self.stop_component("keyboard").await {
                error!("Error stopping keyboard: {}", e);
                exit_code = 1;
            }
        }
        
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
        
        match component {
            "camera" => {
                if let Some(camera_integration) = &self.camera_integration {
                    match timeout(Duration::from_secs(10), camera_integration.stop()).await {
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
                } else {
                    self.set_component_state(component, ComponentState::Stopped).await;
                    Ok(())
                }
            }
            "analyzer" => {
                if let Some(analyzer_integration) = &self.analyzer_integration {
                    let mut analyzer = analyzer_integration.lock().await;
                    match timeout(Duration::from_secs(10), analyzer.stop()).await {
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
                } else {
                    self.set_component_state(component, ComponentState::Stopped).await;
                    Ok(())
                }
            }
            "capture" => {
                if let Some(capture_integration) = &self.capture_integration {
                    match timeout(Duration::from_secs(5), capture_integration.stop()).await {
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
                } else {
                    self.set_component_state(component, ComponentState::Stopped).await;
                    Ok(())
                }
            }
            "display" => {
                if let Some(display_integration) = &self.display_integration {
                    match timeout(Duration::from_secs(5), display_integration.stop()).await {
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
                } else {
                    self.set_component_state(component, ComponentState::Stopped).await;
                    Ok(())
                }
            }
            "storage" => {
                if let Some(storage_integration) = &self.storage_integration {
                    match timeout(Duration::from_secs(5), storage_integration.stop()).await {
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
                } else {
                    self.set_component_state(component, ComponentState::Stopped).await;
                    Ok(())
                }
            }
            "keyboard" => {
                if let Some(keyboard_handler) = &self.keyboard_handler {
                    match timeout(Duration::from_secs(2), keyboard_handler.stop()).await {
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
                } else {
                    self.set_component_state(component, ComponentState::Stopped).await;
                    Ok(())
                }
            }
            _ => {
                // For other components, just simulate a graceful stop with timeout
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
                fps: 30,
                format: "MJPG".to_string(),
            },
            analyzer: crate::config::AnalyzerConfig {
                fps: 5,
                delta_threshold: 25,
                contour_minimum_area: 1000.0,
                hardware_acceleration: true,
                jpeg_decode_scale: 4,
            },
            capture: crate::config::CaptureConfig {
                preroll_seconds: 5,
                postroll_seconds: 10,
                path: "/tmp/doorcam_test".to_string(),
                timestamp_overlay: true,
                timestamp_font_path: "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".to_string(),
                timestamp_font_size: 24.0,
                video_encoding: false,
                keep_images: true,
                save_metadata: true,
                rotation: None,
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
                rotation: None,
            },
            display: crate::config::DisplayConfig {
                framebuffer_device: "/dev/fb0".to_string(),
                backlight_device: "/sys/class/backlight/rpi_backlight".to_string(),
                touch_device: "/dev/input/event0".to_string(),
                activation_period_seconds: 30,
                resolution: (800, 480),
                rotation: None,
                jpeg_decode_scale: 4,
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
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen { .. })) |
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::Configuration { .. })) => {
                println!("Camera hardware not available for testing - skipping orchestrator creation test");
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
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen { .. })) |
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::Configuration { .. })) => {
                println!("Camera hardware not available for testing - skipping component state test");
                return;
            }
            Err(e) => panic!("Unexpected orchestrator error: {}", e),
        };
        
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
        let orchestrator = match DoorcamOrchestrator::new(config).await {
            Ok(orchestrator) => orchestrator,
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen { .. })) |
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::Configuration { .. })) => {
                println!("Camera hardware not available for testing - skipping component state transitions test");
                return;
            }
            Err(e) => panic!("Unexpected orchestrator error: {}", e),
        };
        
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
        let orchestrator = match DoorcamOrchestrator::new(config).await {
            Ok(orchestrator) => Arc::new(orchestrator),
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen { .. })) |
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::Configuration { .. })) => {
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
        let _original_camera_index = config.camera.index;
        let _original_analyzer_fps = config.analyzer.fps;
        
        let orchestrator = match DoorcamOrchestrator::new(config).await {
            Ok(orchestrator) => orchestrator,
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen { .. })) |
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::Configuration { .. })) => {
                println!("Camera hardware not available for testing - skipping configuration access test");
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
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen { .. })) |
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::Configuration { .. })) => {
                println!("Camera hardware not available for testing - skipping error handling test");
                return;
            }
            Err(e) => panic!("Unexpected orchestrator error: {}", e),
        };
        
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