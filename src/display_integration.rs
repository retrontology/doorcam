use crate::config::DisplayConfig;
use crate::display::DisplayController;
use crate::touch::{TouchInputHandler, MockTouchInputHandler};
use crate::events::{DoorcamEvent, EventBus, EventReceiver, EventFilter};
use crate::frame::FrameData;
use crate::ring_buffer::RingBuffer;
use crate::error::{DoorcamError, Result};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::time::{interval, sleep};
use tracing::{debug, error, info, warn};

/// Integration component that manages display controller and touch input together
pub struct DisplayIntegration {
    display_controller: DisplayController,
    config: DisplayConfig,
    event_bus: Arc<EventBus>,
    use_mock_touch: bool,
}

impl DisplayIntegration {
    /// Create a new display integration
    pub async fn new(
        config: DisplayConfig,
        event_bus: Arc<EventBus>,
    ) -> Result<Self> {
        info!("Initializing display integration");

        let display_controller = DisplayController::new(config.clone()).await?;

        Ok(Self {
            display_controller,
            config,
            event_bus,
            use_mock_touch: false,
        })
    }

    /// Enable mock touch input for testing
    pub fn with_mock_touch(mut self) -> Self {
        self.use_mock_touch = true;
        self
    }

    /// Start the display integration with all components
    pub async fn start(
        &self,
        ring_buffer: Arc<RingBuffer>,
    ) -> Result<()> {
        info!("Starting display integration");

        // Start display controller
        self.display_controller.start(Arc::clone(&self.event_bus)).await?;

        // Start touch input handler
        if self.use_mock_touch {
            info!("Using mock touch input handler");
            let mock_handler = MockTouchInputHandler::new(Arc::clone(&self.event_bus));
            mock_handler.start().await?;
        } else {
            info!("Using real touch input handler");
            let touch_handler = TouchInputHandler::new(&self.config, Arc::clone(&self.event_bus));
            touch_handler.start().await?;
        }

        // Start frame rendering loop
        self.start_frame_rendering(ring_buffer).await?;

        info!("Display integration started successfully");
        Ok(())
    }

    /// Start the frame rendering loop
    async fn start_frame_rendering(&self, ring_buffer: Arc<RingBuffer>) -> Result<()> {
        let display_controller = self.display_controller.clone();
        let event_bus = Arc::clone(&self.event_bus);

        tokio::spawn(async move {
            let mut render_interval = interval(Duration::from_millis(33)); // ~30 FPS

            loop {
                render_interval.tick().await;

                // Only render if display is active
                if display_controller.is_active() {
                    if let Some(frame) = ring_buffer.get_latest_frame().await {
                        if let Err(e) = display_controller.render_frame(&frame).await {
                            error!("Failed to render frame to display: {}", e);
                            
                            // Publish error event
                            let _ = event_bus.publish(DoorcamEvent::SystemError {
                                component: "display_rendering".to_string(),
                                error: e.to_string(),
                            }).await;
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Get display controller reference
    pub fn display_controller(&self) -> &DisplayController {
        &self.display_controller
    }

    /// Check if display is currently active
    pub fn is_display_active(&self) -> bool {
        self.display_controller.is_active()
    }

    /// Manually activate display
    pub async fn activate_display(&self) -> Result<()> {
        self.event_bus.publish(DoorcamEvent::DisplayActivate {
            timestamp: SystemTime::now(),
            duration_seconds: self.config.activation_period_seconds,
        }).await.map_err(|e| DoorcamError::component("display_integration".to_string(), e.to_string()))?;
        
        Ok(())
    }

    /// Manually deactivate display
    pub async fn deactivate_display(&self) -> Result<()> {
        self.event_bus.publish(DoorcamEvent::DisplayDeactivate {
            timestamp: SystemTime::now(),
        }).await.map_err(|e| DoorcamError::component("display_integration".to_string(), e.to_string()))?;
        
        Ok(())
    }
}

/// Builder for display integration with configuration options
pub struct DisplayIntegrationBuilder {
    config: Option<DisplayConfig>,
    event_bus: Option<Arc<EventBus>>,
    use_mock_touch: bool,
    render_fps: u32,
}

impl DisplayIntegrationBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            config: None,
            event_bus: None,
            use_mock_touch: false,
            render_fps: 30,
        }
    }

    /// Set display configuration
    pub fn with_config(mut self, config: DisplayConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set event bus
    pub fn with_event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Enable mock touch input
    pub fn with_mock_touch(mut self, enabled: bool) -> Self {
        self.use_mock_touch = enabled;
        self
    }

    /// Set rendering frame rate
    pub fn with_render_fps(mut self, fps: u32) -> Self {
        self.render_fps = fps;
        self
    }

    /// Build the display integration
    pub async fn build(self) -> Result<DisplayIntegration> {
        let config = self.config.ok_or_else(|| {
            DoorcamError::component("display_integration_builder".to_string(), "Display config is required".to_string())
        })?;

        let event_bus = self.event_bus.ok_or_else(|| {
            DoorcamError::component("display_integration_builder".to_string(), "Event bus is required".to_string())
        })?;

        let mut integration = DisplayIntegration::new(config, event_bus).await?;
        
        if self.use_mock_touch {
            integration = integration.with_mock_touch();
        }

        Ok(integration)
    }
}

impl Default for DisplayIntegrationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Display statistics and monitoring
#[derive(Debug, Clone, Default)]
pub struct DisplayStats {
    pub frames_rendered: u64,
    pub render_errors: u64,
    pub touch_events: u64,
    pub activation_count: u64,
    pub total_active_time: Duration,
    pub last_activation: Option<SystemTime>,
    pub last_render: Option<SystemTime>,
}

impl DisplayStats {
    /// Record a frame render
    pub fn record_frame_render(&mut self) {
        self.frames_rendered += 1;
        self.last_render = Some(SystemTime::now());
    }

    /// Record a render error
    pub fn record_render_error(&mut self) {
        self.render_errors += 1;
    }

    /// Record a touch event
    pub fn record_touch_event(&mut self) {
        self.touch_events += 1;
    }

    /// Record display activation
    pub fn record_activation(&mut self) {
        self.activation_count += 1;
        self.last_activation = Some(SystemTime::now());
    }

    /// Get render success rate
    pub fn render_success_rate(&self) -> f64 {
        if self.frames_rendered == 0 {
            0.0
        } else {
            (self.frames_rendered - self.render_errors) as f64 / self.frames_rendered as f64
        }
    }

    /// Reset all statistics
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Display integration with statistics tracking
pub struct DisplayIntegrationWithStats {
    integration: DisplayIntegration,
    stats: Arc<tokio::sync::RwLock<DisplayStats>>,
}

impl DisplayIntegrationWithStats {
    /// Create a new display integration with statistics
    pub async fn new(
        config: DisplayConfig,
        event_bus: Arc<EventBus>,
    ) -> Result<Self> {
        let integration = DisplayIntegration::new(config, event_bus).await?;
        let stats = Arc::new(tokio::sync::RwLock::new(DisplayStats::default()));

        Ok(Self {
            integration,
            stats,
        })
    }

    /// Start with statistics tracking
    pub async fn start(&self, ring_buffer: Arc<RingBuffer>) -> Result<()> {
        // Start the base integration
        self.integration.start(ring_buffer).await?;

        // Start statistics collection
        self.start_stats_collection().await?;

        Ok(())
    }

    /// Start statistics collection
    async fn start_stats_collection(&self) -> Result<()> {
        let event_bus = Arc::clone(&self.integration.event_bus);
        let stats = Arc::clone(&self.stats);

        // Subscribe to events for statistics
        let receiver = event_bus.subscribe();
        let filter = EventFilter::EventTypes(vec![
            "touch_detected",
            "display_activate",
            "system_error",
        ]);
        let mut event_receiver = EventReceiver::new(receiver, filter, "display_stats".to_string());

        tokio::spawn(async move {
            loop {
                match event_receiver.recv().await {
                    Ok(event) => {
                        let mut stats_guard = stats.write().await;
                        match event {
                            DoorcamEvent::TouchDetected { .. } => {
                                stats_guard.record_touch_event();
                            }
                            DoorcamEvent::DisplayActivate { .. } => {
                                stats_guard.record_activation();
                            }
                            DoorcamEvent::SystemError { component, .. } => {
                                if component.contains("display") || component.contains("render") {
                                    stats_guard.record_render_error();
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        error!("Error receiving events for display stats: {}", e);
                        sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });

        Ok(())
    }

    /// Get current statistics
    pub async fn get_stats(&self) -> DisplayStats {
        self.stats.read().await.clone()
    }

    /// Reset statistics
    pub async fn reset_stats(&self) {
        self.stats.write().await.reset();
    }

    /// Get display integration reference
    pub fn integration(&self) -> &DisplayIntegration {
        &self.integration
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DisplayConfig;
    use crate::ring_buffer::RingBufferBuilder;
    use std::time::SystemTime;
    use tokio::time::{timeout, Duration};

    fn create_test_config() -> DisplayConfig {
        DisplayConfig {
            framebuffer_device: "/tmp/test_fb".to_string(),
            backlight_device: "/tmp/test_backlight".to_string(),
            touch_device: "/tmp/test_touch".to_string(),
            activation_period_seconds: 5,
            rotation: None,
        }
    }

    #[tokio::test]
    async fn test_display_integration_creation() {
        let config = create_test_config();
        let event_bus = Arc::new(EventBus::new(10));
        
        let integration = DisplayIntegration::new(config, event_bus).await;
        assert!(integration.is_ok());
    }

    #[tokio::test]
    async fn test_display_integration_builder() {
        let config = create_test_config();
        let event_bus = Arc::new(EventBus::new(10));
        
        let integration = DisplayIntegrationBuilder::new()
            .with_config(config)
            .with_event_bus(event_bus)
            .with_mock_touch(true)
            .with_render_fps(60)
            .build()
            .await;
        
        assert!(integration.is_ok());
    }

    #[tokio::test]
    async fn test_display_activation_events() {
        let config = create_test_config();
        let event_bus = Arc::new(EventBus::new(10));
        let mut receiver = event_bus.subscribe();
        
        let integration = DisplayIntegration::new(config, Arc::clone(&event_bus)).await.unwrap();
        
        // Activate display
        integration.activate_display().await.unwrap();
        
        // Should receive activation event
        let event = timeout(Duration::from_millis(100), receiver.recv()).await.unwrap().unwrap();
        match event {
            DoorcamEvent::DisplayActivate { .. } => {
                // Success
            }
            _ => panic!("Expected DisplayActivate event"),
        }
    }

    #[tokio::test]
    async fn test_display_stats() {
        let mut stats = DisplayStats::default();
        
        stats.record_frame_render();
        stats.record_touch_event();
        stats.record_activation();
        
        assert_eq!(stats.frames_rendered, 1);
        assert_eq!(stats.touch_events, 1);
        assert_eq!(stats.activation_count, 1);
        assert_eq!(stats.render_success_rate(), 1.0);
        
        stats.record_render_error();
        assert!(stats.render_success_rate() < 1.0);
    }

    #[tokio::test]
    async fn test_display_integration_with_stats() {
        let config = create_test_config();
        let event_bus = Arc::new(EventBus::new(10));
        
        let integration_with_stats = DisplayIntegrationWithStats::new(config, event_bus).await;
        assert!(integration_with_stats.is_ok());
        
        let integration = integration_with_stats.unwrap();
        let stats = integration.get_stats().await;
        
        // Initial stats should be zero
        assert_eq!(stats.frames_rendered, 0);
        assert_eq!(stats.touch_events, 0);
        assert_eq!(stats.activation_count, 0);
    }

    #[tokio::test]
    async fn test_mock_touch_integration() {
        let config = create_test_config();
        let event_bus = Arc::new(EventBus::new(10));
        let ring_buffer = Arc::new(
            RingBufferBuilder::new()
                .capacity(10)
                .preroll_duration(Duration::from_secs(5))
                .build()
                .unwrap()
        );
        
        let integration = DisplayIntegration::new(config, event_bus)
            .await
            .unwrap()
            .with_mock_touch();
        
        // Test that start() returns Ok without actually running the infinite loops
        // We use a timeout to ensure the test doesn't hang
        let start_result = timeout(Duration::from_millis(100), async {
            integration.start(ring_buffer).await
        }).await;
        
        // The start should complete quickly (just spawning tasks, not running them)
        assert!(start_result.is_ok());
        assert!(start_result.unwrap().is_ok());
    }
}