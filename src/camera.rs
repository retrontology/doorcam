use crate::config::CameraConfig;
use crate::error::{CameraError, DoorcamError, Result};
use crate::frame::{FrameData, FrameFormat};
use crate::recovery::{CameraRecovery, RecoveryAction};
use crate::ring_buffer::RingBuffer;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::time::{interval, sleep};
use tracing::{debug, error, info, trace, warn};

#[cfg(all(feature = "camera", target_os = "linux"))]
use v4l::prelude::*;
#[cfg(all(feature = "camera", target_os = "linux"))]
use v4l::buffer::Type;
#[cfg(all(feature = "camera", target_os = "linux"))]
use v4l::io::mmap::Stream;
#[cfg(all(feature = "camera", target_os = "linux"))]
use v4l::io::traits::CaptureStream;

/// Camera interface for V4L2 video capture
pub struct CameraInterface {
    config: CameraConfig,
    frame_counter: AtomicU64,
    is_running: AtomicBool,
    recovery: Arc<tokio::sync::Mutex<CameraRecovery>>,
    #[cfg(all(feature = "camera", target_os = "linux"))]
    device: Option<Arc<v4l::Device>>,
}



impl CameraInterface {
    /// Create a new camera interface with the given configuration
    pub async fn new(config: CameraConfig) -> Result<Self> {
        info!(
            "Initializing camera interface for device {} ({}x{} @ {}fps, format: {})",
            config.index, config.resolution.0, config.resolution.1, config.max_fps, config.format
        );
        
        let mut camera = Self {
            config,
            frame_counter: AtomicU64::new(0),
            is_running: AtomicBool::new(false),
            recovery: Arc::new(tokio::sync::Mutex::new(CameraRecovery::new())),
            #[cfg(all(feature = "camera", target_os = "linux"))]
            device: None,
        };
        
        camera.initialize_device().await?;
        
        Ok(camera)
    }
    
    /// Initialize the V4L2 device
    #[cfg(all(feature = "camera", target_os = "linux"))]
    async fn initialize_device(&mut self) -> Result<()> {
        let device_path = format!("/dev/video{}", self.config.index);
        debug!("Opening V4L2 device: {}", device_path);
        
        let device = v4l::Device::new(&device_path)
            .map_err(|e| CameraError::DeviceOpenWithSource {
                device: self.config.index,
                details: e.to_string(),
            })?;
        
        // Configure video format
        let mut fmt = device.format()
            .map_err(|e| CameraError::Configuration { 
                details: format!("Failed to get format: {}", e) 
            })?;
        
        fmt.width = self.config.resolution.0;
        fmt.height = self.config.resolution.1;
        fmt.fourcc = self.parse_format(&self.config.format)?;
        
        device.set_format(&fmt)
            .map_err(|e| CameraError::Configuration { 
                details: format!("Failed to set format: {}", e) 
            })?;
        
        // Verify the format was set correctly
        let actual_fmt = device.format()
            .map_err(|e| CameraError::Configuration { 
                details: format!("Failed to verify format: {}", e) 
            })?;
        
        if actual_fmt.width != self.config.resolution.0 || actual_fmt.height != self.config.resolution.1 {
            warn!(
                "Camera resolution adjusted by driver: requested {}x{}, got {}x{}",
                self.config.resolution.0, self.config.resolution.1,
                actual_fmt.width, actual_fmt.height
            );
        }
        
        // Set frame rate
        let mut params = device.params()
            .map_err(|e| CameraError::Configuration { 
                details: format!("Failed to get params: {}", e) 
            })?;
        
        params.interval = v4l::Fraction::new(1, self.config.max_fps);
        
        device.set_params(&params)
            .map_err(|e| CameraError::Configuration { 
                details: format!("Failed to set frame rate: {}", e) 
            })?;
        
        // Verify frame rate
        let actual_params = device.params()
            .map_err(|e| CameraError::Configuration { 
                details: format!("Failed to verify params: {}", e) 
            })?;
        
        let actual_fps = actual_params.interval.denominator / actual_params.interval.numerator;
        if actual_fps != self.config.max_fps {
            warn!(
                "Camera frame rate adjusted by driver: requested {}fps, got {}fps",
                self.config.max_fps, actual_fps
            );
        }
        
        info!(
            "Camera configured: {}x{} @ {}fps, format: {:?}",
            actual_fmt.width, actual_fmt.height, actual_fps, actual_fmt.fourcc
        );
        
        self.device = Some(Arc::new(device));
        Ok(())
    }
    
    /// Initialize device when camera feature is disabled or not on Linux
    #[cfg(not(all(feature = "camera", target_os = "linux")))]
    async fn initialize_device(&mut self) -> Result<()> {
        #[cfg(not(target_os = "linux"))]
        warn!("V4L2 camera interface is only available on Linux, using mock implementation");
        #[cfg(not(feature = "camera"))]
        warn!("Camera feature is disabled, using mock implementation");
        Ok(())
    }
    
    /// Parse format string to V4L2 FourCC
    #[cfg(all(feature = "camera", target_os = "linux"))]
    fn parse_format(&self, format: &str) -> Result<v4l::FourCC, CameraError> {
        match format.to_uppercase().as_str() {
            "MJPG" | "MJPEG" => Ok(v4l::FourCC::new(b"MJPG")),
            "YUYV" => Ok(v4l::FourCC::new(b"YUYV")),
            "RGB24" => Ok(v4l::FourCC::new(b"RGB3")),
            _ => Err(CameraError::UnsupportedFormat { 
                format: format.to_string() 
            }),
        }
    }
    
    /// Convert V4L2 FourCC to FrameFormat
    #[cfg(all(feature = "camera", target_os = "linux"))]
    fn fourcc_to_frame_format(&self, fourcc: v4l::FourCC) -> FrameFormat {
        match fourcc.str() {
            Ok("MJPG") => FrameFormat::Mjpeg,
            Ok("YUYV") => FrameFormat::Yuyv,
            Ok("RGB3") => FrameFormat::Rgb24,
            _ => {
                warn!("Unknown FourCC format: {:?}, defaulting to MJPEG", fourcc);
                FrameFormat::Mjpeg
            }
        }
    }
    
    /// Start camera capture and feed frames to the ring buffer
    pub async fn start_capture(&self, ring_buffer: Arc<RingBuffer>) -> Result<()> {
        if self.is_running.load(Ordering::Relaxed) {
            warn!("Camera capture is already running");
            return Ok(());
        }
        
        info!("Starting camera capture");
        self.is_running.store(true, Ordering::Relaxed);
        
        #[cfg(all(feature = "camera", target_os = "linux"))]
        {
            if let Some(device) = &self.device {
                self.run_capture_loop(Arc::clone(device), ring_buffer).await?;
            } else {
                return Err(CameraError::Configuration { 
                    details: "Device not initialized".to_string() 
                }.into());
            }
        }
        
        #[cfg(not(all(feature = "camera", target_os = "linux")))]
        {
            self.run_mock_capture_loop(ring_buffer).await?;
        }
        
        Ok(())
    }
    
    /// Run the actual V4L2 capture loop
    #[cfg(all(feature = "camera", target_os = "linux"))]
    async fn run_capture_loop(&self, device: Arc<v4l::Device>, ring_buffer: Arc<RingBuffer>) -> Result<()> {
        let config = self.config.clone();
        
        tokio::spawn(async move {
            let frame_counter = AtomicU64::new(0);
            let mut retry_count = 0;
            const MAX_RETRIES: u32 = 5;
            const RETRY_DELAY: Duration = Duration::from_secs(1);
            
            loop {
                match Self::capture_loop_inner(&device, &ring_buffer, &config, &frame_counter).await {
                    Ok(_) => {
                        info!("Camera capture loop ended normally");
                        break;
                    }
                    Err(e) => {
                        error!("Camera capture error: {}", e);
                        retry_count += 1;
                        
                        if retry_count >= MAX_RETRIES {
                            error!("Camera capture failed after {} retries, giving up", MAX_RETRIES);
                            break;
                        }
                        
                        warn!("Retrying camera capture in {:?} (attempt {}/{})", RETRY_DELAY, retry_count, MAX_RETRIES);
                        sleep(RETRY_DELAY * retry_count).await; // Exponential backoff
                    }
                }
            }
        });
        
        Ok(())
    }
    
    /// Inner capture loop implementation
    #[cfg(all(feature = "camera", target_os = "linux"))]
    async fn capture_loop_inner(
        device: &Arc<v4l::Device>,
        ring_buffer: &Arc<RingBuffer>,
        config: &CameraConfig,
        frame_counter: &AtomicU64,
    ) -> Result<(), CameraError> {
        let mut stream = Stream::with_buffers(device, Type::VideoCapture, 4)
            .map_err(|e| CameraError::CaptureStream { 
                details: format!("Failed to create stream: {}", e) 
            })?;
        
        let frame_interval = Duration::from_millis(1000 / config.max_fps as u64);
        let mut interval_timer = interval(frame_interval);
        
        info!("Camera capture loop started with {}ms frame interval", frame_interval.as_millis());
        
        // Run for a reasonable number of frames in this implementation
        let mut frame_count = 0;
        const MAX_FRAMES: u64 = 10000;
        
        while frame_count < MAX_FRAMES {
            interval_timer.tick().await;
            
            match stream.next() {
                Ok((buffer, meta)) => {
                    let frame_id = frame_counter.fetch_add(1, Ordering::Relaxed);
                    let timestamp = SystemTime::now();
                    
                    // Determine frame format from device format
                    let device_fmt = device.format()
                        .map_err(|e| CameraError::Configuration { 
                            details: format!("Failed to get format: {}", e) 
                        })?;
                    let frame_format = match device_fmt.fourcc.str() {
                        Ok("MJPG") => FrameFormat::Mjpeg,
                        Ok("YUYV") => FrameFormat::Yuyv,
                        Ok("RGB3") => FrameFormat::Rgb24,
                        _ => FrameFormat::Mjpeg, // Default fallback
                    };
                    
                    let frame_data = FrameData::new(
                        frame_id,
                        timestamp,
                        buffer.to_vec(),
                        device_fmt.width,
                        device_fmt.height,
                        frame_format,
                    );
                    
                    trace!(
                        "Captured frame {} ({}x{}, {} bytes, format: {:?})",
                        frame_id,
                        device_fmt.width,
                        device_fmt.height,
                        buffer.len(),
                        frame_format
                    );
                    
                    ring_buffer.push_frame(frame_data).await;
                    frame_count += 1;
                }
                Err(e) => {
                    error!("Frame capture error: {}", e);
                    return Err(CameraError::CaptureStream { 
                        details: format!("Capture failed: {}", e) 
                    });
                }
            }
        }
        
        info!("Camera capture loop stopped");
        Ok(())
    }
    
    /// Run mock capture loop when camera feature is disabled or not on Linux
    #[cfg(not(all(feature = "camera", target_os = "linux")))]
    async fn run_mock_capture_loop(&self, ring_buffer: Arc<RingBuffer>) -> Result<()> {
        let config = self.config.clone();
        
        tokio::spawn(async move {
            let frame_counter = AtomicU64::new(0);
            let frame_interval = Duration::from_millis(1000 / config.max_fps as u64);
            let mut interval_timer = interval(frame_interval);
            
            info!("Mock camera capture loop started");
            
            // Run for a reasonable duration in mock mode
            let mut frame_count = 0;
            const MAX_MOCK_FRAMES: u64 = 1000;
            
            while frame_count < MAX_MOCK_FRAMES {
                interval_timer.tick().await;
                
                let frame_id = frame_counter.fetch_add(1, Ordering::Relaxed);
                let timestamp = SystemTime::now();
                
                // Generate mock frame data (solid color pattern)
                let width = config.resolution.0;
                let height = config.resolution.1;
                let frame_size = (width * height * 3) as usize; // RGB24
                let mut data = vec![0u8; frame_size];
                
                // Fill with a simple pattern based on frame ID
                let color = ((frame_id % 256) as u8, 128u8, ((255 - frame_id % 256) as u8));
                for chunk in data.chunks_mut(3) {
                    chunk[0] = color.0; // R
                    chunk[1] = color.1; // G
                    chunk[2] = color.2; // B
                }
                
                let frame_data = FrameData::new(
                    frame_id,
                    timestamp,
                    data,
                    width,
                    height,
                    FrameFormat::Rgb24,
                );
                
                trace!("Generated mock frame {} ({}x{})", frame_id, width, height);
                ring_buffer.push_frame(frame_data).await;
                
                frame_count += 1;
            }
            
            info!("Mock camera capture loop stopped after {} frames", frame_count);
        });
        
        Ok(())
    }
    
    /// Stop camera capture
    pub async fn stop_capture(&self) -> Result<()> {
        if !self.is_running.load(Ordering::Relaxed) {
            debug!("Camera capture is not running");
            return Ok(());
        }
        
        info!("Stopping camera capture");
        self.is_running.store(false, Ordering::Relaxed);
        
        // Wait a bit for the capture loop to stop gracefully
        sleep(Duration::from_millis(100)).await;
        
        info!("Camera capture stopped");
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
    
    /// Attempt to reconnect the camera with retry logic
    pub async fn reconnect(&mut self) -> Result<()> {
        info!("Attempting to reconnect camera");
        
        // Stop current capture if running
        if self.is_capturing() {
            self.stop_capture().await?;
        }
        
        // Reinitialize device
        self.initialize_device().await?;
        
        // Test connection
        self.test_connection().await?;
        
        info!("Camera reconnected successfully");
        Ok(())
    }
    
    /// Start capture with automatic retry on failure
    pub async fn start_capture_with_retry(
        &self, 
        ring_buffer: Arc<RingBuffer>,
        max_retries: u32,
        retry_delay: Duration,
    ) -> Result<()> {
        let mut retry_count = 0;
        
        loop {
            match self.start_capture(Arc::clone(&ring_buffer)).await {
                Ok(()) => {
                    info!("Camera capture started successfully");
                    return Ok(());
                }
                Err(e) => {
                    error!("Camera capture failed: {}", e);
                    retry_count += 1;
                    
                    if retry_count >= max_retries {
                        error!("Camera capture failed after {} retries", max_retries);
                        return Err(e);
                    }
                    
                    warn!(
                        "Retrying camera capture in {:?} (attempt {}/{})",
                        retry_delay * retry_count,
                        retry_count,
                        max_retries
                    );
                    
                    sleep(retry_delay * retry_count).await; // Exponential backoff
                }
            }
        }
    }
    
    /// Handle camera error with recovery logic
    pub async fn handle_error_with_recovery(&self, error: CameraError) -> RecoveryAction {
        let mut recovery = self.recovery.lock().await;
        recovery.handle_camera_error(&error)
    }
    
    /// Attempt to recover from camera failure
    pub async fn recover(&self) -> Result<()> {
        info!("Attempting camera recovery");
        
        let mut recovery = self.recovery.lock().await;
        
        recovery.recover_camera(|| async {
            self.reinitialize_device().await
        }).await
    }
    
    /// Reinitialize camera device (used for recovery)
    async fn reinitialize_device(&self) -> std::result::Result<(), CameraError> {
        #[cfg(all(feature = "camera", target_os = "linux"))]
        {
            // This is a simplified version - in practice you'd need to handle
            // the device field being in an Arc and potentially recreate it
            info!("Reinitializing camera device {}", self.config.index);
            
            let device_path = format!("/dev/video{}", self.config.index);
            let device = v4l::Device::new(&device_path)
                .map_err(|e| CameraError::DeviceOpenWithSource {
                    device: self.config.index,
                    details: e.to_string(),
                })?;
            
            // Configure the device (similar to initialize_device but simpler)
            let mut fmt = device.format()
                .map_err(|e| CameraError::Configuration { 
                    details: format!("Failed to get format during recovery: {}", e) 
                })?;
            
            fmt.width = self.config.resolution.0;
            fmt.height = self.config.resolution.1;
            fmt.fourcc = self.format_to_fourcc(&self.config.format)?;
            
            device.set_format(&fmt)
                .map_err(|e| CameraError::Configuration { 
                    details: format!("Failed to set format during recovery: {}", e) 
                })?;
            
            info!("Camera device {} reinitialized successfully", self.config.index);
            Ok(())
        }
        
        #[cfg(not(all(feature = "camera", target_os = "linux")))]
        {
            warn!("Camera recovery not available on this platform");
            Err(CameraError::NotAvailable)
        }
    }
    
    /// Reset recovery state after successful operation
    pub async fn reset_recovery(&self) {
        let mut recovery = self.recovery.lock().await;
        recovery.reset();
    }
    
    /// Test camera connectivity and configuration
    pub async fn test_connection(&self) -> Result<()> {
        #[cfg(all(feature = "camera", target_os = "linux"))]
        {
            if let Some(device) = &self.device {
                // Try to get current format to test device access
                let fmt = device.format()
                    .map_err(|e| CameraError::Configuration { 
                        details: format!("Device test failed: {}", e) 
                    })?;
                
                debug!(
                    "Camera test successful: {}x{} format {:?}",
                    fmt.width, fmt.height, fmt.fourcc
                );
                
                Ok(())
            } else {
                Err(CameraError::Configuration { 
                    details: "Device not initialized".to_string() 
                }.into())
            }
        }
        
        #[cfg(not(all(feature = "camera", target_os = "linux")))]
        {
            debug!("Camera test successful (mock mode)");
            Ok(())
        }
    }
}

/// Camera interface builder for easier configuration
pub struct CameraInterfaceBuilder {
    config: Option<CameraConfig>,
}

impl CameraInterfaceBuilder {
    /// Create a new camera interface builder
    pub fn new() -> Self {
        Self { config: None }
    }
    
    /// Set camera configuration
    pub fn config(mut self, config: CameraConfig) -> Self {
        self.config = Some(config);
        self
    }
    
    /// Build the camera interface
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
    use std::time::Duration;
    use tokio::time::timeout;
    
    fn create_test_config() -> CameraConfig {
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
        let config = create_test_config();
        let camera = CameraInterface::new(config).await;
        
        // Should succeed even without actual hardware (mock mode)
        assert!(camera.is_ok());
        
        let camera = camera.unwrap();
        assert_eq!(camera.config().index, 0);
        assert_eq!(camera.config().resolution, (640, 480));
        assert_eq!(camera.config().max_fps, 30);
        assert!(!camera.is_capturing());
    }
    
    #[tokio::test]
    async fn test_camera_builder() {
        let config = create_test_config();
        let camera = CameraInterfaceBuilder::new()
            .config(config)
            .build()
            .await;
        
        assert!(camera.is_ok());
    }
    
    #[tokio::test]
    async fn test_mock_capture() {
        let config = create_test_config();
        let camera = CameraInterface::new(config).await.unwrap();
        let ring_buffer = Arc::new(RingBuffer::new(10, Duration::from_secs(1)));
        
        // Start capture
        camera.start_capture(Arc::clone(&ring_buffer)).await.unwrap();
        assert!(camera.is_capturing());
        
        // Wait for some frames
        timeout(Duration::from_millis(200), async {
            while ring_buffer.get_latest_frame().await.is_none() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("Should capture frames within timeout");
        
        // Verify we got frames
        let latest_frame = ring_buffer.get_latest_frame().await;
        assert!(latest_frame.is_some());
        
        let frame = latest_frame.unwrap();
        assert_eq!(frame.width, 640);
        assert_eq!(frame.height, 480);
        assert_eq!(frame.format, FrameFormat::Rgb24);
        
        // Stop capture
        camera.stop_capture().await.unwrap();
        assert!(!camera.is_capturing());
    }
    
    #[tokio::test]
    async fn test_camera_test_connection() {
        let config = create_test_config();
        let camera = CameraInterface::new(config).await.unwrap();
        
        // Test connection should work in mock mode
        let result = camera.test_connection().await;
        assert!(result.is_ok());
    }
    
    #[cfg(all(feature = "camera", target_os = "linux"))]
    #[test]
    fn test_format_parsing() {
        let config = create_test_config();
        let camera = CameraInterface {
            config,
            frame_counter: AtomicU64::new(0),
            is_running: AtomicBool::new(false),
            device: None,
        };
        
        assert!(camera.parse_format("MJPG").is_ok());
        assert!(camera.parse_format("mjpg").is_ok());
        assert!(camera.parse_format("YUYV").is_ok());
        assert!(camera.parse_format("RGB24").is_ok());
        assert!(camera.parse_format("INVALID").is_err());
    }
}