use crate::{
    config::CaptureConfig,
    error::{CaptureError, DoorcamError, Result},
    events::{DoorcamEvent, EventBus},
    frame::{FrameData, ProcessedFrame},
    ring_buffer::RingBuffer,
    wal::{WalWriter, WalReader, delete_wal, find_wal_files},
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use tokio::sync::{RwLock, Mutex, mpsc};
use tokio::fs;
use tracing::{debug, error, info, warn};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[cfg(all(feature = "video_encoding", target_os = "linux"))]
use gstreamer::prelude::*;
#[cfg(all(feature = "video_encoding", target_os = "linux"))]
use gstreamer::Pipeline;
#[cfg(all(feature = "video_encoding", target_os = "linux"))]
use gstreamer_app::AppSrc;

/// Video capture system for motion-triggered recording
pub struct VideoCapture {
    config: CaptureConfig,
    event_bus: Arc<EventBus>,
    ring_buffer: Arc<RingBuffer>,
    active_captures: Arc<RwLock<Vec<Arc<CaptureEventTask>>>>,
    video_queue_tx: mpsc::UnboundedSender<VideoGenerationJob>,
}

/// Represents an active capture event task
struct CaptureEventTask {
    event_id: String,
    initial_motion_time: SystemTime,
    latest_motion_time: Arc<Mutex<SystemTime>>,
    capture_dir: PathBuf,
    last_frame_id: AtomicU64,
    cancellation_token: CancellationToken,
    is_finalized: AtomicBool,
}

/// Video generation job for the background queue
struct VideoGenerationJob {
    event_id: String,
    capture_dir: PathBuf,
    wal_path: PathBuf,
    frame_count: u32,
}

impl CaptureEventTask {
    fn new(event_id: String, initial_motion_time: SystemTime, capture_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            event_id,
            initial_motion_time,
            latest_motion_time: Arc::new(Mutex::new(initial_motion_time)),
            capture_dir,
            last_frame_id: AtomicU64::new(0),
            cancellation_token: CancellationToken::new(),
            is_finalized: AtomicBool::new(false),
        })
    }
    
    async fn extend_postroll(&self, new_motion_time: SystemTime) {
        let mut latest = self.latest_motion_time.lock().await;
        *latest = new_motion_time;
        info!("Extended capture {} postroll to {:?}", self.event_id, new_motion_time);
    }
    
    fn cancel(&self) {
        self.cancellation_token.cancel();
    }
}

impl VideoCapture {
    /// Create a new video capture system
    pub fn new(
        config: CaptureConfig,
        event_bus: Arc<EventBus>,
        ring_buffer: Arc<RingBuffer>,
    ) -> Self {
        let (video_queue_tx, video_queue_rx) = mpsc::unbounded_channel();
        
        // Spawn video generation worker
        let config_clone = config.clone();
        tokio::spawn(async move {
            Self::video_generation_worker(video_queue_rx, config_clone).await;
        });
        
        Self {
            config,
            event_bus,
            ring_buffer,
            active_captures: Arc::new(RwLock::new(Vec::new())),
            video_queue_tx,
        }
    }
    
    /// Background worker that processes video generation jobs
    async fn video_generation_worker(
        mut queue_rx: mpsc::UnboundedReceiver<VideoGenerationJob>,
        config: CaptureConfig,
    ) {
        info!("Video generation worker started");
        
        while let Some(job) = queue_rx.recv().await {
            let event_id = job.event_id.clone();
            let frame_count = job.frame_count;
            
            info!("Processing video generation job for capture {} ({} frames)", event_id, frame_count);
            
            if let Err(e) = Self::generate_video_from_wal(job, &config).await {
                error!("Failed to generate video for capture {}: {}", event_id, e);
            } else {
                info!("Video generation completed for capture {}", event_id);
            }
        }
        
        info!("Video generation worker stopped");
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
        
        // Recover any incomplete captures from WAL files
        self.recover_incomplete_captures().await?;

        // Subscribe to motion detection events
        let mut event_receiver = self.event_bus.subscribe();
        let capture_system = Arc::new(self.clone());

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

        info!("Video capture system started successfully");
        Ok(())
    }

    /// Handle motion detection event by starting a new capture or extending an existing one
    async fn handle_motion_detected(&self, motion_time: SystemTime) -> Result<()> {
        info!("Motion event received at {:?}", motion_time);
        
        // Check if there's an active capture that can be extended
        {
            let captures = self.active_captures.read().await;
            
            info!("Currently {} active capture(s)", captures.len());
            
            let postroll_duration = Duration::from_secs(self.config.postroll_seconds as u64);
            
            // Look for an active capture within the postroll window
            for capture_task in captures.iter() {
                let latest_motion = *capture_task.latest_motion_time.lock().await;
                let time_since_motion = motion_time
                    .duration_since(latest_motion)
                    .unwrap_or(Duration::ZERO);
                
                // If motion detected within the postroll period, extend the capture
                if time_since_motion < postroll_duration {
                    info!(
                        "Motion detected during active capture {} - extending postroll period (was {:?} into postroll)",
                        capture_task.event_id,
                        time_since_motion
                    );
                    
                    capture_task.extend_postroll(motion_time).await;
                    
                    return Ok(());
                }
            }
        }
        
        // No active capture to extend, create a new one
        // Use timestamp as event ID for easy identification
        let timestamp = DateTime::<Utc>::from(motion_time);
        let event_id = timestamp.format("%Y%m%d_%H%M%S_%3f").to_string();
        
        info!("No active capture to extend - starting new capture: {}", event_id);

        // Create directory for frames only if needed
        // Metadata will be stored in a shared metadata directory
        let capture_dir = PathBuf::from(&self.config.path).join(&event_id);

        // Only create event directory if we need to save images
        if self.config.keep_images {
            fs::create_dir_all(&capture_dir).await
                .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to create capture directory: {}", e)))?;

            info!("Created capture directory for images: {}", capture_dir.display());
        } else {
            debug!("Skipping capture directory creation (images disabled)");
        }

        // Create capture event task
        let capture_task = CaptureEventTask::new(event_id.clone(), motion_time, capture_dir.clone());
        
        // Add to active captures
        {
            let mut captures = self.active_captures.write().await;
            captures.push(Arc::clone(&capture_task));
        }

        // Spawn dedicated task for this capture event
        let ring_buffer = Arc::clone(&self.ring_buffer);
        let config = self.config.clone();
        let event_bus = Arc::clone(&self.event_bus);
        let video_queue_tx = self.video_queue_tx.clone();
        let active_captures = Arc::clone(&self.active_captures);
        
        tokio::spawn(async move {
            if let Err(e) = Self::run_capture_event(
                capture_task,
                ring_buffer,
                config,
                event_bus,
                video_queue_tx,
                active_captures,
            ).await {
                error!("Capture event task failed: {}", e);
            }
        });

        Ok(())
    }

    /// Run a dedicated capture event task that writes frames to WAL
    async fn run_capture_event(
        capture_task: Arc<CaptureEventTask>,
        ring_buffer: Arc<RingBuffer>,
        config: CaptureConfig,
        event_bus: Arc<EventBus>,
        video_queue_tx: mpsc::UnboundedSender<VideoGenerationJob>,
        active_captures: Arc<RwLock<Vec<Arc<CaptureEventTask>>>>,
    ) -> Result<()> {
        let event_id = capture_task.event_id.clone();
        info!("Starting capture event task for {}", event_id);
        
        let preroll_duration = Duration::from_secs(config.preroll_seconds as u64);
        let postroll_duration = Duration::from_secs(config.postroll_seconds as u64);
        
        // Create WAL writer for persistent storage
        let wal_dir = PathBuf::from(&config.path).join("wal");
        let mut wal_writer = WalWriter::new(event_id.clone(), &wal_dir).await?;
        
        // Collect and write preroll frames
        let preroll_start = capture_task.initial_motion_time - preroll_duration;
        let preroll_frames = ring_buffer
            .get_frames_in_range(preroll_start, capture_task.initial_motion_time)
            .await;
        
        let preroll_count = preroll_frames.len();
        info!("Collected {} preroll frames for capture {}", preroll_count, event_id);
        
        // Write preroll frames to WAL
        for frame in preroll_frames {
            capture_task.last_frame_id.store(frame.id, Ordering::SeqCst);
            wal_writer.append_frame(&frame).await?;
        }
        
        info!("Wrote {} preroll frames to WAL", preroll_count);
        
        // Now start the postroll collection phase
        let mut check_interval = tokio::time::interval(Duration::from_millis(100));
        let postroll_start = SystemTime::now();
        
        info!("Starting postroll period for capture {}", event_id);
        
        loop {
            tokio::select! {
                _ = check_interval.tick() => {
                    // Collect new frames since last check
                    let last_frame_id = capture_task.last_frame_id.load(Ordering::SeqCst);
                    let new_frames = ring_buffer.get_frames_since_id(last_frame_id).await;
                    
                    // Write new frames to WAL
                    if !new_frames.is_empty() {
                        for frame in new_frames {
                            capture_task.last_frame_id.store(frame.id, Ordering::SeqCst);
                            wal_writer.append_frame(&frame).await?;
                        }
                    }
                    
                    // Check if we should finalize
                    // Calculate time since last motion
                    let latest_motion = *capture_task.latest_motion_time.lock().await;
                    let time_since_motion = SystemTime::now()
                        .duration_since(latest_motion)
                        .unwrap_or(Duration::ZERO);
                    
                    // Also ensure minimum postroll time has elapsed
                    let time_since_postroll_start = SystemTime::now()
                        .duration_since(postroll_start)
                        .unwrap_or(Duration::ZERO);
                    
                    if time_since_motion >= postroll_duration && time_since_postroll_start >= postroll_duration {
                        info!(
                            "Postroll period completed for capture {} ({}s since last motion)",
                            event_id,
                            time_since_motion.as_secs()
                        );
                        break;
                    }
                }
                _ = capture_task.cancellation_token.cancelled() => {
                    warn!("Capture event task {} cancelled", event_id);
                    break;
                }
            }
        }
        
        // Close WAL and get path
        let frame_count = wal_writer.frame_count();
        let wal_path = wal_writer.close().await?;
        
        // Mark as finalized
        capture_task.is_finalized.store(true, Ordering::SeqCst);
        
        let total_frames = frame_count as usize;
        
        info!(
            "Capture {} finalized with {} total frames (stored in WAL)",
            event_id,
            total_frames
        );
        
        // Save metadata if enabled (to shared metadata directory)
        if config.save_metadata {
            let metadata = CaptureMetadata {
                event_id: event_id.clone(),
                start_time: capture_task.initial_motion_time - preroll_duration,
                motion_detected_time: capture_task.initial_motion_time,
                preroll_frame_count: preroll_count,
                postroll_frame_count: total_frames - preroll_count,
                total_frame_count: total_frames,
                config: config.clone(),
            };
            
            if let Err(e) = Self::save_metadata(&metadata, &event_id, &config.path).await {
                warn!("Failed to save metadata for capture {}: {}", event_id, e);
            }
        } else {
            debug!("Skipping metadata save (disabled in config)");
        }
        
        // Queue video generation from WAL
        if config.video_encoding && total_frames > 0 {
            let job = VideoGenerationJob {
                event_id: event_id.clone(),
                capture_dir: capture_task.capture_dir.clone(),
                wal_path,
                frame_count,
            };
            
            if let Err(e) = video_queue_tx.send(job) {
                error!("Failed to queue video generation for capture {}: {}", event_id, e);
            } else {
                info!("Queued video generation for capture {} ({} frames)", event_id, total_frames);
            }
        } else {
            info!("Video encoding disabled or no frames captured");
            // Delete WAL if not encoding
            if let Err(e) = delete_wal(&wal_path).await {
                warn!("Failed to delete WAL file: {}", e);
            }
        }
        
        // Publish capture completed event
        let saved_files = 1; // Just metadata (video will be created by encoder)
        
        if let Err(e) = event_bus.publish(DoorcamEvent::CaptureCompleted {
            event_id: event_id.clone(),
            file_count: saved_files,
        }).await {
            error!("Failed to publish capture completed event: {}", e);
        }
        
        // Remove from active captures
        {
            let mut captures = active_captures.write().await;
            captures.retain(|c| c.event_id != event_id);
        }
        
        info!("Capture event task {} completed", event_id);
        Ok(())
    }
    


    
    /// Save metadata to disk in shared metadata directory
    async fn save_metadata(metadata: &CaptureMetadata, event_id: &str, capture_path: &str) -> Result<()> {
        let metadata_json = serde_json::to_string_pretty(&metadata)
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to serialize metadata: {}", e)))?;

        // Store metadata in shared metadata directory
        let metadata_dir = PathBuf::from(capture_path).join("metadata");
        fs::create_dir_all(&metadata_dir).await
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to create metadata directory: {}", e)))?;

        let metadata_path = metadata_dir.join(format!("{}.json", event_id));
        fs::write(&metadata_path, metadata_json).await
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to write metadata file: {}", e)))?;

        debug!("Saved metadata to {}", metadata_path.display());
        Ok(())
    }

    /// Generate video from WAL file (called by background worker)
    async fn generate_video_from_wal(job: VideoGenerationJob, config: &CaptureConfig) -> Result<()> {
        let video_filename = format!("{}.mp4", job.event_id);
        // Save video in root captures directory, not in event subdirectory
        let video_path = PathBuf::from(&config.path).join(video_filename);

        info!("Generating video file from WAL: {}", video_path.display());

        // Read frames from WAL
        let reader = WalReader::new(job.wal_path.clone());
        let frames = reader.read_all_frames().await?;
        
        info!("Read {} frames from WAL for encoding", frames.len());

        // Extract individual JPEGs if keep_images is enabled
        if config.keep_images {
            info!("Extracting {} JPEG files from WAL", frames.len());
            Self::extract_jpegs_from_frames(&frames, &job.capture_dir, config).await?;
        }

        #[cfg(all(feature = "video_encoding", target_os = "linux"))]
        {
            Self::create_video_gstreamer_from_frames(&frames, &video_path, config).await?;
        }

        #[cfg(not(all(feature = "video_encoding", target_os = "linux")))]
        {
            return Err(DoorcamError::component("video_capture", "Video encoding not available on this platform"));
        }

        // Delete WAL file after successful encoding
        if let Err(e) = delete_wal(&job.wal_path).await {
            warn!("Failed to delete WAL file: {}", e);
        }

        info!("Video file created: {} ({} frames encoded)", video_path.display(), frames.len());
        Ok(())
    }
    
    /// Extract individual JPEG files from frames
    async fn extract_jpegs_from_frames(
        frames: &[FrameData],
        capture_dir: &PathBuf,
        config: &CaptureConfig,
    ) -> Result<()> {
        // Create frames subdirectory
        let frames_dir = capture_dir.join("frames");
        fs::create_dir_all(&frames_dir).await
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to create frames directory: {}", e)))?;
        
        for frame in frames {
            let timestamp = DateTime::<Utc>::from(frame.timestamp);
            let filename = format!("{}.jpg", timestamp.format("%Y%m%d_%H%M%S_%3f"));
            let file_path = frames_dir.join(&filename);
            
            // Get JPEG data (with optional timestamp overlay)
            let jpeg_data = if config.timestamp_overlay {
                let processed_frame = ProcessedFrame::from_frame(frame.clone(), None).await
                    .map_err(|e| DoorcamError::component("video_capture", &format!("Frame processing failed: {}", e)))?;
                let base_jpeg = processed_frame.get_jpeg().await
                    .map_err(|e| DoorcamError::component("video_capture", &format!("JPEG encoding failed: {}", e)))?;
                Self::add_timestamp_overlay_static(&base_jpeg, frame.timestamp, config).await?
            } else {
                frame.data.clone()
            };
            
            // Write to file
            fs::write(&file_path, &*jpeg_data).await
                .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to write JPEG file: {}", e)))?;
        }
        
        info!("Extracted {} JPEG files", frames.len());
        Ok(())
    }

    /// Create video using GStreamer from frames
    #[cfg(all(feature = "video_encoding", target_os = "linux"))]
    async fn create_video_gstreamer_from_frames(frames: &[FrameData], video_path: &PathBuf, config: &CaptureConfig) -> Result<()> {
        // Initialize GStreamer if not already done
        gstreamer::init().map_err(|e| {
            DoorcamError::component("video_capture", &format!("Failed to initialize GStreamer: {}", e))
        })?;

        // Use hardware H.264 encoding via V4L2 with explicit level setting
        // Use v4l2jpegdec for hardware-accelerated JPEG decoding on Pi 4
        // Note: Must set h264_level in extra-controls AND output caps level to avoid driver errors
        // Higher quality encoding for 1920x1080:
        // - 8 Mbps bitrate (good quality for Full HD)
        // - Variable bitrate mode for better quality/size ratio
        // - High profile for better compression efficiency
        let pipeline_desc = format!(
            "appsrc name=src format=time is-live=false caps=image/jpeg,framerate=30/1 ! \
             v4l2jpegdec ! \
             videoconvert ! \
             v4l2h264enc extra-controls=\"controls,h264_level=11,h264_profile=4,video_bitrate=8000000,video_bitrate_mode=0,repeat_sequence_header=1\" ! \
             video/x-h264,level=(string)4 ! \
             h264parse ! \
             mp4mux faststart=true ! \
             filesink location={}",
            video_path.to_string_lossy()
        );
        
        let encoder_type = "hardware (v4l2jpegdec + v4l2h264enc)";

        info!("Creating GStreamer video encoding pipeline using {} decoder/encoder", encoder_type);
        debug!("Pipeline: {}", pipeline_desc);

        let pipeline = gstreamer::parse::launch(&pipeline_desc)
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to create video pipeline: {}", e)))?
            .downcast::<Pipeline>()
            .map_err(|_| DoorcamError::component("video_capture", "Failed to downcast to Pipeline"))?;

        let appsrc = pipeline
            .by_name("src")
            .ok_or_else(|| DoorcamError::component("video_capture", "Failed to get appsrc element"))?
            .downcast::<AppSrc>()
            .map_err(|_| DoorcamError::component("video_capture", "Failed to downcast to AppSrc"))?;

        // Configure appsrc
        appsrc.set_property("format", gstreamer::Format::Time);
        appsrc.set_property("is-live", false);

        // Start pipeline
        pipeline.set_state(gstreamer::State::Playing)
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to start video pipeline: {}", e)))?;

        info!("Started GStreamer video encoding pipeline");

        // Get base time from first frame
        let base_time = if let Some(first_frame) = frames.first() {
            first_frame.timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_nanos() as u64
        } else {
            0
        };
        
        info!("Encoding {} frames", frames.len());
        
        // Process frames
        for (frame_index, frame) in frames.iter().enumerate() {
            // Get JPEG data from frame (with optional timestamp overlay)
            let jpeg_data = if config.timestamp_overlay {
                // Apply timestamp overlay
                let processed_frame = ProcessedFrame::from_frame(frame.clone(), None).await
                    .map_err(|e| DoorcamError::component("video_capture", &format!("Frame processing failed: {}", e)))?;
                let base_jpeg = processed_frame.get_jpeg().await
                    .map_err(|e| DoorcamError::component("video_capture", &format!("JPEG encoding failed: {}", e)))?;
                Self::add_timestamp_overlay_static(&base_jpeg, frame.timestamp, config).await?
            } else {
                // Use frame data directly (already an Arc<Vec<u8>>)
                frame.data.clone()
            };

            // Create GStreamer buffer
            let mut buffer = gstreamer::Buffer::with_size(jpeg_data.len())
                .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to create buffer: {}", e)))?;

            {
                let buffer_ref = buffer.get_mut().unwrap();
                let mut map = buffer_ref.map_writable()
                    .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to map buffer: {}", e)))?;
                map.copy_from_slice(&jpeg_data);
            }

            // Calculate timestamp relative to first frame
            let frame_ns = frame.timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_nanos() as u64;
            let relative_ns = frame_ns.saturating_sub(base_time);
            
            // Calculate duration to next frame
            let next_duration = if frame_index + 1 < frames.len() {
                let next_frame = &frames[frame_index + 1];
                let next_ns = next_frame.timestamp
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_nanos() as u64;
                next_ns.saturating_sub(frame_ns)
            } else {
                1_000_000_000 / 30 // Last frame, use default 30 FPS duration
            };
            
            // Set timestamp and duration
            buffer.get_mut().unwrap().set_pts(gstreamer::ClockTime::from_nseconds(relative_ns));
            buffer.get_mut().unwrap().set_duration(gstreamer::ClockTime::from_nseconds(next_duration));

            // Push buffer
            appsrc.push_buffer(buffer)
                .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to push buffer: {:?}", e)))?;

            if frame_index % 30 == 0 && frame_index > 0 {
                debug!("Encoded {} frames to video", frame_index);
            }
        }

        // Signal end of stream
        appsrc.end_of_stream()
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to signal EOS: {:?}", e)))?;

        // Wait for pipeline to finish
        let bus = pipeline.bus().unwrap();
        for msg in bus.iter_timed(gstreamer::ClockTime::from_seconds(30)) {
            match msg.view() {
                gstreamer::MessageView::Eos(..) => {
                    info!("Video encoding completed successfully");
                    break;
                }
                gstreamer::MessageView::Error(err) => {
                    let error_msg = format!("Video encoding error: {} ({})", err.error(), err.debug().unwrap_or_default());
                    return Err(DoorcamError::component("video_capture", &error_msg));
                }
                _ => {}
            }
        }

        // Stop pipeline
        pipeline.set_state(gstreamer::State::Null)
            .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to stop pipeline: {}", e)))?;

        info!("GStreamer video encoding completed: {} frames", frames.len());
        Ok(())
    }

    /// Add timestamp overlay to JPEG image (static version for use in static context)
    async fn add_timestamp_overlay_static(jpeg_data: &[u8], timestamp: SystemTime, config: &CaptureConfig) -> Result<Arc<Vec<u8>>> {
        #[cfg(feature = "motion_analysis")]
        {
            use image::{DynamicImage, ImageFormat, Rgba};
            use imageproc::drawing::draw_text_mut;
            use imageproc::drawing::text_size;
            use rusttype::{Font, Scale};
            use std::fs;
            
            // Decode JPEG
            let mut img = image::load_from_memory_with_format(jpeg_data, ImageFormat::Jpeg)
                .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to decode JPEG for overlay: {}", e)))?
                .to_rgba8();
            
            // Format timestamp
            let datetime = DateTime::<Utc>::from(timestamp);
            let timestamp_text = datetime.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string();
            
            // Load font from system path specified in config
            let font_data = fs::read(&config.timestamp_font_path)
                .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to read font file '{}': {}", config.timestamp_font_path, e)))?;
            
            let font = Font::try_from_vec(font_data)
                .ok_or_else(|| DoorcamError::component("video_capture", &format!("Failed to parse font file '{}'", config.timestamp_font_path)))?;
            
            let scale = Scale::uniform(config.timestamp_font_size);
            
            // Position: bottom-left corner with some padding
            let x: u32 = 10;
            let y: u32 = img.height().saturating_sub((config.timestamp_font_size * 1.5) as u32);
            
            // Calculate text dimensions for background
            let (text_width, text_height) = text_size(scale, &font, &timestamp_text);
            
            // Draw semi-transparent black background for better readability
            for dy in 0..(text_height as u32 + 10) {
                for dx in 0..(text_width as u32 + 10) {
                    let px = x.saturating_sub(5) + dx;
                    let py = y.saturating_sub(5) + dy;
                    if px < img.width() && py < img.height() {
                        let pixel = img.get_pixel(px, py);
                        // Blend with black background (alpha blending)
                        img.put_pixel(px, py, Rgba([
                            pixel[0] / 3,
                            pixel[1] / 3,
                            pixel[2] / 3,
                            255
                        ]));
                    }
                }
            }
            
            // Draw white text
            draw_text_mut(
                &mut img,
                Rgba([255, 255, 255, 255]),
                x as i32,
                y as i32,
                scale,
                &font,
                &timestamp_text
            );
            
            // Encode back to JPEG
            let mut output = Vec::new();
            DynamicImage::ImageRgba8(img)
                .write_to(&mut std::io::Cursor::new(&mut output), ImageFormat::Jpeg)
                .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to encode JPEG with overlay: {}", e)))?;
            
            debug!("Added timestamp overlay: {} (font: {}, size: {})", timestamp_text, config.timestamp_font_path, config.timestamp_font_size);
            Ok(Arc::new(output))
        }
        
        #[cfg(not(feature = "motion_analysis"))]
        {
            debug!("Timestamp overlay requested but motion_analysis feature not enabled");
            let _ = (timestamp, config); // Suppress unused variable warnings
            Ok(Arc::new(jpeg_data.to_vec()))
        }
    }



    /// Get statistics about active captures
    pub async fn get_capture_stats(&self) -> CaptureStats {
        let captures = self.active_captures.read().await;
        
        CaptureStats {
            active_captures: captures.len(),
            total_active_frames: 0, // Frames are now in memory, not tracked individually
        }
    }

    /// Recover incomplete captures from WAL files (called on startup)
    async fn recover_incomplete_captures(&self) -> Result<()> {
        let wal_dir = PathBuf::from(&self.config.path).join("wal");
        
        if !wal_dir.exists() {
            return Ok(());
        }
        
        let wal_files = find_wal_files(&wal_dir).await?;
        
        if wal_files.is_empty() {
            info!("No incomplete captures to recover");
            return Ok(());
        }
        
        info!("Found {} incomplete capture(s) to recover", wal_files.len());
        
        for wal_path in wal_files {
            // Extract event ID from filename (timestamp format: YYYYMMDD_HHMMSS_mmm)
            let event_id = wal_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            
            info!("Recovering capture: {} (from WAL)", event_id);
            
            // Read frame count from WAL
            let reader = WalReader::new(wal_path.clone());
            let frames = reader.read_all_frames().await?;
            let frame_count = frames.len() as u32;
            
            // Capture directory should already exist (created when capture started)
            // But create it if missing (in case it was deleted) and if we need it for images
            let capture_dir = PathBuf::from(&self.config.path).join(&event_id);
            if !capture_dir.exists() && self.config.keep_images {
                fs::create_dir_all(&capture_dir).await
                    .map_err(|e| DoorcamError::component("video_capture", &format!("Failed to create capture directory: {}", e)))?;
                info!("Created missing capture directory for recovered event: {}", event_id);
            }
            
            // Queue for encoding
            let job = VideoGenerationJob {
                event_id: event_id.clone(),
                capture_dir,
                wal_path,
                frame_count,
            };
            
            if let Err(e) = self.video_queue_tx.send(job) {
                error!("Failed to queue recovered capture for encoding: {}", e);
            } else {
                info!("Queued recovered capture {} for encoding ({} frames)", event_id, frame_count);
            }
        }
        
        Ok(())
    }
    
    /// Stop the video capture system
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping video capture system");
        
        // Cancel all active capture tasks
        let active_captures = {
            let captures = self.active_captures.read().await;
            captures.clone()
        };

        for capture in active_captures {
            warn!("Cancelling capture on shutdown: {}", capture.event_id);
            capture.cancel();
        }
        
        // Wait a bit for tasks to finish
        tokio::time::sleep(Duration::from_millis(500)).await;

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
            video_queue_tx: self.video_queue_tx.clone(),
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


    fn create_test_config() -> CaptureConfig {
        CaptureConfig {
            preroll_seconds: 2,
            postroll_seconds: 3,
            path: "./test_captures".to_string(),
            timestamp_overlay: true,
            timestamp_font_path: "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".to_string(),
            timestamp_font_size: 24.0,
            video_encoding: false,
            keep_images: true,
            save_metadata: true,
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

        // Wait for postroll to complete and task to finalize
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Check that capture completed
        let stats = capture.get_capture_stats().await;
        assert_eq!(stats.active_captures, 0);
    }
}