use crate::config::{CaptureConfig, EventConfig};
use crate::error::{DoorcamError, Result};
use serde::{Deserialize, Serialize};
use serde_json;
use std::path::PathBuf;
use std::time::SystemTime;
use tokio::fs;
use tracing::debug;

/// Statistics about the video capture system
#[derive(Debug, Clone)]
pub struct CaptureStats {
    pub active_captures: usize,
    pub total_active_frames: usize,
}

/// Metadata for a completed capture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureMetadata {
    pub event_id: String,
    pub start_time: SystemTime,
    pub motion_detected_time: SystemTime,
    pub preroll_frame_count: usize,
    pub postroll_frame_count: usize,
    pub total_frame_count: usize,
    pub config: CaptureConfig,
    pub event: EventConfig,
}

pub(crate) async fn save_metadata(
    metadata: &CaptureMetadata,
    event_id: &str,
    capture_path: &str,
) -> Result<()> {
    let metadata_json = serde_json::to_string_pretty(&metadata).map_err(|e| {
        DoorcamError::component(
            "video_capture",
            &format!("Failed to serialize metadata: {}", e),
        )
    })?;

    let metadata_dir = PathBuf::from(capture_path).join("metadata");
    fs::create_dir_all(&metadata_dir).await.map_err(|e| {
        DoorcamError::component(
            "video_capture",
            &format!("Failed to create metadata directory: {}", e),
        )
    })?;

    let metadata_path = metadata_dir.join(format!("{}.json", event_id));
    fs::write(&metadata_path, metadata_json)
        .await
        .map_err(|e| {
            DoorcamError::component(
                "video_capture",
                &format!("Failed to write metadata file: {}", e),
            )
        })?;

    debug!("Saved metadata to {}", metadata_path.display());
    Ok(())
}
