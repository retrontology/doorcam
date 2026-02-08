use crate::{
    config::StreamConfig,
    error::{DoorcamError, Result, StreamError},
    events::EventBus,
    ring_buffer::RingBuffer,
};
use axum::{routing::get, Router};
use std::sync::Arc;
use tokio::time::Duration;
use tracing::info;

use super::handlers::{health_handler, mjpeg_stream_handler, stream_page_handler};

/// Shared state for the Axum server
#[derive(Clone)]
pub struct ServerState {
    pub(crate) ring_buffer: Arc<RingBuffer>,
    pub(crate) event_bus: Arc<EventBus>,
    pub(crate) target_frame_interval: Duration,
    pub(crate) stream_rotation: Option<crate::config::Rotation>,
}

/// MJPEG streaming server that serves camera frames over HTTP
pub struct StreamServer {
    pub(crate) config: StreamConfig,
    pub(crate) ring_buffer: Arc<RingBuffer>,
    pub(crate) event_bus: Arc<EventBus>,
    pub(crate) target_frame_interval: Duration,
}

impl StreamServer {
    /// Create a new streaming server
    pub fn new(
        config: StreamConfig,
        ring_buffer: Arc<RingBuffer>,
        event_bus: Arc<EventBus>,
        target_fps: u32,
    ) -> Self {
        let target_frame_interval = Duration::from_micros(1_000_000u64 / target_fps.max(1) as u64);

        Self {
            config,
            ring_buffer,
            event_bus,
            target_frame_interval,
        }
    }

    /// Start the HTTP server and begin serving MJPEG streams
    pub async fn start(&self) -> Result<()> {
        let state = ServerState {
            ring_buffer: Arc::clone(&self.ring_buffer),
            event_bus: Arc::clone(&self.event_bus),
            target_frame_interval: self.target_frame_interval,
            stream_rotation: self.config.rotation,
        };

        let app = Router::new()
            .route("/", get(stream_page_handler))
            .route("/stream.mjpg", get(mjpeg_stream_handler))
            .route("/health", get(health_handler))
            .with_state(state);

        let addr = format!("{}:{}", self.config.ip, self.config.port);

        info!("Starting MJPEG streaming server on {}", addr);

        let listener =
            tokio::net::TcpListener::bind(&addr)
                .await
                .map_err(|e| StreamError::BindFailed {
                    address: addr.clone(),
                    source: e,
                })?;

        info!("MJPEG server listening on {}", addr);

        axum::serve(listener, app)
            .await
            .map_err(|e| StreamError::StartupFailed {
                details: format!("Server error: {}", e),
            })?;

        Ok(())
    }
}

/// Stream server builder for configuration
pub struct StreamServerBuilder {
    config: Option<StreamConfig>,
    ring_buffer: Option<Arc<RingBuffer>>,
    event_bus: Option<Arc<EventBus>>,
    target_fps: Option<u32>,
}

impl StreamServerBuilder {
    /// Create a new stream server builder
    pub fn new() -> Self {
        Self {
            config: None,
            ring_buffer: None,
            event_bus: None,
            target_fps: None,
        }
    }

    /// Set the stream configuration
    pub fn config(mut self, config: StreamConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set the ring buffer
    pub fn ring_buffer(mut self, ring_buffer: Arc<RingBuffer>) -> Self {
        self.ring_buffer = Some(ring_buffer);
        self
    }

    /// Set the event bus
    pub fn event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Set the target FPS for pacing
    pub fn target_fps(mut self, fps: u32) -> Self {
        self.target_fps = Some(fps);
        self
    }

    /// Build the stream server
    pub fn build(self) -> Result<StreamServer> {
        let config = self.config.ok_or_else(|| {
            DoorcamError::Stream(StreamError::StartupFailed {
                details: "Stream configuration is required".to_string(),
            })
        })?;

        let ring_buffer = self.ring_buffer.ok_or_else(|| {
            DoorcamError::Stream(StreamError::StartupFailed {
                details: "Ring buffer is required".to_string(),
            })
        })?;

        let event_bus = self.event_bus.ok_or_else(|| {
            DoorcamError::Stream(StreamError::StartupFailed {
                details: "Event bus is required".to_string(),
            })
        })?;

        let target_fps = self.target_fps.ok_or_else(|| {
            DoorcamError::Stream(StreamError::StartupFailed {
                details: "Target FPS is required".to_string(),
            })
        })?;

        Ok(StreamServer::new(
            config,
            ring_buffer,
            event_bus,
            target_fps,
        ))
    }
}

impl Default for StreamServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
