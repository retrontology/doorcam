use crate::config::DisplayConfig;
use crate::events::{DoorcamEvent, EventBus, EventReceiver, EventFilter};
use crate::frame::{FrameData, FrameFormat};
use crate::error::{DisplayError, Result};
use std::fs::{File, OpenOptions};
use std::io::{Write, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

#[cfg(all(feature = "display", target_os = "linux"))]
use gstreamer::prelude::*;
#[cfg(all(feature = "display", target_os = "linux"))]
use gstreamer::Pipeline;
#[cfg(all(feature = "display", target_os = "linux"))]
use gstreamer_app::AppSrc;

/// Display controller for HyperPixel 4.0 with GStreamer hardware acceleration
pub struct DisplayController {
    config: DisplayConfig,
    backlight: Arc<RwLock<Option<File>>>,
    is_active: Arc<AtomicBool>,
    activation_timer: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    #[cfg(all(feature = "display", target_os = "linux"))]
    display_pipeline: Arc<RwLock<Option<Pipeline>>>,
    #[cfg(all(feature = "display", target_os = "linux"))]
    appsrc: Arc<RwLock<Option<AppSrc>>>,
}

impl DisplayController {
    /// Create a new display controller
    pub async fn new(config: DisplayConfig) -> Result<Self> {
        info!("Initializing GStreamer display controller for HyperPixel 4.0");
        debug!("Display config: {:?}", config);

        #[cfg(all(feature = "display", target_os = "linux"))]
        {
            // Initialize GStreamer
            gstreamer::init().map_err(|e| {
                DisplayError::Framebuffer {
                    details: format!("Failed to initialize GStreamer: {}", e),
                }
            })?;
        }

        let controller = Self {
            config,
            backlight: Arc::new(RwLock::new(None)),
            is_active: Arc::new(AtomicBool::new(false)),
            activation_timer: Arc::new(RwLock::new(None)),
            #[cfg(all(feature = "display", target_os = "linux"))]
            display_pipeline: Arc::new(RwLock::new(None)),
            #[cfg(all(feature = "display", target_os = "linux"))]
            appsrc: Arc::new(RwLock::new(None)),
        };

        // Initialize display pipeline and devices
        controller.initialize_display_pipeline().await?;
        controller.initialize_devices().await?;

        Ok(controller)
    }

    /// Initialize GStreamer display pipeline with hardware acceleration
    #[cfg(all(feature = "display", target_os = "linux"))]
    async fn initialize_display_pipeline(&self) -> Result<()> {
        let (display_width, display_height) = self.config.resolution;
        
        // Calculate pre-rotation dimensions based on rotation angle
        // For 90/270 degree rotations, we need to swap dimensions before rotating
        let (pre_rotation_width, pre_rotation_height) = match &self.config.rotation {
            Some(crate::config::Rotation::Rotate90) | Some(crate::config::Rotation::Rotate270) => {
                // Swap dimensions so after rotation they'll be correct
                (display_height, display_width)
            }
            _ => {
                // No rotation or 180 degree rotation - keep original dimensions
                (display_width, display_height)
            }
        };
        
        info!("Using display JPEG decode scale 1/{}", self.config.jpeg_decode_scale);

        // Build hardware-accelerated display pipeline with efficient JPEG decoding
        // Note: Using software jpegdec for display - v4l2jpegdec causes pipeline stalls
        // Hardware decoder works fine for video encoding but not for live display
        let mut pipeline_desc = "appsrc name=src format=bytes is-live=true caps=image/jpeg ! queue max-size-buffers=1 leaky=downstream ! jpegdec".to_string();
        
        // First downsize to pre-rotation dimensions
        // Use nearest-neighbour scaling for better performance (faster than bilinear)
        pipeline_desc.push_str(&format!(
            " ! videoconvert ! \
             videoscale method=nearest-neighbour ! \
             video/x-raw,width={},height={}",
            pre_rotation_width, pre_rotation_height
        ));
        
        // Then add rotation if configured (now operating on smaller image)
        if let Some(rotation) = &self.config.rotation {
            let flip_method = match rotation {
                crate::config::Rotation::Rotate90 => "clockwise",
                crate::config::Rotation::Rotate180 => "rotate-180", 
                crate::config::Rotation::Rotate270 => "counterclockwise",
            };
            
            pipeline_desc.push_str(&format!(" ! videoflip method={}", flip_method));
            let degrees = match rotation {
                crate::config::Rotation::Rotate90 => 90,
                crate::config::Rotation::Rotate180 => 180,
                crate::config::Rotation::Rotate270 => 270,
            };
            info!("Display rotation enabled: {} degrees ({}) - scaling to {}x{} before rotation to achieve final {}x{}", 
                  degrees, flip_method, pre_rotation_width, pre_rotation_height, display_width, display_height);
        }
        
        // Final format conversion and output with optimized buffering
        pipeline_desc.push_str(&format!(
            " ! videoconvert ! \
             video/x-raw,format=RGB16 ! \
             fbdevsink device={} sync=false max-lateness=-1 async=false",
            self.config.framebuffer_device
        ));

        info!("Creating GStreamer display pipeline: {}", pipeline_desc);

        let pipeline = gstreamer::parse::launch(&pipeline_desc)
            .map_err(|e| DisplayError::Framebuffer {
                details: format!("Failed to create display pipeline: {}", e),
            })?
            .downcast::<Pipeline>()
            .map_err(|_| DisplayError::Framebuffer {
                details: "Failed to downcast to Pipeline".to_string(),
            })?;

        // Get the appsrc element
        let appsrc = pipeline
            .by_name("src")
            .ok_or_else(|| DisplayError::Framebuffer {
                details: "Failed to get appsrc element".to_string(),
            })?
            .downcast::<AppSrc>()
            .map_err(|_| DisplayError::Framebuffer {
                details: "Failed to downcast to AppSrc".to_string(),
            })?;

        // Configure appsrc for low-latency live streaming
        appsrc.set_property("format", gstreamer::Format::Bytes); // Use bytes format for immediate processing
        appsrc.set_property("is-live", true);
        appsrc.set_property("max-bytes", 200000u64); // Limit internal queue size
        appsrc.set_property("block", false); // Don't block when queue is full
        appsrc.set_property("do-timestamp", false); // Don't apply timestamps for immediate rendering

        // Store pipeline and appsrc
        {
            let mut pipeline_lock = self.display_pipeline.write().await;
            *pipeline_lock = Some(pipeline);
        }
        {
            let mut appsrc_lock = self.appsrc.write().await;
            *appsrc_lock = Some(appsrc);
        }

        info!("GStreamer display pipeline initialized successfully");
        Ok(())
    }

    /// Initialize display pipeline when GStreamer feature is disabled
    #[cfg(not(all(feature = "display", target_os = "linux")))]
    async fn initialize_display_pipeline(&self) -> Result<()> {
        warn!("GStreamer display pipeline is only available on Linux with display feature");
        Ok(())
    }

    /// Initialize backlight device connection
    async fn initialize_devices(&self) -> Result<()> {
        // Initialize backlight
        match self.open_backlight().await {
            Ok(bl) => {
                let mut backlight = self.backlight.write().await;
                *backlight = Some(bl);
                info!("Backlight device opened: {}", self.config.backlight_device);
            }
            Err(e) => {
                warn!("Failed to open backlight device {}: {}", self.config.backlight_device, e);
                // Continue without backlight control - will be retried later
            }
        }

        Ok(())
    }

    /// Open backlight device for writing
    async fn open_backlight(&self) -> Result<File> {
        Ok(OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.config.backlight_device)
            .map_err(|e| DisplayError::BacklightOpen {
                device: self.config.backlight_device.clone(),
                source: e,
            })?)
    }

    /// Start the display controller with event handling
    pub async fn start(
        &self,
        event_bus: Arc<EventBus>,
    ) -> Result<()> {
        info!("Starting display controller");

        // Subscribe to relevant events
        let receiver = event_bus.subscribe();
        let filter = EventFilter::EventTypes(vec![
            "motion_detected",
            "touch_detected",
            "display_activate",
            "display_deactivate",
        ]);
        let mut event_receiver = EventReceiver::new(receiver, filter, "display_controller".to_string());

        // Clone references for the event handling task
        let controller = self.clone_for_task();
        let event_bus_clone = Arc::clone(&event_bus);

        // Start event handling task
        tokio::spawn(async move {
            loop {
                match event_receiver.recv().await {
                    Ok(event) => {
                        if let Err(e) = controller.handle_event(event, &event_bus_clone).await {
                            error!("Error handling display event: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Error receiving display events: {}", e);
                        sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });

        info!("Display controller started successfully");
        Ok(())
    }

    /// Handle incoming events
    async fn handle_event(&self, event: DoorcamEvent, event_bus: &Arc<EventBus>) -> Result<()> {
        match event {
            DoorcamEvent::MotionDetected { timestamp, .. } => {
                debug!("Motion detected - activating display");
                self.activate_display(timestamp, event_bus).await?;
            }
            DoorcamEvent::TouchDetected { timestamp } => {
                debug!("Touch detected - activating display");
                self.activate_display(timestamp, event_bus).await?;
            }
            DoorcamEvent::DisplayActivate { timestamp, duration_seconds } => {
                debug!("Display activation requested for {} seconds", duration_seconds);
                self.activate_display_with_duration(timestamp, duration_seconds, event_bus).await?;
            }
            DoorcamEvent::DisplayDeactivate { .. } => {
                debug!("Display deactivation requested");
                self.deactivate_display(event_bus).await?;
            }
            _ => {
                // Ignore other events
            }
        }
        Ok(())
    }

    /// Activate the display for the configured duration
    async fn activate_display(&self, _timestamp: SystemTime, event_bus: &Arc<EventBus>) -> Result<()> {
        self.activate_display_with_duration(
            SystemTime::now(),
            self.config.activation_period_seconds,
            event_bus
        ).await
    }

    /// Activate the display for a specific duration
    async fn activate_display_with_duration(
        &self,
        _timestamp: SystemTime,
        duration_seconds: u32,
        event_bus: &Arc<EventBus>,
    ) -> Result<()> {
        // Set display as active
        self.is_active.store(true, Ordering::Relaxed);

        // Turn on backlight
        self.set_backlight(true).await?;

        // Cancel any existing timer
        {
            let mut timer = self.activation_timer.write().await;
            if let Some(handle) = timer.take() {
                handle.abort();
            }
        }

        // Start new deactivation timer
        let is_active = Arc::clone(&self.is_active);
        let event_bus_clone = Arc::clone(event_bus);
        let duration = Duration::from_secs(duration_seconds as u64);

        let timer_handle = tokio::spawn(async move {
            sleep(duration).await;
            
            // Deactivate display
            is_active.store(false, Ordering::Relaxed);
            
            // Publish deactivation event
            let _ = event_bus_clone.publish(DoorcamEvent::DisplayDeactivate {
                timestamp: SystemTime::now(),
            }).await;
        });

        // Store the timer handle
        {
            let mut timer = self.activation_timer.write().await;
            *timer = Some(timer_handle);
        }

        info!("Display activated for {} seconds", duration_seconds);
        Ok(())
    }

    /// Deactivate the display immediately
    async fn deactivate_display(&self, _event_bus: &Arc<EventBus>) -> Result<()> {
        // Set display as inactive
        self.is_active.store(false, Ordering::Relaxed);

        // Turn off backlight
        self.set_backlight(false).await?;

        // Cancel any existing timer
        {
            let mut timer = self.activation_timer.write().await;
            if let Some(handle) = timer.take() {
                handle.abort();
            }
        }

        info!("Display deactivated");
        Ok(())
    }

    /// Control backlight on/off state
    async fn set_backlight(&self, enabled: bool) -> Result<()> {
        let mut backlight = self.backlight.write().await;
        
        if let Some(ref mut bl_file) = *backlight {
            // For bl_power: "0" = ON, "1" = OFF (inverted logic)
            let power_value = if enabled { "0" } else { "1" };
            
            // Seek to beginning and write power value
            bl_file.seek(SeekFrom::Start(0))
                .map_err(|e| DisplayError::Backlight { 
                    details: format!("Failed to seek backlight: {}", e) 
                })?;
            
            bl_file.write_all(power_value.as_bytes())
                .map_err(|e| DisplayError::Backlight { 
                    details: format!("Failed to write backlight: {}", e) 
                })?;
            
            bl_file.flush()
                .map_err(|e| DisplayError::Backlight { 
                    details: format!("Failed to flush backlight: {}", e) 
                })?;
            
            debug!("Backlight set to: {} (power value: {})", if enabled { "ON" } else { "OFF" }, power_value);
        } else {
            // Try to reinitialize backlight
            match self.open_backlight().await {
                Ok(bl) => {
                    *backlight = Some(bl);
                    debug!("Backlight device reconnected");
                    // Retry the operation once
                    drop(backlight);
                    let mut backlight_retry = self.backlight.write().await;
                    if let Some(ref mut bl_file) = *backlight_retry {
                        // For bl_power: "0" = ON, "1" = OFF (inverted logic)
                        let power_value = if enabled { "0" } else { "1" };
                        
                        bl_file.seek(SeekFrom::Start(0))
                            .map_err(|e| DisplayError::Backlight { 
                                details: format!("Failed to seek backlight: {}", e) 
                            })?;
                        
                        bl_file.write_all(power_value.as_bytes())
                            .map_err(|e| DisplayError::Backlight { 
                                details: format!("Failed to write backlight: {}", e) 
                            })?;
                        
                        bl_file.flush()
                            .map_err(|e| DisplayError::Backlight { 
                                details: format!("Failed to flush backlight: {}", e) 
                            })?;
                        
                        debug!("Backlight set to: {} (power value: {})", if enabled { "ON" } else { "OFF" }, power_value);
                    }
                }
                Err(e) => {
                    warn!("Backlight control unavailable: {}", e);
                }
            }
        }
        
        Ok(())
    }

    /// Render a frame to the display using GStreamer hardware acceleration
    pub async fn render_frame(&self, frame: &FrameData) -> Result<()> {
        if !self.is_active.load(Ordering::Relaxed) {
            // Display is not active, skip rendering
            return Ok(());
        }

        #[cfg(all(feature = "display", target_os = "linux"))]
        {
            self.render_frame_gstreamer(frame).await?;
        }

        #[cfg(not(all(feature = "display", target_os = "linux")))]
        {
            warn!("Display rendering not available without GStreamer on Linux");
        }

        Ok(())
    }

    /// Render frame using GStreamer pipeline
    #[cfg(all(feature = "display", target_os = "linux"))]
    async fn render_frame_gstreamer(&self, frame: &FrameData) -> Result<()> {
        let pipeline_lock = self.display_pipeline.read().await;
        let appsrc_lock = self.appsrc.read().await;

        if let (Some(pipeline), Some(appsrc)) = (pipeline_lock.as_ref(), appsrc_lock.as_ref()) {
            // Start pipeline if not already playing
            if pipeline.current_state() != gstreamer::State::Playing {
                pipeline.set_state(gstreamer::State::Playing)
                    .map_err(|e| DisplayError::Framebuffer {
                        details: format!("Failed to start display pipeline: {}", e),
                    })?;
                debug!("Display pipeline started");
            }

            // Push JPEG frame data to pipeline
            let jpeg_data = match frame.format {
                FrameFormat::Mjpeg => {
                    // Frame is already JPEG, use directly
                    frame.data.as_ref().clone()
                }
                _ => {
                    return Err(DisplayError::Framebuffer {
                        details: format!("Only MJPEG frames supported for GStreamer display, got {:?}", frame.format),
                    }.into());
                }
            };

            // Create GStreamer buffer from JPEG data
            let mut buffer = gstreamer::Buffer::with_size(jpeg_data.len())
                .map_err(|e| DisplayError::Framebuffer {
                    details: format!("Failed to create GStreamer buffer: {}", e),
                })?;

            {
                let buffer_ref = buffer.get_mut().unwrap();
                let mut map = buffer_ref.map_writable()
                    .map_err(|e| DisplayError::Framebuffer {
                        details: format!("Failed to map buffer: {}", e),
                    })?;
                map.copy_from_slice(&jpeg_data);
            }

            // Don't set timestamp for live display - this can cause buffering delays
            // For live display, we want immediate rendering without synchronization

            // Push buffer to appsrc
            appsrc.push_buffer(buffer)
                .map_err(|e| DisplayError::Framebuffer {
                    details: format!("Failed to push buffer to display pipeline: {:?}", e),
                })?;

            debug!("Frame {} rendered via GStreamer pipeline", frame.id);
            Ok(())
        } else {
            Err(DisplayError::Framebuffer {
                details: "Display pipeline not initialized".to_string(),
            }.into())
        }
    }

    /// Check if display is currently active
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Relaxed)
    }

    /// Get display configuration
    pub fn config(&self) -> &DisplayConfig {
        &self.config
    }

    /// Clone for use in async tasks
    fn clone_for_task(&self) -> Self {
        Self {
            config: self.config.clone(),
            backlight: Arc::clone(&self.backlight),
            is_active: Arc::clone(&self.is_active),
            activation_timer: Arc::clone(&self.activation_timer),
            #[cfg(all(feature = "display", target_os = "linux"))]
            display_pipeline: Arc::clone(&self.display_pipeline),
            #[cfg(all(feature = "display", target_os = "linux"))]
            appsrc: Arc::clone(&self.appsrc),
        }
    }
}

impl Clone for DisplayController {
    fn clone(&self) -> Self {
        self.clone_for_task()
    }
}



/// Display format conversion utilities
pub struct DisplayConverter;

impl DisplayConverter {
    /// Create placeholder RGB565 data for testing
    pub fn create_placeholder_rgb565(width: u32, height: u32) -> Result<Vec<u8>> {
        let pixel_count = (width * height) as usize;
        let mut data = Vec::with_capacity(pixel_count * 2);
        
        // Create a simple gradient pattern
        for y in 0..height {
            for x in 0..width {
                // Create RGB565 pixel (5 bits red, 6 bits green, 5 bits blue)
                let r = ((x * 31) / width) as u16;  // 5 bits
                let g = ((y * 63) / height) as u16; // 6 bits
                let b = (((x + y) * 31) / (width + height)) as u16; // 5 bits
                
                let rgb565 = (r << 11) | (g << 5) | b;
                
                // Write as little-endian bytes
                data.push((rgb565 & 0xFF) as u8);
                data.push((rgb565 >> 8) as u8);
            }
        }
        
        Ok(data)
    }

    /// Convert RGB24 to RGB565 format with optional scaling
    pub fn rgb24_to_rgb565(rgb24_data: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
        let expected_size = (width * height * 3) as usize;
        if rgb24_data.len() != expected_size {
            return Err(DisplayError::FormatConversion { 
                details: format!("Invalid RGB24 data size: expected {}, got {}", expected_size, rgb24_data.len()) 
            }.into());
        }

        let mut rgb565_data = Vec::with_capacity((width * height * 2) as usize);
        
        for chunk in rgb24_data.chunks_exact(3) {
            let r = chunk[0] >> 3;  // 8 bits -> 5 bits
            let g = chunk[1] >> 2;  // 8 bits -> 6 bits
            let b = chunk[2] >> 3;  // 8 bits -> 5 bits
            
            let rgb565 = ((r as u16) << 11) | ((g as u16) << 5) | (b as u16);
            
            // Write as little-endian
            rgb565_data.push((rgb565 & 0xFF) as u8);
            rgb565_data.push((rgb565 >> 8) as u8);
        }
        
        Ok(rgb565_data)
    }

    /// Scale RGB565 data to target resolution using simple nearest neighbor
    pub fn scale_rgb565(
        data: &[u8], 
        src_width: u32, 
        src_height: u32, 
        dst_width: u32, 
        dst_height: u32
    ) -> Result<Vec<u8>> {
        if data.len() != (src_width * src_height * 2) as usize {
            return Err(DisplayError::FormatConversion { 
                details: format!("Invalid RGB565 data size: expected {}, got {}", 
                               src_width * src_height * 2, data.len()) 
            }.into());
        }

        let mut scaled_data = Vec::with_capacity((dst_width * dst_height * 2) as usize);
        
        let x_ratio = src_width as f32 / dst_width as f32;
        let y_ratio = src_height as f32 / dst_height as f32;
        
        for dst_y in 0..dst_height {
            for dst_x in 0..dst_width {
                let src_x = ((dst_x as f32) * x_ratio) as u32;
                let src_y = ((dst_y as f32) * y_ratio) as u32;
                
                // Ensure we don't go out of bounds
                let src_x = src_x.min(src_width - 1);
                let src_y = src_y.min(src_height - 1);
                
                let src_index = ((src_y * src_width + src_x) * 2) as usize;
                
                // Copy the RGB565 pixel (2 bytes)
                scaled_data.push(data[src_index]);
                scaled_data.push(data[src_index + 1]);
            }
        }
        
        Ok(scaled_data)
    }

    /// Crop RGB565 data to fit within target dimensions (center crop)
    pub fn crop_rgb565(
        data: &[u8], 
        src_width: u32, 
        src_height: u32, 
        dst_width: u32, 
        dst_height: u32
    ) -> Result<Vec<u8>> {
        if data.len() != (src_width * src_height * 2) as usize {
            return Err(DisplayError::FormatConversion { 
                details: format!("Invalid RGB565 data size: expected {}, got {}", 
                               src_width * src_height * 2, data.len()) 
            }.into());
        }

        // Calculate crop offsets (center crop)
        let crop_width = dst_width.min(src_width);
        let crop_height = dst_height.min(src_height);
        let offset_x = (src_width - crop_width) / 2;
        let offset_y = (src_height - crop_height) / 2;
        
        let mut cropped_data = Vec::with_capacity((crop_width * crop_height * 2) as usize);
        
        for y in 0..crop_height {
            let src_y = offset_y + y;
            let src_row_start = (src_y * src_width + offset_x) as usize * 2;
            let src_row_end = src_row_start + (crop_width as usize * 2);
            
            cropped_data.extend_from_slice(&data[src_row_start..src_row_end]);
        }
        
        Ok(cropped_data)
    }

    /// Apply rotation to display data (placeholder for future implementation)
    pub fn apply_rotation(
        data: &[u8],
        _width: u32,
        _height: u32,
        rotation: crate::config::Rotation,
    ) -> Result<Vec<u8>> {
        // TODO: Implement actual rotation in later tasks
        debug!("Display rotation {:?} requested - placeholder implementation", rotation);
        Ok(data.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DisplayConfig;
    use std::time::SystemTime;
    // use tempfile::NamedTempFile;

    fn create_test_config() -> DisplayConfig {
        DisplayConfig {
            framebuffer_device: "/tmp/test_fb".to_string(),
            backlight_device: "/tmp/test_backlight".to_string(),
            touch_device: "/tmp/test_touch".to_string(),
            activation_period_seconds: 5,
            resolution: (800, 480),
            rotation: None,
            jpeg_decode_scale: 4,
        }
    }

    #[tokio::test]
    async fn test_display_controller_creation() {
        let config = create_test_config();
        
        // This will fail to open devices, but should not panic
        let result = DisplayController::new(config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_display_activation_state() {
        let config = create_test_config();
        let controller = DisplayController::new(config).await.unwrap();
        
        // Initially inactive
        assert!(!controller.is_active());
        
        // Activate
        controller.is_active.store(true, Ordering::Relaxed);
        assert!(controller.is_active());
        
        // Deactivate
        controller.is_active.store(false, Ordering::Relaxed);
        assert!(!controller.is_active());
    }

    #[tokio::test]
    async fn test_placeholder_display_data() {
        let config = create_test_config();
        let _controller = DisplayController::new(config).await.unwrap();
        
        // Test the DisplayConverter utility directly
        let data = DisplayConverter::create_placeholder_rgb565(320, 240).unwrap();
        
        // Should be 2 bytes per pixel for RGB565
        assert_eq!(data.len(), 320 * 240 * 2);
    }

    #[test]
    fn test_rgb24_to_rgb565_conversion() {
        // Test data: red, green, blue pixels
        let rgb24_data = vec![
            255, 0, 0,    // Red
            0, 255, 0,    // Green  
            0, 0, 255,    // Blue
        ];
        
        let rgb565_data = DisplayConverter::rgb24_to_rgb565(&rgb24_data, 3, 1).unwrap();
        
        // Should be 2 bytes per pixel
        assert_eq!(rgb565_data.len(), 6);
        
        // Verify red pixel (should be 0xF800 in RGB565)
        let red_pixel = ((rgb565_data[1] as u16) << 8) | (rgb565_data[0] as u16);
        assert_eq!(red_pixel & 0xF800, 0xF800); // Red bits should be set
    }

    #[test]
    fn test_rgb24_to_rgb565_invalid_size() {
        let invalid_data = vec![255, 0]; // Not divisible by 3
        let result = DisplayConverter::rgb24_to_rgb565(&invalid_data, 1, 1);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_frame_conversion() {
        let config = create_test_config();
        let _controller = DisplayController::new(config).await.unwrap();
        
        let _frame = FrameData::new(
            1,
            SystemTime::now(),
            vec![0u8; 100],
            320,
            240,
            FrameFormat::Mjpeg,
        );
        
        // Test RGB565 conversion directly
        let rgb24_data = vec![255u8; 320 * 240 * 3];
        let display_data = DisplayConverter::rgb24_to_rgb565(&rgb24_data, 320, 240).unwrap();
        
        // Should produce RGB565 data (2 bytes per pixel)
        assert_eq!(display_data.len(), 320 * 240 * 2);
    }

    #[test]
    fn test_rgb565_scaling() {
        // Create test RGB565 data (2x2 pixels)
        let src_data = vec![
            0x00, 0xF8, // Red pixel
            0xE0, 0x07, // Green pixel  
            0x1F, 0x00, // Blue pixel
            0xFF, 0xFF, // White pixel
        ];
        
        // Scale to 4x4
        let scaled_data = DisplayConverter::scale_rgb565(&src_data, 2, 2, 4, 4).unwrap();
        
        // Should be 4x4 pixels = 32 bytes
        assert_eq!(scaled_data.len(), 32);
    }

    #[test]
    fn test_rgb565_cropping() {
        // Create test RGB565 data (4x4 pixels)
        let src_data = vec![0u8; 4 * 4 * 2]; // 32 bytes
        
        // Crop to 2x2 (center crop)
        let cropped_data = DisplayConverter::crop_rgb565(&src_data, 4, 4, 2, 2).unwrap();
        
        // Should be 2x2 pixels = 8 bytes
        assert_eq!(cropped_data.len(), 8);
    }
}