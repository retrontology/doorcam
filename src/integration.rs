use crate::camera::{CameraInterface, CameraInterfaceBuilder};
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
        
        // Calculate ring buffer capacity based on preroll duration and camera FPS
        let preroll_duration = Duration::from_secs(config.capture.preroll_seconds as u64);
        let estimated_capacity = (config.camera.max_fps as u64 * config.capture.preroll_seconds as u64 * 2) as usize;
        let capacity = estimated_capacity.max(config.system.ring_buffer_capacity);
        
        debug!(
            "Ring buffer capacity: {} frames (estimated: {}, configured: {})",
            capacity, estimated_capacity, config.system.ring_buffer_capacity
        );
        
        // Create ring buffer
        let ring_buffer = RingBufferBuilder::new()
            .capacity(capacity)
            .preroll_duration(preroll_duration)
            .build()?;
        
        // Create camera interface
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
        
        // Test camera connection first
        self.camera.test_connection().await?;
        
        // Start camera capture with ring buffer integration
        self.camera.start_capture(Arc::clone(&self.ring_buffer)).await?;
        
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
        info!("Waiting for camera frames (timeout: {:?})", timeout_duration);
        
        let result = timeout(timeout_duration, async {
            loop {
                if let Some(frame) = self.ring_buffer.get_latest_frame().await {
                    info!("First frame received: {} ({}x{})", frame.id, frame.width, frame.height);
                    return Ok(());
                }
                sleep(Duration::from_millis(50)).await;
            }
        }).await;
        
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
        
        // Check camera status
        if !self.camera.is_capturing() {
            issues.push("Camera is not capturing".to_string());
        }
        
        // Check if we're getting frames
        if self.ring_buffer.get_latest_frame().await.is_none() {
            issues.push("No frames in ring buffer".to_string());
        }
        
        // Check ring buffer utilization
        let stats = self.ring_buffer.stats();
        if stats.utilization_percent > 90 {
            warnings.push(format!("High ring buffer utilization: {}%", stats.utilization_percent));
        }
        
        // Check for buffer overruns
        if stats.buffer_overruns > 0 {
            warnings.push(format!("Buffer overruns detected: {}", stats.buffer_overruns));
        }
        
        // Test camera connection
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
        
        // Stop current capture
        if let Err(e) = self.camera.stop_capture().await {
            error!("Error stopping camera capture: {}", e);
        }
        
        // Wait a bit before restarting
        sleep(Duration::from_millis(500)).await;
        
        // Clear ring buffer to start fresh
        self.ring_buffer.clear().await;
        
        // Restart capture
        self.camera.start_capture(Arc::clone(&self.ring_buffer)).await?;
        
        info!("Camera capture restarted successfully");
        Ok(())
    }
}

/// Status information for the integration
#[derive(Debug, Clone)]
pub struct IntegrationStatus {
    pub camera_capturing: bool,
    pub camera_frame_count: u64,
    pub ring_buffer_capacity: usize,
    pub ring_buffer_frame_count: usize,
    pub ring_buffer_utilization: u64,
    pub frames_pushed: u64,
    pub frames_retrieved: u64,
    pub buffer_overruns: u64,
}

/// Health check result
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub status: HealthStatus,
    pub issues: Vec<String>,
    pub warnings: Vec<String>,
}

/// Health status enumeration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Warning,
    Unhealthy,
}

/// Builder for camera-ring buffer integration
pub struct CameraRingBufferIntegrationBuilder {
    config: Option<DoorcamConfig>,
}

impl CameraRingBufferIntegrationBuilder {
    /// Create a new integration builder
    pub fn new() -> Self {
        Self { config: None }
    }
    
    /// Set the configuration
    pub fn config(mut self, config: DoorcamConfig) -> Self {
        self.config = Some(config);
        self
    }
    
    /// Build the integration
    pub async fn build(self) -> Result<CameraRingBufferIntegration> {
        let config = self.config.ok_or_else(|| {
            DoorcamError::system("Configuration must be specified")
        })?;
        
        CameraRingBufferIntegration::new(config).await
    }
}

impl Default for CameraRingBufferIntegrationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AnalyzerConfig, CaptureConfig, DisplayConfig, StreamConfig, SystemConfig};
    use crate::frame::Rotation;
    
    fn create_test_config() -> DoorcamConfig {
        DoorcamConfig {
            camera: CameraConfig {
                index: 0,
                resolution: (640, 480),
                max_fps: 30,
                format: "MJPG".to_string(),
                rotation: None,
            },
            analyzer: AnalyzerConfig {
                max_fps: 5,
                delta_threshold: 25,
                contour_minimum_area: 1000.0,
            },
            capture: CaptureConfig {
                preroll_seconds: 5,
                postroll_seconds: 10,
                path: "./captures".to_string(),
                timestamp_overlay: true,
                video_encoding: false,
                keep_images: true,
            },
            stream: StreamConfig {
                ip: "0.0.0.0".to_string(),
                port: 8080,
            },
            display: DisplayConfig {
                framebuffer_device: "/dev/fb0".to_string(),
                backlight_device: "/sys/class/backlight/rpi_backlight/brightness".to_string(),
                touch_device: "/dev/input/event0".to_string(),
                activation_period_seconds: 30,
                rotation: None,
            },
            system: SystemConfig {
                trim_old: true,
                retention_days: 7,
                ring_buffer_capacity: 150,
                event_bus_capacity: 100,
            },
        }
    }
    
    #[tokio::test]
    async fn test_integration_creation() {
        let config = create_test_config();
        let integration = CameraRingBufferIntegration::new(config).await;
        
        assert!(integration.is_ok());
        
        let integration = integration.unwrap();
        assert!(!integration.camera().is_capturing());
        assert_eq!(integration.ring_buffer().capacity(), 300); // 30fps * 5s * 2
    }
    
    #[tokio::test]
    async fn test_integration_builder() {
        let config = create_test_config();
        let integration = CameraRingBufferIntegrationBuilder::new()
            .config(config)
            .build()
            .await;
        
        assert!(integration.is_ok());
    }
    
    #[tokio::test]
    async fn test_integration_start_stop() {
        let config = create_test_config();
        let integration = CameraRingBufferIntegration::new(config).await.unwrap();
        
        // Start integration with timeout
        let start_result = tokio::time::timeout(
            Duration::from_millis(200),
            integration.start()
        ).await;
        assert!(start_result.is_ok());
        assert!(start_result.unwrap().is_ok());
        assert!(integration.camera().is_capturing());
        
        // Wait for frames (with short timeout for mock mode)
        let result = integration.wait_for_frames(Duration::from_millis(500)).await;
        assert!(result.is_ok());
        
        // Check status
        let status = integration.get_status().await;
        assert!(status.camera_capturing);
        assert!(status.frames_pushed > 0);
        
        // Stop integration with timeout
        let stop_result = tokio::time::timeout(
            Duration::from_millis(200),
            integration.stop()
        ).await;
        assert!(stop_result.is_ok());
        assert!(stop_result.unwrap().is_ok());
        assert!(!integration.camera().is_capturing());
    }
    
    #[tokio::test]
    async fn test_health_check() {
        let config = create_test_config();
        let integration = CameraRingBufferIntegration::new(config).await.unwrap();
        
        // Health check before starting (should be unhealthy)
        let health = integration.health_check().await.unwrap();
        assert_eq!(health.status, HealthStatus::Unhealthy);
        assert!(!health.issues.is_empty());
        
        // Start and check again with timeout
        let start_result = tokio::time::timeout(
            Duration::from_millis(200),
            integration.start()
        ).await;
        assert!(start_result.is_ok());
        assert!(start_result.unwrap().is_ok());
        
        // Wait for frames
        let _ = integration.wait_for_frames(Duration::from_millis(200)).await;
        
        let health = integration.health_check().await.unwrap();
        // Should be healthy or have warnings (depending on timing)
        assert!(health.status == HealthStatus::Healthy || health.status == HealthStatus::Warning);
    }
    
    #[tokio::test]
    async fn test_restart_capture() {
        let config = create_test_config();
        let integration = CameraRingBufferIntegration::new(config).await.unwrap();
        
        // Start capture with timeout
        let start_result = tokio::time::timeout(
            Duration::from_millis(200),
            integration.start()
        ).await;
        assert!(start_result.is_ok());
        assert!(start_result.unwrap().is_ok());
        assert!(integration.camera().is_capturing());
        
        // Restart capture with timeout
        let restart_result = tokio::time::timeout(
            Duration::from_secs(1),
            integration.restart_capture()
        ).await;
        assert!(restart_result.is_ok());
        assert!(restart_result.unwrap().is_ok());
        assert!(integration.camera().is_capturing());
    }
}