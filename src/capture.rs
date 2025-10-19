use crate::{
    config::CaptureConfig,
    error::{CaptureError, DoorcamError, Result},
    events::{DoorcamEvent, EventBus},
    frame::{FrameData, ProcessedFrame},
    ring_buffer::RingBuffer,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::fs;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Video capture system for motion-triggered recording
pub struct VideoCapture {
    config: CaptureConfig,
    event_bus: Arc<EventBus>,
    ring_buffer: Arc<RingBuffer>,
    active_captures: Arc<RwLock<Vec<ActiveCapture>>>,
}

/// Represents an active capture session
#[derive(Debug, Clone)]
struct ActiveCapture {
    event_id: String,
    start_time: SystemTime,
    motion_detected_time: SystemTime,
    preroll_frames: Vec<FrameData>,
    postroll_frames: Vec<FrameData>,
    _is_recording_postroll: bool,
}

impl VideoCapture {
    /// Create a new video capture system
    pub fn new(
        config: CaptureConfig,
        event_bus: Arc<EventBus>,
        ring_buffer: Arc<RingBuffer>,
    ) -> Self {
        Self {
            config,
            event_bus,
            ring_buffer,
            active_captures: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Start the video capture system
    pub async fn start(&self) -> Result<()> {
        info!("Starting video capture system");
        
        // Create capture directory if it doesn't exist
        let capture_path = PathBuf::from(&self.config.path);
        if !capture_path.exists() {
            std::fs::create_dir_all(&capture_path)
                .map_err(|e| CaptureError::DirectoryCreation { 
                    path: capture_path.display().to_string(), 
                    source: e 
                })?;
            info!("Created capture directory: {}", capture_path.display());
        }

        // Subscribe to motion detection events
        let mut event_receiver = self.event_bus.subscribe();
        let capture_system = Arc::new(self.clone());
        let capture_system_clone = Arc::clone(&capture_system);

        tokio::spawn(async move {
            while let Ok(event) = event_receiver.recv().await {
                if let DoorcamEvent::MotionDetected { contour_area, timestamp } = event {
                    debug!("Motion detected, starting capture (area: {:.2})", contour_area);
                    
                    if let Err(e) = capture_system.handle_motion_detected(timestamp).await {
                        error!("Failed to handle motion detection: {}", e);
                    }
                }
            }
        });

        // Start postroll monitoring task
        tokio::spawn(async move {
            capture_system_clone.monitor_postroll().await;
        });

        info!("Video capture system started successfully");
        Ok(())
    }

    /// Handle motion detection event by starting a new capture
    async fn handle_motion_detected(&self, motion_time: SystemTime) -> Result<()> {
        let event_id = Uuid::new_v4().to_string();
        
        debug!("Starting capture for event: {}", event_id);

        // Get preroll frames from ring buffer
        let preroll_frames = self.ring_buffer.get_preroll_frames().await;
        
        info!(
            "Capture {} started with {} preroll frames",
            event_id,
            preroll_frames.len()
        );

        // Create new active capture
        let active_capture = ActiveCapture {
            event_id: event_id.clone(),
            start_time: SystemTime::now(),
            motion_detected_time: motion_time,
            preroll_frames,
            postroll_frames: Vec::new(),
            _is_recording_postroll: true,
        };

        // Add to active captures
        {
            let mut captures = self.active_captures.write().await;
            captures.push(active_capture);
        }

        // Publish capture started event
        self.event_bus
            .publish(DoorcamEvent::CaptureStarted { event_id })
            .await
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to publish capture started event: {}", e)))?;

        Ok(())
    }

    /// Monitor active captures for postroll completion
    async fn monitor_postroll(&self) {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        
        loop {
            interval.tick().await;
            
            let mut completed_captures = Vec::new();
            
            // Check for completed captures
            {
                let mut captures = self.active_captures.write().await;
                let postroll_duration = Duration::from_secs(self.config.postroll_seconds as u64);
                
                captures.retain(|capture| {
                    let elapsed_since_motion = capture.motion_detected_time
                        .elapsed()
                        .unwrap_or(Duration::ZERO);
                    
                    if elapsed_since_motion >= postroll_duration {
                        completed_captures.push(capture.clone());
                        false // Remove from active captures
                    } else {
                        true // Keep in active captures
                    }
                });
            }

            // Process completed captures
            for capture in completed_captures {
                if let Err(e) = self.finalize_capture(capture).await {
                    error!("Failed to finalize capture: {}", e);
                }
            }
        }
    }

    /// Finalize a completed capture by collecting postroll frames and saving
    async fn finalize_capture(&self, mut capture: ActiveCapture) -> Result<()> {
        debug!("Finalizing capture: {}", capture.event_id);

        // Collect postroll frames
        let postroll_end_time = capture.motion_detected_time + Duration::from_secs(self.config.postroll_seconds as u64);
        let postroll_frames = self.ring_buffer
            .get_frames_in_range(capture.motion_detected_time, postroll_end_time)
            .await;

        capture.postroll_frames = postroll_frames;

        // Calculate total frame count
        let total_frames = capture.preroll_frames.len() + capture.postroll_frames.len();

        info!(
            "Capture {} completed: {} preroll + {} postroll = {} total frames",
            capture.event_id,
            capture.preroll_frames.len(),
            capture.postroll_frames.len(),
            total_frames
        );

        // Save frames to storage
        let saved_files = self.save_capture_frames(&capture).await?;

        // Publish capture completed event
        self.event_bus
            .publish(DoorcamEvent::CaptureCompleted {
                event_id: capture.event_id.clone(),
                file_count: saved_files,
            })
            .await
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to publish capture completed event: {}", e)))?;

        Ok(())
    }

    /// Save capture frames to timestamped directory
    async fn save_capture_frames(&self, capture: &ActiveCapture) -> Result<u32> {
        // Create timestamped directory
        let timestamp = DateTime::<Utc>::from(capture.motion_detected_time);
        let dir_name = timestamp.format("%Y%m%d_%H%M%S_%3f").to_string();
        let capture_dir = PathBuf::from(&self.config.path).join(&dir_name);

        fs::create_dir_all(&capture_dir).await
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to create capture directory: {}", e)))?;

        info!("Saving capture {} to directory: {}", capture.event_id, capture_dir.display());

        let mut saved_files = 0u32;

        // Combine preroll and postroll frames
        let all_frames: Vec<&FrameData> = capture.preroll_frames.iter()
            .chain(capture.postroll_frames.iter())
            .collect();

        // Save individual JPEG frames if enabled
        if self.config.keep_images {
            saved_files += self.save_jpeg_frames(&all_frames, &capture_dir).await?;
        }

        // Create video file if enabled
        if self.config.video_encoding {
            if let Err(e) = self.create_video_file(&all_frames, &capture_dir, &capture.event_id).await {
                warn!("Failed to create video file for capture {}: {}", capture.event_id, e);
                // Don't fail the entire capture if video encoding fails
            } else {
                saved_files += 1; // Count the video file
            }
        }

        // Save metadata file
        if let Err(e) = self.save_capture_metadata(capture, &capture_dir).await {
            warn!("Failed to save metadata for capture {}: {}", capture.event_id, e);
        } else {
            saved_files += 1; // Count the metadata file
        }

        info!("Saved {} files for capture {}", saved_files, capture.event_id);
        Ok(saved_files)
    }

    /// Save frames as individual JPEG files
    async fn save_jpeg_frames(&self, frames: &[&FrameData], capture_dir: &PathBuf) -> Result<u32> {
        let mut saved_count = 0u32;

        for (index, frame) in frames.iter().enumerate() {
            let filename = format!("frame_{:06}.jpg", index);
            let file_path = capture_dir.join(filename);

            match self.process_and_save_frame(frame, &file_path).await {
                Ok(_) => {
                    saved_count += 1;
                    debug!("Saved frame {} to {}", frame.id, file_path.display());
                }
                Err(e) => {
                    error!("Failed to save frame {} to {}: {}", frame.id, file_path.display(), e);
                }
            }
        }

        info!("Saved {} JPEG frames to {}", saved_count, capture_dir.display());
        Ok(saved_count)
    }

    /// Process a frame (rotation, timestamp overlay) and save as JPEG
    async fn process_and_save_frame(&self, frame: &FrameData, file_path: &PathBuf) -> Result<()> {
        // Create processed frame with rotation if needed
        let processed_frame = ProcessedFrame::from_frame(frame.clone(), None).await
            .map_err(|e| DoorcamError::component("video_capture", &format!("Frame processing failed: {}", e)))?;

        // Get JPEG data (this will encode if needed)
        let jpeg_data = processed_frame.get_jpeg().await
            .map_err(|e| DoorcamError::component("video_capture", &format!("JPEG encoding failed: {}", e)))?;

        // Apply timestamp overlay if enabled
        let final_jpeg_data = if self.config.timestamp_overlay {
            self.add_timestamp_overlay(&jpeg_data, frame.timestamp).await?
        } else {
            jpeg_data
        };

        // Write to file
        fs::write(file_path, &*final_jpeg_data).await
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to write JPEG file: {}", e)))?;

        Ok(())
    }

    /// Add timestamp overlay to JPEG image
    async fn add_timestamp_overlay(&self, jpeg_data: &[u8], timestamp: SystemTime) -> Result<Arc<Vec<u8>>> {
        // For now, return the original data
        // In a full implementation, this would use image processing to add timestamp text
        // This could be implemented using the `image` crate or OpenCV
        
        debug!("Timestamp overlay requested for frame at {:?}", timestamp);
        
        // TODO: Implement actual timestamp overlay using image processing
        // For now, just return the original JPEG data
        Ok(Arc::new(jpeg_data.to_vec()))
    }

    /// Create video file from frames using hardware acceleration if available
    async fn create_video_file(&self, frames: &[&FrameData], capture_dir: &PathBuf, event_id: &str) -> Result<()> {
        let video_filename = format!("{}.mp4", event_id);
        let video_path = capture_dir.join(video_filename);

        info!("Creating video file: {}", video_path.display());

        // For now, we'll create a simple implementation
        // In a full implementation, this would use FFmpeg or hardware encoding
        
        // Create a placeholder video file for now
        let video_info = format!(
            "Video file for capture {}\nFrames: {}\nCreated: {:?}\n",
            event_id,
            frames.len(),
            SystemTime::now()
        );

        fs::write(&video_path, video_info).await
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to create video file: {}", e)))?;

        // TODO: Implement actual video encoding
        // This would involve:
        // 1. Setting up FFmpeg or hardware encoder (h264_v4l2m2m)
        // 2. Processing frames through the encoder
        // 3. Writing the encoded video stream to file

        info!("Video file created: {}", video_path.display());
        Ok(())
    }

    /// Save capture metadata as JSON
    async fn save_capture_metadata(&self, capture: &ActiveCapture, capture_dir: &PathBuf) -> Result<()> {
        let metadata = CaptureMetadata {
            event_id: capture.event_id.clone(),
            start_time: capture.start_time,
            motion_detected_time: capture.motion_detected_time,
            preroll_frame_count: capture.preroll_frames.len(),
            postroll_frame_count: capture.postroll_frames.len(),
            total_frame_count: capture.preroll_frames.len() + capture.postroll_frames.len(),
            config: self.config.clone(),
        };

        let metadata_json = serde_json::to_string_pretty(&metadata)
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to serialize metadata: {}", e)))?;

        let metadata_path = capture_dir.join("metadata.json");
        fs::write(&metadata_path, metadata_json).await
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to write metadata file: {}", e)))?;

        debug!("Saved metadata to {}", metadata_path.display());
        Ok(())
    }

    /// Get statistics about active captures
    pub async fn get_capture_stats(&self) -> CaptureStats {
        let captures = self.active_captures.read().await;
        
        CaptureStats {
            active_captures: captures.len(),
            total_active_frames: captures.iter()
                .map(|c| c.preroll_frames.len() + c.postroll_frames.len())
                .sum(),
        }
    }

    /// Stop the video capture system
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping video capture system");
        
        // Wait for active captures to complete or force finalize them
        let active_captures = {
            let mut captures = self.active_captures.write().await;
            std::mem::take(&mut *captures)
        };

        for capture in active_captures {
            warn!("Force finalizing capture on shutdown: {}", capture.event_id);
            if let Err(e) = self.finalize_capture(capture).await {
                error!("Failed to finalize capture during shutdown: {}", e);
            }
        }

        info!("Video capture system stopped");
        Ok(())
    }
}

impl Clone for VideoCapture {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            event_bus: Arc::clone(&self.event_bus),
            ring_buffer: Arc::clone(&self.ring_buffer),
            active_captures: Arc::clone(&self.active_captures),
        }
    }
}

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{CaptureConfig},
        events::EventBus,
        frame::{FrameData, FrameFormat},
        ring_buffer::RingBuffer,
    };
    use std::time::Duration;
    use tokio::time::timeout;

    fn create_test_config() -> CaptureConfig {
        CaptureConfig {
            preroll_seconds: 2,
            postroll_seconds: 3,
            path: "./test_captures".to_string(),
            timestamp_overlay: true,
            video_encoding: false,
            keep_images: true,
        }
    }

    fn create_test_frame(id: u64, timestamp: SystemTime) -> FrameData {
        FrameData::new(
            id,
            timestamp,
            vec![0u8; 1024],
            640,
            480,
            FrameFormat::Mjpeg,
        )
    }

    #[tokio::test]
    async fn test_video_capture_creation() {
        let config = create_test_config();
        let event_bus = Arc::new(EventBus::new(10));
        let ring_buffer = Arc::new(RingBuffer::new(50, Duration::from_secs(5)));

        let capture = VideoCapture::new(config, event_bus, ring_buffer);
        
        let stats = capture.get_capture_stats().await;
        assert_eq!(stats.active_captures, 0);
    }

    #[tokio::test]
    async fn test_motion_triggered_capture() {
        let config = create_test_config();
        let event_bus = Arc::new(EventBus::new(10));
        let ring_buffer = Arc::new(RingBuffer::new(50, Duration::from_secs(5)));

        // Add a subscriber to prevent channel closed error
        let _receiver = event_bus.subscribe();

        // Add some frames to ring buffer
        let now = SystemTime::now();
        for i in 0..10 {
            let frame = create_test_frame(i, now - Duration::from_millis(100 * (10 - i)));
            ring_buffer.push_frame(frame).await;
        }

        let capture = VideoCapture::new(config, Arc::clone(&event_bus), ring_buffer);
        
        // Simulate motion detection
        let motion_time = SystemTime::now();
        capture.handle_motion_detected(motion_time).await.unwrap();

        // Check that capture was started
        let stats = capture.get_capture_stats().await;
        assert_eq!(stats.active_captures, 1);
    }

    #[tokio::test]
    async fn test_capture_completion() {
        let mut config = create_test_config();
        config.postroll_seconds = 1; // Short postroll for testing
        
        let event_bus = Arc::new(EventBus::new(10));
        let ring_buffer = Arc::new(RingBuffer::new(50, Duration::from_secs(5)));

        // Add a subscriber to prevent channel closed error
        let _receiver = event_bus.subscribe();

        // Add frames to ring buffer
        let now = SystemTime::now();
        for i in 0..20 {
            let frame = create_test_frame(i, now + Duration::from_millis(50 * i));
            ring_buffer.push_frame(frame).await;
        }

        let capture = VideoCapture::new(config, Arc::clone(&event_bus), ring_buffer);
        
        // Start capture
        let motion_time = now + Duration::from_millis(500);
        capture.handle_motion_detected(motion_time).await.unwrap();

        // Wait for postroll to complete
        tokio::time::sleep(Duration::from_millis(1200)).await;

        // Manually trigger finalization (in real system this happens automatically)
        let captures = {
            let mut active = capture.active_captures.write().await;
            std::mem::take(&mut *active)
        };

        for cap in captures {
            capture.finalize_capture(cap).await.unwrap();
        }

        let stats = capture.get_capture_stats().await;
        assert_eq!(stats.active_captures, 0);
    }
}