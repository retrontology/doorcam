use super::builder::{calculate_ring_buffer_capacity, CameraInterfaceBuilder};
use super::health::{HealthCheckResult, HealthStatus, IntegrationStatus};
use super::interface::CameraInterface;
use crate::config::DoorcamConfig;
use crate::error::{DoorcamError, Result};
use crate::ring_buffer::{RingBuffer, RingBufferBuilder};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, warn};

/// Integration manager for camera and ring buffer coordination
pub struct CameraRingBufferIntegration {
    camera: CameraInterface,
    ring_buffer: Arc<RingBuffer>,
    _config: DoorcamConfig,
}

impl CameraRingBufferIntegration {
    /// Create a new integration instance
    pub async fn new(config: DoorcamConfig) -> Result<Self> {
        info!("Initializing camera-ring buffer integration");

        let preroll_duration = Duration::from_secs(config.event.preroll_seconds as u64);
        let capacity =
            calculate_ring_buffer_capacity(config.camera.fps, config.event.preroll_seconds);

        debug!(
            "Ring buffer capacity: {} frames (fps: {}, preroll: {}, safety_factor: 2)",
            capacity, config.camera.fps, config.event.preroll_seconds
        );

        let ring_buffer = RingBufferBuilder::new()
            .capacity(capacity)
            .preroll_duration(preroll_duration)
            .build()?;

        let camera = CameraInterfaceBuilder::new()
            .config(config.camera.clone())
            .build()
            .await?;

        Ok(Self {
            camera,
            ring_buffer: Arc::new(ring_buffer),
            _config: config,
        })
    }

    /// Start the integrated camera capture system
    pub async fn start(&self) -> Result<()> {
        info!("Starting integrated camera capture system");

        self.camera.test_connection().await?;
        self.camera
            .start_capture(Arc::clone(&self.ring_buffer))
            .await?;

        info!("Camera capture system started successfully");
        Ok(())
    }

    /// Stop the integrated camera capture system
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping integrated camera capture system");
        self.camera.stop_capture().await?;
        info!("Camera capture system stopped");
        Ok(())
    }

    /// Get the ring buffer for external access
    pub fn ring_buffer(&self) -> Arc<RingBuffer> {
        Arc::clone(&self.ring_buffer)
    }

    /// Get camera interface for external access
    pub fn camera(&self) -> &CameraInterface {
        &self.camera
    }

    /// Wait for camera to start producing frames
    pub async fn wait_for_frames(&self, timeout_duration: Duration) -> Result<()> {
        info!(
            "Waiting for camera frames (timeout: {:?})",
            timeout_duration
        );

        let result = timeout(timeout_duration, async {
            loop {
                if let Some(frame) = self.ring_buffer.get_latest_frame().await {
                    info!(
                        "First frame received: {} ({}x{})",
                        frame.id, frame.width, frame.height
                    );
                    return Ok(());
                }
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await;

        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(DoorcamError::system("Timeout waiting for camera frames")),
        }
    }

    /// Get system status and statistics
    pub async fn get_status(&self) -> IntegrationStatus {
        let ring_buffer_stats = self.ring_buffer.stats();
        let frame_count = self.ring_buffer.approximate_frame_count().await;

        IntegrationStatus {
            camera_capturing: self.camera.is_capturing(),
            camera_frame_count: self.camera.frame_count(),
            ring_buffer_capacity: self.ring_buffer.capacity(),
            ring_buffer_frame_count: frame_count,
            ring_buffer_utilization: ring_buffer_stats.utilization_percent,
            frames_pushed: ring_buffer_stats.frames_pushed,
            frames_retrieved: ring_buffer_stats.frames_retrieved,
            buffer_overruns: ring_buffer_stats.buffer_overruns,
        }
    }

    /// Perform health check on the integration
    pub async fn health_check(&self) -> Result<HealthCheckResult> {
        let mut issues = Vec::new();
        let mut warnings = Vec::new();

        if !self.camera.is_capturing() {
            issues.push("Camera is not capturing".to_string());
        }

        if self.ring_buffer.get_latest_frame().await.is_none() {
            issues.push("No frames in ring buffer".to_string());
        }

        let stats = self.ring_buffer.stats();
        if stats.utilization_percent > 90 {
            warnings.push(format!(
                "High ring buffer utilization: {}%",
                stats.utilization_percent
            ));
        }

        if stats.buffer_overruns > 0 {
            warnings.push(format!(
                "Buffer overruns detected: {}",
                stats.buffer_overruns
            ));
        }

        if let Err(e) = self.camera.test_connection().await {
            issues.push(format!("Camera connection test failed: {}", e));
        }

        let status = if issues.is_empty() {
            if warnings.is_empty() {
                HealthStatus::Healthy
            } else {
                HealthStatus::Warning
            }
        } else {
            HealthStatus::Unhealthy
        };

        Ok(HealthCheckResult {
            status,
            issues,
            warnings,
        })
    }

    /// Restart the camera capture with error recovery
    pub async fn restart_capture(&self) -> Result<()> {
        warn!("Restarting camera capture");

        if let Err(e) = self.camera.stop_capture().await {
            error!("Error stopping camera capture: {}", e);
        }

        sleep(Duration::from_millis(500)).await;
        self.ring_buffer.clear().await;

        self.camera
            .start_capture(Arc::clone(&self.ring_buffer))
            .await?;

        info!("Camera capture restarted successfully");
        Ok(())
    }
}

/// Builder for camera-ring buffer integration
pub struct CameraRingBufferIntegrationBuilder {
    config: Option<DoorcamConfig>,
}

impl CameraRingBufferIntegrationBuilder {
    pub fn new() -> Self {
        Self { config: None }
    }

    pub fn config(mut self, config: DoorcamConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub async fn build(self) -> Result<CameraRingBufferIntegration> {
        let config = self
            .config
            .ok_or_else(|| DoorcamError::system("Configuration must be specified"))?;

        CameraRingBufferIntegration::new(config).await
    }
}

impl Default for CameraRingBufferIntegrationBuilder {
    fn default() -> Self {
        Self::new()
    }
}
