use crate::config::CameraConfig;
use crate::error::{CameraError, DoorcamError, Result};
use crate::frame::{FrameData, FrameFormat};
use crate::ring_buffer::RingBuffer;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

#[cfg(target_os = "linux")]
use gstreamer::prelude::*;
#[cfg(target_os = "linux")]
use gstreamer::Pipeline;
#[cfg(target_os = "linux")]
use gstreamer_app::AppSink;
#[cfg(target_os = "linux")]
use gstreamer_video::VideoInfo;

/// GStreamer-based camera interface with hardware acceleration support
pub struct CameraInterface {
    config: CameraConfig,
    frame_counter: Arc<AtomicU64>,
    is_running: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    pipeline: Option<Pipeline>,
    capture_task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl CameraInterface {
    /// Create a new GStreamer camera interface
    pub async fn new(config: CameraConfig) -> Result<Self> {
        info!(
            "Initializing GStreamer camera interface for device {} ({}x{} @ {}fps)",
            config.index, config.resolution.0, config.resolution.1, config.fps
        );

        #[cfg(target_os = "linux")]
        {
            // Initialize GStreamer
            gstreamer::init().map_err(|e| {
                DoorcamError::Camera(CameraError::Configuration {
                    details: format!("Failed to initialize GStreamer: {}", e),
                })
            })?;
        }

        let mut camera = Self {
            config,
            frame_counter: Arc::new(AtomicU64::new(0)),
            is_running: Arc::new(AtomicBool::new(false)),
            #[cfg(target_os = "linux")]
            pipeline: None,
            capture_task: Arc::new(tokio::sync::Mutex::new(None)),
        };

        camera.initialize_pipeline().await?;

        Ok(camera)
    }

    /// Initialize GStreamer pipeline with hardware acceleration
    #[cfg(target_os = "linux")]
    async fn initialize_pipeline(&mut self) -> Result<()> {
        let pipeline_desc = self.build_pipeline_string()?;

        info!("Creating GStreamer pipeline: {}", pipeline_desc);

        let pipeline = gstreamer::parse::launch(&pipeline_desc)
            .map_err(|e| CameraError::Configuration {
                details: format!("Failed to create pipeline: {}", e),
            })?
            .downcast::<Pipeline>()
            .map_err(|_| CameraError::Configuration {
                details: "Failed to downcast to Pipeline".to_string(),
            })?;

        self.pipeline = Some(pipeline);

        Ok(())
    }

    /// Build GStreamer pipeline string for MJPEG capture
    #[cfg(target_os = "linux")]
    fn build_pipeline_string(&self) -> Result<String> {
        let (width, height) = self.config.resolution;
        let fps = self.config.fps;
        let device_index = self.config.index;

        let pipeline = format!(
            "v4l2src device=/dev/video{} io-mode=mmap do-timestamp=true ! \
             image/jpeg,width={},height={},framerate={}/1 ! \
             queue max-size-buffers=4 leaky=downstream ! \
             appsink name=sink sync=false max-buffers=10 drop=false qos=false enable-last-sample=false emit-signals=false",
            device_index, width, height, fps
        );

        Ok(pipeline)
    }

    /// Initialize pipeline when GStreamer feature is disabled
    #[cfg(not(target_os = "linux"))]
    async fn initialize_pipeline(&mut self) -> Result<()> {
        warn!("GStreamer camera interface is only available on Linux with camera feature");
        Ok(())
    }

    /// Start camera capture using GStreamer
    pub async fn start_capture(&self, ring_buffer: Arc<RingBuffer>) -> Result<()> {
        if self.is_running.load(Ordering::Relaxed) {
            warn!("GStreamer camera capture is already running");
            return Ok(());
        }

        info!("Starting GStreamer camera capture");
        self.is_running.store(true, Ordering::Relaxed);

        #[cfg(target_os = "linux")]
        {
            if let Some(pipeline) = &self.pipeline {
                self.run_gst_capture_loop(pipeline.clone(), ring_buffer)
                    .await?;
            } else {
                return Err(CameraError::Configuration {
                    details: "Pipeline not initialized".to_string(),
                }
                .into());
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            self.run_mock_capture_loop(ring_buffer).await?;
        }

        Ok(())
    }

    /// Run GStreamer capture loop
    #[cfg(target_os = "linux")]
    async fn run_gst_capture_loop(
        &self,
        pipeline: Pipeline,
        ring_buffer: Arc<RingBuffer>,
    ) -> Result<()> {
        let is_running = Arc::clone(&self.is_running);
        let capture_task = Arc::clone(&self.capture_task);
        let frame_counter = Arc::clone(&self.frame_counter);

        let task = tokio::spawn(async move {
            let appsink = pipeline
                .by_name("sink")
                .expect("Failed to get appsink")
                .downcast::<AppSink>()
                .expect("Failed to downcast to AppSink");

            let (tx, mut rx) = mpsc::unbounded_channel();
            let mut last_sample_time = tokio::time::Instant::now();
            let mut watchdog_interval = tokio::time::interval(Duration::from_secs(1));
            let watchdog_timeout = Duration::from_secs(5);

            appsink.set_callbacks(
                gstreamer_app::AppSinkCallbacks::builder()
                    .new_sample(move |appsink| {
                        let sample = appsink
                            .pull_sample()
                            .map_err(|_| gstreamer::FlowError::Eos)?;
                        let _ = tx.send(sample);
                        Ok(gstreamer::FlowSuccess::Ok)
                    })
                    .build(),
            );

            if let Err(e) = pipeline.set_state(gstreamer::State::Playing) {
                error!("Failed to start GStreamer pipeline: {}", e);
                return;
            }

            info!("GStreamer pipeline started successfully");

            while is_running.load(Ordering::Relaxed) {
                tokio::select! {
                    sample = rx.recv() => {
                        if let Some(sample) = sample {
                            if let Err(e) = Self::process_gst_sample(
                                sample,
                                &frame_counter,
                                &ring_buffer
                            ).await {
                                error!("Error processing GStreamer sample: {}", e);
                            }
                            last_sample_time = tokio::time::Instant::now();
                        }
                    }
                    _ = watchdog_interval.tick() => {
                        if last_sample_time.elapsed() >= watchdog_timeout {
                            warn!(
                                "No camera frames received for {:?}; restarting pipeline",
                                watchdog_timeout
                            );
                            let _ = pipeline.set_state(gstreamer::State::Null);
                            if let Err(e) = pipeline.set_state(gstreamer::State::Playing) {
                                error!("Failed to restart GStreamer pipeline: {}", e);
                            } else {
                                last_sample_time = tokio::time::Instant::now();
                            }
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        // Periodic check for shutdown
                    }
                }
            }

            let _ = pipeline.set_state(gstreamer::State::Null);
            info!("GStreamer capture loop stopped");
        });

        *capture_task.lock().await = Some(task);
        Ok(())
    }

    /// Process a GStreamer sample into a frame
    #[cfg(target_os = "linux")]
    async fn process_gst_sample(
        sample: gstreamer::Sample,
        frame_counter: &Arc<AtomicU64>,
        ring_buffer: &Arc<RingBuffer>,
    ) -> Result<()> {
        let buffer = sample.buffer().ok_or_else(|| CameraError::CaptureStream {
            details: "No buffer in sample".to_string(),
        })?;

        let caps = sample.caps().ok_or_else(|| CameraError::CaptureStream {
            details: "No caps in sample".to_string(),
        })?;

        let video_info = VideoInfo::from_caps(caps).map_err(|e| CameraError::CaptureStream {
            details: format!("Failed to get video info: {}", e),
        })?;

        let width = video_info.width();
        let height = video_info.height();

        let map = buffer
            .map_readable()
            .map_err(|e| CameraError::CaptureStream {
                details: format!("Failed to map buffer: {}", e),
            })?;

        let frame_id = frame_counter.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now();

        let frame_data = FrameData::new(
            frame_id,
            timestamp,
            map.as_slice().to_vec(),
            width,
            height,
            FrameFormat::Mjpeg,
        );

        trace!(
            "Captured MJPEG frame {} ({}x{}, {} bytes)",
            frame_id,
            width,
            height,
            map.len()
        );

        ring_buffer.push_frame(frame_data).await;

        Ok(())
    }

    /// Run mock capture loop when GStreamer is not available
    #[cfg(not(target_os = "linux"))]
    async fn run_mock_capture_loop(&self, ring_buffer: Arc<RingBuffer>) -> Result<()> {
        let config = self.config.clone();
        let is_running = Arc::clone(&self.is_running);
        let capture_task = Arc::clone(&self.capture_task);
        let frame_counter = Arc::clone(&self.frame_counter);

        let task = tokio::spawn(async move {
            let frame_interval = Duration::from_millis(1000 / config.fps as u64);
            let mut interval_timer = tokio::time::interval(frame_interval);

            info!("Mock GStreamer capture loop started");

            while is_running.load(Ordering::Relaxed) {
                interval_timer.tick().await;

                if !is_running.load(Ordering::Relaxed) {
                    break;
                }

                let frame_id = frame_counter.fetch_add(1, Ordering::Relaxed);
                let timestamp = SystemTime::now();

                let width = config.resolution.0;
                let height = config.resolution.1;

                let mut data = vec![
                    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01,
                    0x01, 0x00, 0x48, 0x00, 0x48, 0x00, 0x00,
                ];

                let pattern_size = 1000 + (frame_id % 500) as usize;
                let pattern_byte = (frame_id % 256) as u8;
                data.extend(vec![pattern_byte; pattern_size]);
                data.extend_from_slice(&[0xFF, 0xD9]);

                let data_len = data.len();
                let frame_data =
                    FrameData::new(frame_id, timestamp, data, width, height, FrameFormat::Mjpeg);

                trace!(
                    "Generated mock MJPEG frame {} ({}x{}, {} bytes)",
                    frame_id,
                    width,
                    height,
                    data_len
                );
                ring_buffer.push_frame(frame_data).await;
            }

            info!("Mock GStreamer capture loop stopped");
        });

        *capture_task.lock().await = Some(task);
        Ok(())
    }

    /// Stop camera capture
    pub async fn stop_capture(&self) -> Result<()> {
        if !self.is_running.load(Ordering::Relaxed) {
            debug!("GStreamer camera capture is not running");
            return Ok(());
        }

        info!("Stopping GStreamer camera capture");
        self.is_running.store(false, Ordering::Relaxed);

        if let Some(task) = self.capture_task.lock().await.take() {
            match tokio::time::timeout(Duration::from_secs(3), task).await {
                Ok(Ok(())) => {
                    info!("GStreamer capture task completed successfully");
                }
                Ok(Err(e)) => {
                    error!("Error waiting for GStreamer capture task: {}", e);
                }
                Err(_) => {
                    warn!("GStreamer capture task did not complete within timeout");
                }
            }
        }

        info!("GStreamer camera capture stopped");
        Ok(())
    }

    /// Check if camera is currently capturing
    pub fn is_capturing(&self) -> bool {
        self.is_running.load(Ordering::Relaxed)
    }

    /// Get camera configuration
    pub fn config(&self) -> &CameraConfig {
        &self.config
    }

    /// Get current frame count
    pub fn frame_count(&self) -> u64 {
        self.frame_counter.load(Ordering::Relaxed)
    }

    /// Test GStreamer pipeline
    pub async fn test_connection(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            if let Some(pipeline) = &self.pipeline {
                pipeline.set_state(gstreamer::State::Ready).map_err(|e| {
                    CameraError::Configuration {
                        details: format!("Pipeline test failed: {}", e),
                    }
                })?;

                pipeline.set_state(gstreamer::State::Null).map_err(|e| {
                    CameraError::Configuration {
                        details: format!("Failed to reset pipeline: {}", e),
                    }
                })?;

                debug!("GStreamer pipeline test successful");
                Ok(())
            } else {
                Err(CameraError::Configuration {
                    details: "Pipeline not initialized".to_string(),
                }
                .into())
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            debug!("GStreamer pipeline test successful (mock mode)");
            Ok(())
        }
    }
}
