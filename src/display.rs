use crate::config::DisplayConfig;
use crate::events::{DoorcamEvent, EventBus, EventReceiver, EventFilter};
use crate::frame::{FrameData, FrameFormat};
use crate::error::{DisplayError, DoorcamError, Result};
use std::fs::{File, OpenOptions};
use std::io::{Write, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::time::{interval, sleep};
use tracing::{debug, error, info, warn};

/// Display controller for HyperPixel 4.0 framebuffer interface
pub struct DisplayController {
    config: DisplayConfig,
    framebuffer: Arc<RwLock<Option<File>>>,
    backlight: Arc<RwLock<Option<File>>>,
    is_active: Arc<AtomicBool>,
    activation_timer: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
}

impl DisplayController {
    /// Create a new display controller
    pub async fn new(config: DisplayConfig) -> Result<Self> {
        info!("Initializing display controller for HyperPixel 4.0");
        debug!("Display config: {:?}", config);

        let controller = Self {
            config,
            framebuffer: Arc::new(RwLock::new(None)),
            backlight: Arc::new(RwLock::new(None)),
            is_active: Arc::new(AtomicBool::new(false)),
            activation_timer: Arc::new(RwLock::new(None)),
        };

        // Initialize framebuffer and backlight connections
        controller.initialize_devices().await?;

        Ok(controller)
    }

    /// Initialize framebuffer and backlight device connections
    async fn initialize_devices(&self) -> Result<()> {
        // Initialize framebuffer
        match self.open_framebuffer().await {
            Ok(fb) => {
                let mut framebuffer = self.framebuffer.write().await;
                *framebuffer = Some(fb);
                info!("Framebuffer device opened: {}", self.config.framebuffer_device);
            }
            Err(e) => {
                warn!("Failed to open framebuffer device {}: {}", self.config.framebuffer_device, e);
                // Continue without framebuffer - will be retried later
            }
        }

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

    /// Open framebuffer device for writing
    async fn open_framebuffer(&self) -> Result<File> {
        Ok(OpenOptions::new()
            .write(true)
            .open(&self.config.framebuffer_device)
            .map_err(|e| DisplayError::FramebufferOpen {
                device: self.config.framebuffer_device.clone(),
                source: e,
            })?)
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
        let event_bus_clone = Arc::clone(&event_bus);
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
            let brightness_value = if enabled { "255" } else { "0" };
            
            // Seek to beginning and write brightness value
            bl_file.seek(SeekFrom::Start(0))
                .map_err(|e| DisplayError::Backlight { 
                    details: format!("Failed to seek backlight: {}", e) 
                })?;
            
            bl_file.write_all(brightness_value.as_bytes())
                .map_err(|e| DisplayError::Backlight { 
                    details: format!("Failed to write backlight: {}", e) 
                })?;
            
            bl_file.flush()
                .map_err(|e| DisplayError::Backlight { 
                    details: format!("Failed to flush backlight: {}", e) 
                })?;
            
            debug!("Backlight set to: {}", if enabled { "ON" } else { "OFF" });
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
                        let brightness_value = if enabled { "255" } else { "0" };
                        
                        bl_file.seek(SeekFrom::Start(0))
                            .map_err(|e| DisplayError::Backlight { 
                                details: format!("Failed to seek backlight: {}", e) 
                            })?;
                        
                        bl_file.write_all(brightness_value.as_bytes())
                            .map_err(|e| DisplayError::Backlight { 
                                details: format!("Failed to write backlight: {}", e) 
                            })?;
                        
                        bl_file.flush()
                            .map_err(|e| DisplayError::Backlight { 
                                details: format!("Failed to flush backlight: {}", e) 
                            })?;
                        
                        debug!("Backlight set to: {}", if enabled { "ON" } else { "OFF" });
                    }
                }
                Err(e) => {
                    warn!("Backlight control unavailable: {}", e);
                }
            }
        }
        
        Ok(())
    }

    /// Render a frame to the display
    pub async fn render_frame(&self, frame: &FrameData) -> Result<()> {
        if !self.is_active.load(Ordering::Relaxed) {
            // Display is not active, skip rendering
            return Ok(());
        }

        let mut framebuffer = self.framebuffer.write().await;
        
        if let Some(ref mut fb_file) = *framebuffer {
            // Convert frame to display format
            let display_data = self.convert_frame_for_display(frame).await?;
            
            // Write to framebuffer
            fb_file.seek(SeekFrom::Start(0))
                .map_err(|e| DisplayError::Framebuffer { 
                    details: format!("Failed to seek framebuffer: {}", e) 
                })?;
            
            fb_file.write_all(&display_data)
                .map_err(|e| DisplayError::Framebuffer { 
                    details: format!("Failed to write framebuffer: {}", e) 
                })?;
            
            fb_file.flush()
                .map_err(|e| DisplayError::Framebuffer { 
                    details: format!("Failed to flush framebuffer: {}", e) 
                })?;
            
            debug!("Frame {} rendered to display", frame.id);
        } else {
            // Try to reinitialize framebuffer
            match self.open_framebuffer().await {
                Ok(fb) => {
                    *framebuffer = Some(fb);
                    debug!("Framebuffer device reconnected");
                    // Retry the operation once
                    drop(framebuffer);
                    let mut framebuffer_retry = self.framebuffer.write().await;
                    if let Some(ref mut fb_file) = *framebuffer_retry {
                        let display_data = self.convert_frame_for_display(frame).await?;
                        
                        fb_file.seek(SeekFrom::Start(0))
                            .map_err(|e| DisplayError::Framebuffer { 
                                details: format!("Failed to seek framebuffer: {}", e) 
                            })?;
                        
                        fb_file.write_all(&display_data)
                            .map_err(|e| DisplayError::Framebuffer { 
                                details: format!("Failed to write framebuffer: {}", e) 
                            })?;
                        
                        fb_file.flush()
                            .map_err(|e| DisplayError::Framebuffer { 
                                details: format!("Failed to flush framebuffer: {}", e) 
                            })?;
                        
                        debug!("Frame {} rendered to display", frame.id);
                    }
                }
                Err(e) => {
                    warn!("Framebuffer unavailable for rendering: {}", e);
                }
            }
        }
        
        Ok(())
    }

    /// Convert frame data to display format (RGB565 for HyperPixel 4.0)
    async fn convert_frame_for_display(&self, frame: &FrameData) -> Result<Vec<u8>> {
        // For now, implement a basic conversion placeholder
        // TODO: Implement proper format conversion and rotation in later tasks
        
        match frame.format {
            FrameFormat::Mjpeg => {
                // For MJPEG, we would need to decode first, then convert to RGB565
                // For now, return a placeholder pattern
                debug!("MJPEG frame conversion for display - placeholder implementation");
                self.create_placeholder_display_data(frame.width, frame.height).await
            }
            FrameFormat::Yuyv => {
                // Convert YUYV to RGB565
                debug!("YUYV frame conversion for display - placeholder implementation");
                self.create_placeholder_display_data(frame.width, frame.height).await
            }
            FrameFormat::Rgb24 => {
                // Convert RGB24 to RGB565
                debug!("RGB24 frame conversion for display - placeholder implementation");
                self.create_placeholder_display_data(frame.width, frame.height).await
            }
        }
    }

    /// Create placeholder display data for testing
    async fn create_placeholder_display_data(&self, width: u32, height: u32) -> Result<Vec<u8>> {
        // Create a simple pattern for testing - RGB565 format (2 bytes per pixel)
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
            framebuffer: Arc::clone(&self.framebuffer),
            backlight: Arc::clone(&self.backlight),
            is_active: Arc::clone(&self.is_active),
            activation_timer: Arc::clone(&self.activation_timer),
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
    /// Convert RGB24 to RGB565 format
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

    /// Apply rotation to display data (placeholder for future implementation)
    pub fn apply_rotation(
        data: &[u8],
        width: u32,
        height: u32,
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
            rotation: None,
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
        let controller = DisplayController::new(config).await.unwrap();
        
        let data = controller.create_placeholder_display_data(320, 240).await.unwrap();
        
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
        let controller = DisplayController::new(config).await.unwrap();
        
        let frame = FrameData::new(
            1,
            SystemTime::now(),
            vec![0u8; 100],
            320,
            240,
            FrameFormat::Mjpeg,
        );
        
        let display_data = controller.convert_frame_for_display(&frame).await.unwrap();
        
        // Should produce RGB565 data
        assert_eq!(display_data.len(), 320 * 240 * 2);
    }
}