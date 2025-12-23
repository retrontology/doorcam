use crate::{
    capture::{CaptureStats, VideoCapture},
    config::{CaptureConfig, EventConfig},
    error::{DoorcamError, Result},
    events::EventBus,
    ring_buffer::RingBuffer,
};
use std::sync::Arc;
use tracing::{info, warn};

/// Integration layer for video capture system
pub struct VideoCaptureIntegration {
    capture: VideoCapture,
    config: CaptureConfig,
    event_config: EventConfig,
}

impl VideoCaptureIntegration {
    /// Create a new video capture integration
    pub fn new(
        config: CaptureConfig,
        event_config: EventConfig,
        event_bus: Arc<EventBus>,
        ring_buffer: Arc<RingBuffer>,
    ) -> Self {
        let capture =
            VideoCapture::new(config.clone(), event_config.clone(), event_bus, ring_buffer);

        Self {
            capture,
            config,
            event_config,
        }
    }

    /// Start the video capture integration
    pub async fn start(&self) -> Result<()> {
        info!("Starting video capture integration");

        // Validate configuration
        self.validate_config()?;

        // Start the capture system
        self.capture.start().await?;

        info!("Video capture integration started successfully");
        Ok(())
    }

    /// Stop the video capture integration
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping video capture integration");

        self.capture.stop().await?;

        info!("Video capture integration stopped");
        Ok(())
    }

    /// Get capture statistics
    pub async fn get_stats(&self) -> CaptureStats {
        self.capture.get_capture_stats().await
    }

    /// Get the capture configuration
    pub fn config(&self) -> &CaptureConfig {
        &self.config
    }

    /// Get the event timing configuration
    pub fn event_config(&self) -> &EventConfig {
        &self.event_config
    }

    /// Validate the capture configuration
    fn validate_config(&self) -> Result<()> {
        if self.event_config.preroll_seconds == 0 {
            return Err(DoorcamError::component(
                "video_capture_integration",
                "Preroll seconds must be greater than 0",
            ));
        }

        if self.event_config.postroll_seconds == 0 {
            return Err(DoorcamError::component(
                "video_capture_integration",
                "Postroll seconds must be greater than 0",
            ));
        }

        if self.config.path.is_empty() {
            return Err(DoorcamError::component(
                "video_capture_integration",
                "Capture path cannot be empty",
            ));
        }

        // Validate that we have at least one output format enabled
        if !self.config.keep_images && !self.config.video_encoding {
            warn!("Neither image saving nor video encoding is enabled - captures will only save metadata");
        }

        Ok(())
    }
}

/// Builder for video capture integration
pub struct VideoCaptureIntegrationBuilder {
    config: Option<CaptureConfig>,
    event_config: Option<EventConfig>,
    event_bus: Option<Arc<EventBus>>,
    ring_buffer: Option<Arc<RingBuffer>>,
}

impl VideoCaptureIntegrationBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            config: None,
            event_config: None,
            event_bus: None,
            ring_buffer: None,
        }
    }

    /// Set the capture configuration
    pub fn config(mut self, config: CaptureConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set the event configuration
    pub fn event_config(mut self, event_config: EventConfig) -> Self {
        self.event_config = Some(event_config);
        self
    }

    /// Set the event bus
    pub fn event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Set the ring buffer
    pub fn ring_buffer(mut self, ring_buffer: Arc<RingBuffer>) -> Self {
        self.ring_buffer = Some(ring_buffer);
        self
    }

    /// Build the video capture integration
    pub fn build(self) -> Result<VideoCaptureIntegration> {
        let config = self.config.ok_or_else(|| {
            DoorcamError::component("video_capture_integration_builder", "Config is required")
        })?;

        let event_config = self.event_config.ok_or_else(|| {
            DoorcamError::component(
                "video_capture_integration_builder",
                "Event config is required",
            )
        })?;

        let event_bus = self.event_bus.ok_or_else(|| {
            DoorcamError::component("video_capture_integration_builder", "Event bus is required")
        })?;

        let ring_buffer = self.ring_buffer.ok_or_else(|| {
            DoorcamError::component(
                "video_capture_integration_builder",
                "Ring buffer is required",
            )
        })?;

        Ok(VideoCaptureIntegration::new(
            config,
            event_config,
            event_bus,
            ring_buffer,
        ))
    }
}

impl Default for VideoCaptureIntegrationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{CaptureConfig, EventConfig},
        events::EventBus,
        ring_buffer::RingBuffer,
    };
    use std::time::Duration;

    fn create_test_configs() -> (CaptureConfig, EventConfig) {
        let capture_config = CaptureConfig {
            path: "./test_captures".to_string(),
            timestamp_overlay: true,
            timestamp_font_path: "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".to_string(),
            timestamp_font_size: 24.0,
            video_encoding: false,
            keep_images: true,
            save_metadata: true,
            rotation: None,
        };

        let event_config = EventConfig {
            preroll_seconds: 5,
            postroll_seconds: 10,
        };

        (capture_config, event_config)
    }

    #[tokio::test]
    async fn test_integration_creation() {
        let (capture_config, event_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new(10));
        let ring_buffer = Arc::new(RingBuffer::new(50, Duration::from_secs(5)));

        let integration =
            VideoCaptureIntegration::new(capture_config, event_config, event_bus, ring_buffer);

        assert_eq!(integration.event_config().preroll_seconds, 5);
        assert_eq!(integration.event_config().postroll_seconds, 10);
    }

    #[tokio::test]
    async fn test_builder_pattern() {
        let (capture_config, event_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new(10));
        let ring_buffer = Arc::new(RingBuffer::new(50, Duration::from_secs(5)));

        let integration = VideoCaptureIntegrationBuilder::new()
            .config(capture_config)
            .event_config(event_config.clone())
            .event_bus(event_bus)
            .ring_buffer(ring_buffer)
            .build()
            .unwrap();

        assert_eq!(integration.event_config().preroll_seconds, 5);
    }

    #[tokio::test]
    async fn test_builder_validation() {
        // Missing config
        let result = VideoCaptureIntegrationBuilder::new().build();
        assert!(result.is_err());

        // Missing event bus
        let (capture_config, event_config) = create_test_configs();
        let result = VideoCaptureIntegrationBuilder::new()
            .config(capture_config)
            .event_config(event_config)
            .build();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_validation() {
        let event_bus = Arc::new(EventBus::new(10));
        let ring_buffer = Arc::new(RingBuffer::new(50, Duration::from_secs(5)));

        // Invalid config - zero preroll
        let (capture_config, mut event_config) = create_test_configs();
        event_config.preroll_seconds = 0;

        let integration = VideoCaptureIntegration::new(
            capture_config.clone(),
            event_config,
            event_bus.clone(),
            ring_buffer.clone(),
        );
        assert!(integration.validate_config().is_err());

        // Invalid config - zero postroll
        let (capture_config, mut event_config) = create_test_configs();
        event_config.postroll_seconds = 0;

        let integration = VideoCaptureIntegration::new(
            capture_config.clone(),
            event_config,
            event_bus.clone(),
            ring_buffer.clone(),
        );
        assert!(integration.validate_config().is_err());

        // Invalid config - empty path
        let (mut invalid_capture_config, event_config) = create_test_configs();
        invalid_capture_config.path = String::new();

        let integration = VideoCaptureIntegration::new(
            invalid_capture_config,
            event_config,
            event_bus,
            ring_buffer,
        );
        assert!(integration.validate_config().is_err());
    }

    #[tokio::test]
    async fn test_stats() {
        let (capture_config, event_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new(10));
        let ring_buffer = Arc::new(RingBuffer::new(50, Duration::from_secs(5)));

        let integration =
            VideoCaptureIntegration::new(capture_config, event_config, event_bus, ring_buffer);

        let stats = integration.get_stats().await;
        assert_eq!(stats.active_captures, 0);
        assert_eq!(stats.total_active_frames, 0);
    }
}
