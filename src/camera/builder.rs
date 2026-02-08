use super::interface::CameraInterface;
use crate::config::CameraConfig;
use crate::error::{DoorcamError, Result};

/// Builder for GStreamer camera interface
pub struct CameraInterfaceBuilder {
    config: Option<CameraConfig>,
}

impl CameraInterfaceBuilder {
    pub fn new() -> Self {
        Self { config: None }
    }

    pub fn config(mut self, config: CameraConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub async fn build(self) -> Result<CameraInterface> {
        let config = self
            .config
            .ok_or_else(|| DoorcamError::system("Camera configuration must be specified"))?;

        CameraInterface::new(config).await
    }
}

impl Default for CameraInterfaceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate an appropriate ring buffer capacity from fps and preroll.
/// Uses a small safety factor to avoid overruns during bursty load.
pub fn calculate_ring_buffer_capacity(camera_fps: u32, preroll_seconds: u32) -> usize {
    let estimated = (camera_fps as u64 * preroll_seconds as u64 * 2).max(1);
    estimated as usize
}
