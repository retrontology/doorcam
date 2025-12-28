use super::{metadata::CaptureStats, VideoCapture};
use crate::config::{CaptureConfig, EventConfig};
use crate::error::{DoorcamError, Result};
use crate::events::EventBus;
use crate::ring_buffer::RingBuffer;
use std::sync::Arc;
use tracing::warn;

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
        self.validate_config()?;
        self.capture.start().await?;
        Ok(())
    }

    /// Stop the video capture integration
    pub async fn stop(&self) -> Result<()> {
        self.capture.stop().await
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

        if !self.config.keep_images && !self.config.video_encoding {
            warn!(
                "Neither image saving nor video encoding is enabled - captures will only save metadata"
            );
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
    pub fn new() -> Self {
        Self {
            config: None,
            event_config: None,
            event_bus: None,
            ring_buffer: None,
        }
    }

    pub fn config(mut self, config: CaptureConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn event_config(mut self, event_config: EventConfig) -> Self {
        self.event_config = Some(event_config);
        self
    }

    pub fn event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    pub fn ring_buffer(mut self, ring_buffer: Arc<RingBuffer>) -> Self {
        self.ring_buffer = Some(ring_buffer);
        self
    }

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
