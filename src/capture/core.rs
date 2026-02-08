use crate::{
    config::{CaptureConfig, EventConfig},
    error::{CaptureError, DoorcamError, Result},
    events::{DoorcamEvent, EventBus},
    ring_buffer::RingBuffer,
    wal::{delete_wal, find_wal_files, WalReader, WalWriter},
};
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::fs;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use super::{
    encode::{video_generation_worker, VideoGenerationJob},
    metadata::{save_metadata, CaptureMetadata, CaptureStats},
    overlay::resolve_timestamp_timezone,
};

/// Video capture system for motion-triggered recording
pub struct VideoCapture {
    config: CaptureConfig,
    event_config: EventConfig,
    camera_fps: u32,
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
        info!(
            "Extended capture {} postroll to {:?}",
            self.event_id, new_motion_time
        );
    }

    fn cancel(&self) {
        self.cancellation_token.cancel();
    }
}

impl VideoCapture {
    /// Create a new video capture system
    pub fn new(
        config: CaptureConfig,
        event_config: EventConfig,
        camera_fps: u32,
        event_bus: Arc<EventBus>,
        ring_buffer: Arc<RingBuffer>,
    ) -> Self {
        let (video_queue_tx, video_queue_rx) = mpsc::unbounded_channel();

        // Spawn video generation worker
        let config_clone = config.clone();
        tokio::spawn(async move {
            video_generation_worker(video_queue_rx, config_clone).await;
        });

        Self {
            config,
            event_config,
            camera_fps,
            event_bus,
            ring_buffer,
            active_captures: Arc::new(RwLock::new(Vec::new())),
            video_queue_tx,
        }
    }

    /// Start the video capture system
    pub async fn start(&self) -> Result<()> {
        info!("Starting video capture system");

        let capture_path = PathBuf::from(&self.config.path);
        if !capture_path.exists() {
            std::fs::create_dir_all(&capture_path).map_err(|e| {
                CaptureError::DirectoryCreation {
                    path: capture_path.display().to_string(),
                    source: e,
                }
            })?;
            info!("Created capture directory: {}", capture_path.display());
        }

        self.recover_incomplete_captures().await?;

        let mut event_receiver = self.event_bus.subscribe();
        let capture_system = Arc::new(self.clone());

        tokio::spawn(async move {
            loop {
                match event_receiver.recv().await {
                    Ok(event) => {
                        if let DoorcamEvent::MotionDetected {
                            contour_area,
                            timestamp,
                        } = event
                        {
                            debug!(
                                "Motion detected, starting capture (area: {:.2})",
                                contour_area
                            );

                            if let Err(e) = capture_system.handle_motion_detected(timestamp).await {
                                error!("Failed to handle motion detection: {}", e);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!(
                            "Capture event listener lagged by {} events; continuing",
                            skipped
                        );
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        warn!("Event bus closed; stopping capture event listener");
                        break;
                    }
                }
            }
        });

        info!("Video capture system started successfully");
        Ok(())
    }

    /// Handle motion detection event by starting a new capture or extending an existing one
    pub async fn handle_motion_detected(&self, motion_time: SystemTime) -> Result<()> {
        info!("Motion event received at {:?}", motion_time);

        {
            let captures = self.active_captures.read().await;
            let postroll_duration = Duration::from_secs(self.event_config.postroll_seconds as u64);

            for capture_task in captures.iter() {
                let latest_motion = *capture_task.latest_motion_time.lock().await;
                let time_since_motion = motion_time
                    .duration_since(latest_motion)
                    .unwrap_or(Duration::ZERO);

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

        let timezone = resolve_timestamp_timezone(&self.config.timestamp_timezone);
        let timestamp = DateTime::<Utc>::from(motion_time).with_timezone(&timezone);
        let event_id = timestamp.format("%Y%m%d_%H%M%S_%3f").to_string();

        info!(
            "No active capture to extend - starting new capture: {}",
            event_id
        );

        let capture_dir = PathBuf::from(&self.config.path).join(&event_id);

        if self.config.keep_images {
            fs::create_dir_all(&capture_dir).await.map_err(|e| {
                DoorcamError::component(
                    "video_capture",
                    &format!("Failed to create capture directory: {}", e),
                )
            })?;

            info!(
                "Created capture directory for images: {}",
                capture_dir.display()
            );
        } else {
            debug!("Skipping capture directory creation (images disabled)");
        }

        let capture_task =
            CaptureEventTask::new(event_id.clone(), motion_time, capture_dir.clone());

        {
            let mut captures = self.active_captures.write().await;
            captures.push(Arc::clone(&capture_task));
        }

        let ring_buffer = Arc::clone(&self.ring_buffer);
        let config = self.config.clone();
        let event_config = self.event_config.clone();
        let camera_fps = self.camera_fps;
        let event_bus = Arc::clone(&self.event_bus);
        let video_queue_tx = self.video_queue_tx.clone();
        let active_captures = Arc::clone(&self.active_captures);

        tokio::spawn(async move {
            if let Err(e) = VideoCapture::run_capture_event(
                capture_task,
                ring_buffer,
                config,
                event_config,
                camera_fps,
                event_bus,
                video_queue_tx,
                active_captures,
            )
            .await
            {
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
        event_config: EventConfig,
        camera_fps: u32,
        event_bus: Arc<EventBus>,
        video_queue_tx: mpsc::UnboundedSender<VideoGenerationJob>,
        active_captures: Arc<RwLock<Vec<Arc<CaptureEventTask>>>>,
    ) -> Result<()> {
        let event_id = capture_task.event_id.clone();
        info!("Starting capture event task for {}", event_id);

        let preroll_duration = Duration::from_secs(event_config.preroll_seconds as u64);
        let postroll_duration = Duration::from_secs(event_config.postroll_seconds as u64);

        let wal_dir = PathBuf::from(&config.path).join("wal");
        let mut wal_writer = WalWriter::new(event_id.clone(), &wal_dir, camera_fps).await?;

        let preroll_start = capture_task.initial_motion_time - preroll_duration;
        let preroll_frames = ring_buffer
            .get_frames_in_range(preroll_start, capture_task.initial_motion_time)
            .await;

        let preroll_count = preroll_frames.len();
        info!(
            "Collected {} preroll frames for capture {}",
            preroll_count, event_id
        );

        for frame in preroll_frames {
            capture_task.last_frame_id.store(frame.id, Ordering::SeqCst);
            wal_writer.append_frame(&frame).await?;
        }

        info!("Wrote {} preroll frames to WAL", preroll_count);

        let mut check_interval = tokio::time::interval(Duration::from_millis(100));
        let postroll_start = SystemTime::now();

        info!("Starting postroll period for capture {}", event_id);

        loop {
            tokio::select! {
                _ = check_interval.tick() => {
                    let last_frame_id = capture_task.last_frame_id.load(Ordering::SeqCst);
                    let new_frames = ring_buffer.get_frames_since_id(last_frame_id).await;

                    if !new_frames.is_empty() {
                        for frame in new_frames {
                            capture_task.last_frame_id.store(frame.id, Ordering::SeqCst);
                            wal_writer.append_frame(&frame).await?;
                        }
                    }

                    let latest_motion = *capture_task.latest_motion_time.lock().await;
                    let time_since_motion = SystemTime::now()
                        .duration_since(latest_motion)
                        .unwrap_or(Duration::ZERO);

                    let time_since_postroll_start = SystemTime::now()
                        .duration_since(postroll_start)
                        .unwrap_or(Duration::ZERO);

                    if time_since_motion >= postroll_duration
                        && time_since_postroll_start >= postroll_duration
                    {
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

        let frame_count = wal_writer.frame_count();
        let wal_path = wal_writer.close().await?;

        capture_task.is_finalized.store(true, Ordering::SeqCst);

        let total_frames = frame_count as usize;

        info!(
            "Capture {} finalized with {} total frames (stored in WAL)",
            event_id, total_frames
        );

        if config.save_metadata {
            let metadata = CaptureMetadata {
                event_id: event_id.clone(),
                start_time: capture_task.initial_motion_time - preroll_duration,
                motion_detected_time: capture_task.initial_motion_time,
                preroll_frame_count: preroll_count,
                postroll_frame_count: total_frames - preroll_count,
                total_frame_count: total_frames,
                config: config.clone(),
                event: event_config.clone(),
            };

            if let Err(e) = save_metadata(&metadata, &event_id, &config.path).await {
                warn!("Failed to save metadata for capture {}: {}", event_id, e);
            }
        } else {
            debug!("Skipping metadata save (disabled in config)");
        }

        if config.video_encoding && total_frames > 0 {
            let job = VideoGenerationJob {
                event_id: event_id.clone(),
                capture_dir: capture_task.capture_dir.clone(),
                wal_path,
                frame_count,
                camera_fps,
            };

            if let Err(e) = video_queue_tx.send(job) {
                error!(
                    "Failed to queue video generation for capture {}: {}",
                    event_id, e
                );
            } else {
                info!(
                    "Queued video generation for capture {} ({} frames)",
                    event_id, total_frames
                );
            }
        } else {
            info!("Video encoding disabled or no frames captured");
            if let Err(e) = delete_wal(&wal_path).await {
                warn!("Failed to delete WAL file: {}", e);
            }
        }

        let saved_files = 1;

        if let Err(e) = event_bus
            .publish(DoorcamEvent::CaptureCompleted {
                event_id: event_id.clone(),
                file_count: saved_files,
            })
            .await
        {
            error!("Failed to publish capture completed event: {}", e);
        }

        {
            let mut captures = active_captures.write().await;
            captures.retain(|c| c.event_id != event_id);
        }

        info!("Capture event task {} completed", event_id);
        Ok(())
    }

    /// Get statistics about active captures
    pub async fn get_capture_stats(&self) -> CaptureStats {
        let captures = self.active_captures.read().await;

        CaptureStats {
            active_captures: captures.len(),
            total_active_frames: 0,
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
            let event_id = wal_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            info!("Recovering capture: {} (from WAL)", event_id);

            let reader = WalReader::new(wal_path.clone());
            let frames = reader.read_all_frames().await?;
            let frame_count = frames.len() as u32;

            let capture_dir = PathBuf::from(&self.config.path).join(&event_id);
            if !capture_dir.exists() && self.config.keep_images {
                fs::create_dir_all(&capture_dir).await.map_err(|e| {
                    DoorcamError::component(
                        "video_capture",
                        &format!("Failed to create capture directory: {}", e),
                    )
                })?;
                info!(
                    "Created missing capture directory for recovered event: {}",
                    event_id
                );
            }

            let job = VideoGenerationJob {
                event_id: event_id.clone(),
                capture_dir,
                wal_path,
                frame_count,
                camera_fps: self.camera_fps,
            };

            if let Err(e) = self.video_queue_tx.send(job) {
                error!("Failed to queue recovered capture for encoding: {}", e);
            } else {
                info!(
                    "Queued recovered capture {} for encoding ({} frames)",
                    event_id, frame_count
                );
            }
        }

        Ok(())
    }

    /// Stop the video capture system
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping video capture system");

        let active_captures = {
            let captures = self.active_captures.read().await;
            captures.clone()
        };

        for capture in active_captures {
            warn!("Cancelling capture on shutdown: {}", capture.event_id);
            capture.cancel();
        }

        sleep(Duration::from_millis(500)).await;

        info!("Video capture system stopped");
        Ok(())
    }
}

impl Clone for VideoCapture {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            event_config: self.event_config.clone(),
            camera_fps: self.camera_fps,
            event_bus: Arc::clone(&self.event_bus),
            ring_buffer: Arc::clone(&self.ring_buffer),
            active_captures: Arc::clone(&self.active_captures),
            video_queue_tx: self.video_queue_tx.clone(),
        }
    }
}
