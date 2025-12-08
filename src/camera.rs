use crate::config::CameraConfig;
use crate::error::{CameraError, DoorcamError, Result};
use crate::frame::{FrameData, FrameFormat};
use crate::ring_buffer::RingBuffer;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

#[cfg(all(feature = "camera", target_os = "linux"))]
use gstreamer::prelude::*;
#[cfg(all(feature = "camera", target_os = "linux"))]
use gstreamer::Pipeline;
#[cfg(all(feature = "camera", target_os = "linux"))]
use gstreamer_app::AppSink;
#[cfg(all(feature = "camera", target_os = "linux"))]
use gstreamer_video::VideoInfo;

/// GStreamer-based camera interface with hardware acceleration support
pub struct CameraInterface {
    config: CameraConfig,
    frame_counter: Arc<AtomicU64>,
    is_running: Arc<AtomicBool>,
    #[cfg(all(feature = "camera", target_os = "linux"))]
    pipeline: Option<Pipeline>,
    capture_task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl CameraInterface {
    /// Create a new GStreamer camera interface
    pub async fn new(config: CameraConfig) -> Result<Self> {
        info!(
            "Initializing GStreamer camera interface for device {} ({}x{} @ {}fps)",
            config.index, config.resolution.0, config.resolution.1, config.max_fps
        );
        
        #[cfg(all(feature = "camera", target_os = "linux"))]
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
            #[cfg(all(feature = "camera", target_os = "linux"))]
            pipeline: None,
            capture_task: Arc::new(tokio::sync::Mutex::new(None)),
        };
        
        camera.initialize_pipeline().await?;
        
        Ok(camera)
    }
    
    /// Initialize GStreamer pipeline with hardware acceleration
    #[cfg(all(feature = "camera", target_os = "linux"))]
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
    #[cfg(all(feature = "camera", target_os = "linux"))]
    fn build_pipeline_string(&self) -> Result<String> {
        let (width, height) = self.config.resolution;
        let fps = self.config.max_fps;
        let device_index = self.config.index;
        
        // Simple MJPEG pipeline - capture JPEG frames directly without decoding
        // Use io-mode=mmap for efficient memory-mapped I/O
        let pipeline = format!(
            "v4l2src device=/dev/video{} io-mode=mmap ! \
             image/jpeg,width={},height={},framerate={}/1 ! \
             appsink name=sink sync=false max-buffers=2 drop=true qos=true emit-signals=false",
            device_index, width, height, fps
        );
        
        Ok(pipeline)
    }
    
    /// Initialize pipeline when GStreamer feature is disabled
    #[cfg(not(all(feature = "camera", target_os = "linux")))]
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
        
        #[cfg(all(feature = "camera", target_os = "linux"))]
        {
            if let Some(pipeline) = &self.pipeline {
                self.run_gst_capture_loop(pipeline.clone(), ring_buffer).await?;
            } else {
                return Err(CameraError::Configuration {
                    details: "Pipeline not initialized".to_string(),
                }.into());
            }
        }
        
        #[cfg(not(all(feature = "camera", target_os = "linux")))]
        {
            self.run_mock_capture_loop(ring_buffer).await?;
        }
        
        Ok(())
    }
    
    /// Run GStreamer capture loop
    #[cfg(all(feature = "camera", target_os = "linux"))]
    async fn run_gst_capture_loop(&self, pipeline: Pipeline, ring_buffer: Arc<RingBuffer>) -> Result<()> {
        let is_running = Arc::clone(&self.is_running);
        let capture_task = Arc::clone(&self.capture_task);
        let frame_counter = Arc::clone(&self.frame_counter);
        
        let task = tokio::spawn(async move {
            // Get the appsink element
            let appsink = pipeline
                .by_name("sink")
                .expect("Failed to get appsink")
                .downcast::<AppSink>()
                .expect("Failed to downcast to AppSink");
            
            // Set up sample callback
            let (tx, mut rx) = mpsc::unbounded_channel();
            
            appsink.set_callbacks(
                gstreamer_app::AppSinkCallbacks::builder()
                    .new_sample(move |appsink| {
                        let sample = appsink.pull_sample().map_err(|_| gstreamer::FlowError::Eos)?;
                        let _ = tx.send(sample);
                        Ok(gstreamer::FlowSuccess::Ok)
                    })
                    .build(),
            );
            
            // Start pipeline
            if let Err(e) = pipeline.set_state(gstreamer::State::Playing) {
                error!("Failed to start GStreamer pipeline: {}", e);
                return;
            }
            
            info!("GStreamer pipeline started successfully");
            
            // Process samples
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
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        // Periodic check for shutdown
                    }
                }
            }
            
            // Stop pipeline
            let _ = pipeline.set_state(gstreamer::State::Null);
            info!("GStreamer capture loop stopped");
        });
        
        *capture_task.lock().await = Some(task);
        Ok(())
    }
    
    /// Process a GStreamer sample into a frame
    #[cfg(all(feature = "camera", target_os = "linux"))]
    async fn process_gst_sample(
        sample: gstreamer::Sample,
        frame_counter: &Arc<AtomicU64>,
        ring_buffer: &Arc<RingBuffer>,
    ) -> Result<()> {
        let buffer = sample.buffer().ok_or_else(|| {
            CameraError::CaptureStream {
                details: "No buffer in sample".to_string(),
            }
        })?;
        
        let caps = sample.caps().ok_or_else(|| {
            CameraError::CaptureStream {
                details: "No caps in sample".to_string(),
            }
        })?;
        
        // Get video info from caps
        let video_info = VideoInfo::from_caps(caps).map_err(|e| {
            CameraError::CaptureStream {
                details: format!("Failed to get video info: {}", e),
            }
        })?;
        
        let width = video_info.width();
        let height = video_info.height();
        
        // Map buffer for reading
        let map = buffer.map_readable().map_err(|e| {
            CameraError::CaptureStream {
                details: format!("Failed to map buffer: {}", e),
            }
        })?;
        
        let frame_id = frame_counter.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now();
        
        // Store the raw JPEG data directly - much more efficient!
        let frame_data = FrameData::new(
            frame_id,
            timestamp,
            map.as_slice().to_vec(),
            width,
            height,
            FrameFormat::Mjpeg, // Store as MJPEG
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
    #[cfg(not(all(feature = "camera", target_os = "linux")))]
    async fn run_mock_capture_loop(&self, ring_buffer: Arc<RingBuffer>) -> Result<()> {
        // Same mock implementation as the v4l version
        let config = self.config.clone();
        let is_running = Arc::clone(&self.is_running);
        let capture_task = Arc::clone(&self.capture_task);
        let frame_counter = Arc::clone(&self.frame_counter);
        
        let task = tokio::spawn(async move {
            let frame_interval = Duration::from_millis(1000 / config.max_fps as u64);
            let mut interval_timer = tokio::time::interval(frame_interval);
            
            info!("Mock GStreamer capture loop started");
            
            while is_running.load(Ordering::Relaxed) {
                interval_timer.tick().await;
                
                if !is_running.load(Ordering::Relaxed) {
                    break;
                }
                
                let frame_id = frame_counter.fetch_add(1, Ordering::Relaxed);
                let timestamp = SystemTime::now();
                
                // Generate mock MJPEG frame data (minimal JPEG header + data)
                let width = config.resolution.0;
                let height = config.resolution.1;
                
                // Create a minimal mock JPEG frame (just header bytes for testing)
                let mut data = vec![
                    0xFF, 0xD8, // JPEG SOI (Start of Image)
                    0xFF, 0xE0, // JFIF marker
                    0x00, 0x10, // Length
                    0x4A, 0x46, 0x49, 0x46, 0x00, // "JFIF\0"
                    0x01, 0x01, // Version 1.1
                    0x01, // Units (1 = pixels per inch)
                    0x00, 0x48, 0x00, 0x48, // X and Y density (72 DPI)
                    0x00, 0x00, // Thumbnail width and height (0 = no thumbnail)
                ];
                
                // Add some mock image data based on frame ID
                let pattern_size = 1000 + (frame_id % 500) as usize;
                let pattern_byte = (frame_id % 256) as u8;
                data.extend(vec![pattern_byte; pattern_size]);
                
                // Add JPEG EOI (End of Image)
                data.extend_from_slice(&[0xFF, 0xD9]);
                
                let frame_data = FrameData::new(
                    frame_id,
                    timestamp,
                    data,
                    width,
                    height,
                    FrameFormat::Mjpeg,
                );
                
                trace!("Generated mock MJPEG frame {} ({}x{}, {} bytes)", frame_id, width, height, data.len());
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
        #[cfg(all(feature = "camera", target_os = "linux"))]
        {
            if let Some(pipeline) = &self.pipeline {
                // Test pipeline by setting it to READY state
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
                }.into())
            }
        }
        
        #[cfg(not(all(feature = "camera", target_os = "linux")))]
        {
            debug!("GStreamer pipeline test successful (mock mode)");
            Ok(())
        }
    }
}

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
        let config = self.config.ok_or_else(|| {
            DoorcamError::system("Camera configuration must be specified")
        })?;
        
        CameraInterface::new(config).await
    }
}

impl Default for CameraInterfaceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CameraConfig;

    fn create_test_camera_config() -> CameraConfig {
        CameraConfig {
            index: 0,
            resolution: (640, 480),
            max_fps: 30,
            format: "MJPG".to_string(),
            rotation: None,
        }
    }

    #[tokio::test]
    async fn test_camera_interface_creation() {
        let config = create_test_camera_config();
        
        // This may fail if no camera hardware is available, which is expected in CI
        match CameraInterface::new(config).await {
            Ok(camera) => {
                assert!(!camera.is_capturing());
                assert_eq!(camera.frame_count(), 0);
            }
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen { .. })) => {
                // Expected when no camera hardware is available
                println!("Camera hardware not available - test passed");
            }
            Err(e) => {
                panic!("Unexpected error creating camera interface: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_camera_builder_pattern() {
        let config = create_test_camera_config();
        
        let builder = CameraInterfaceBuilder::new()
            .config(config);
        
        match builder.build().await {
            Ok(camera) => {
                assert!(!camera.is_capturing());
            }
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen { .. })) => {
                // Expected when no camera hardware is available
                println!("Camera hardware not available - builder test passed");
            }
            Err(e) => {
                panic!("Unexpected error in camera builder: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_camera_builder_validation() {
        let builder = CameraInterfaceBuilder::new();
        
        // Should fail without config
        let result = builder.build().await;
        assert!(result.is_err());
        
        if let Err(crate::error::DoorcamError::System { message }) = result {
            assert!(message.contains("Camera configuration must be specified"));
        } else {
            panic!("Expected system error for missing configuration");
        }
    }

    #[tokio::test]
    async fn test_camera_test_connection() {
        let config = create_test_camera_config();
        
        match CameraInterface::new(config).await {
            Ok(camera) => {
                // Test connection should work even if camera isn't capturing
                let result = camera.test_connection().await;
                // Result may vary based on hardware availability
                match result {
                    Ok(()) => {
                        println!("Camera connection test passed");
                    }
                    Err(_) => {
                        println!("Camera connection test failed - expected without hardware");
                    }
                }
            }
            Err(crate::error::DoorcamError::Camera(crate::error::CameraError::DeviceOpen { .. })) => {
                println!("Camera hardware not available - skipping connection test");
            }
            Err(e) => {
                panic!("Unexpected error: {}", e);
            }
        }
    }

    #[test]
    fn test_camera_config_validation() {
        let config = CameraConfig {
            index: 0,
            resolution: (640, 480),
            max_fps: 30,
            format: "MJPG".to_string(),
            rotation: None,
        };
        
        // Basic validation - config should be valid
        assert_eq!(config.index, 0);
        assert_eq!(config.resolution, (640, 480));
        assert_eq!(config.max_fps, 30);
        assert_eq!(config.format, "MJPG");
        assert!(config.rotation.is_none());
    }
}