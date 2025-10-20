use crate::config::DisplayConfig;
use crate::events::{DoorcamEvent, EventBus};
use crate::error::{DoorcamError, Result, TouchError};
use crate::recovery::{TouchRecovery, RecoveryAction};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

#[cfg(feature = "display")]
use evdev::{Device, EventType, InputEventKind, Key};

/// Touch input handler for processing touch events from input devices using evdev
pub struct TouchInputHandler {
    device_path: String,
    event_bus: Arc<EventBus>,
    recovery: Arc<tokio::sync::Mutex<TouchRecovery>>,
    _retry_count: u32,
    max_retries: u32,
    retry_delay: Duration,
}

impl TouchInputHandler {
    /// Create a new touch input handler
    pub fn new(config: &DisplayConfig, event_bus: Arc<EventBus>) -> Self {
        Self {
            device_path: config.touch_device.clone(),
            event_bus,
            recovery: Arc::new(tokio::sync::Mutex::new(TouchRecovery::new())),
            _retry_count: 0,
            max_retries: 10,
            retry_delay: Duration::from_secs(5),
        }
    }

    /// Start monitoring touch input events
    pub async fn start(&self) -> Result<()> {
        info!("Starting touch input handler for device: {}", self.device_path);

        let device_path = self.device_path.clone();
        let event_bus = Arc::clone(&self.event_bus);
        let max_retries = self.max_retries;
        let retry_delay = self.retry_delay;

        tokio::spawn(async move {
            let mut retry_count = 0;
            
            loop {
                match Self::monitor_touch_device(&device_path, &event_bus).await {
                    Ok(_) => {
                        info!("Touch device monitoring ended normally");
                        break;
                    }
                    Err(e) => {
                        error!("Touch device error: {}", e);
                        retry_count += 1;
                        
                        // Publish system error event
                        let _ = event_bus.publish(DoorcamEvent::SystemError {
                            component: "touch_input".to_string(),
                            error: format!("Attempt {}/{}: {}", retry_count, max_retries, e),
                        }).await;
                        
                        if retry_count >= max_retries {
                            error!("Touch input handler failed after {} attempts, giving up", max_retries);
                            break;
                        }
                        
                        // Exponential backoff with jitter
                        let delay = retry_delay * 2_u32.pow(retry_count.min(5));
                        warn!("Retrying touch device connection in {:?} (attempt {}/{})", delay, retry_count, max_retries);
                        sleep(delay).await;
                    }
                }
            }
        });

        Ok(())
    }
    
    /// Handle touch error with recovery logic
    pub async fn handle_error_with_recovery(&self, error: TouchError) -> RecoveryAction {
        let mut recovery = self.recovery.lock().await;
        recovery.handle_touch_error(&error)
    }
    
    /// Attempt to recover from touch device failure
    pub async fn recover(&self) -> Result<()> {
        info!("Attempting touch device recovery");
        
        let mut recovery = self.recovery.lock().await;
        
        recovery.recover_touch(|| async {
            self.test_touch_device().await
        }).await
    }
    
    /// Test touch device connectivity (used for recovery)
    async fn test_touch_device(&self) -> std::result::Result<(), TouchError> {
        #[cfg(feature = "display")]
        {
            let device = Device::open(&self.device_path)
                .map_err(|e| TouchError::DeviceOpen {
                    device: self.device_path.clone(),
                    details: e.to_string(),
                })?;
            
            let supported_events = device.supported_events();
            if !supported_events.contains(EventType::KEY) {
                return Err(TouchError::Device(
                    format!("Device {} does not support key events", self.device_path)
                ));
            }
            
            info!("Touch device {} test successful", self.device_path);
            Ok(())
        }
        
        #[cfg(not(feature = "display"))]
        {
            warn!("Touch device testing not available on this platform");
            Err(TouchError::NotAvailable)
        }
    }
    
    /// Reset recovery state after successful operation
    pub async fn reset_recovery(&self) {
        let mut recovery = self.recovery.lock().await;
        recovery.reset();
    }

    /// Monitor touch device for input events using evdev
    #[cfg(feature = "display")]
    async fn monitor_touch_device(device_path: &str, event_bus: &EventBus) -> Result<()> {
        // Try to open the touch device using evdev
        let mut device = match Device::open(device_path) {
            Ok(device) => device,
            Err(e) => {
                let touch_error = match e.kind() {
                    std::io::ErrorKind::NotFound => TouchError::DeviceNotFound(device_path.to_string()),
                    std::io::ErrorKind::PermissionDenied => TouchError::PermissionDenied(device_path.to_string()),
                    _ => TouchError::Device(format!("Failed to open {}: {}", device_path, e)),
                };
                
                return Err(DoorcamError::component(
                    "touch_input".to_string(),
                    touch_error.user_message()
                ));
            }
        };
        
        info!("Touch device opened successfully: {} ({})", device_path, device.name().unwrap_or("Unknown"));
        debug!("Device capabilities: {:?}", device.supported_events());
        
        // Validate device capabilities
        Self::validate_touch_device(&device, device_path)?;

        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: u32 = 5;

        loop {
            // Fetch events from the device
            match device.fetch_events() {
                Ok(events) => {
                    consecutive_errors = 0; // Reset error count on successful read
                    
                    for event in events {
                        if Self::is_touch_event(&event) {
                            debug!("Touch event detected: {:?}", event);
                            
                            // Publish touch detected event
                            let _ = event_bus.publish(DoorcamEvent::TouchDetected {
                                timestamp: SystemTime::now(),
                            }).await;
                        }
                    }
                }
                Err(e) => {
                    consecutive_errors += 1;
                    
                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        return Err(DoorcamError::component(
                            "touch_input".to_string(),
                            format!("Too many consecutive errors reading from touch device: {}", e)
                        ));
                    }
                    
                    warn!("Error reading from touch device (attempt {}): {}", consecutive_errors, e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            }
            
            // Small delay to prevent busy waiting
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Validate that the device supports touch input
    #[cfg(feature = "display")]
    fn validate_touch_device(device: &Device, device_path: &str) -> Result<()> {
        let supported_events = device.supported_events();
        
        // Check if device supports key events (required for touch buttons)
        if !supported_events.contains(EventType::KEY) {
            return Err(DoorcamError::component(
                "touch_input".to_string(),
                TouchError::UnsupportedDevice(format!("{} does not support key events", device_path)).user_message()
            ));
        }
        
        // Check for specific touch-related keys
        if let Some(keys) = device.supported_keys() {
            let has_touch_keys = keys.contains(Key::BTN_TOUCH) || 
                                keys.contains(Key::BTN_LEFT) || 
                                keys.contains(Key::BTN_RIGHT);
            
            if !has_touch_keys {
                warn!("Device {} does not have standard touch keys, but will still monitor key events", device_path);
            } else {
                debug!("Device {} supports touch keys: {:?}", device_path, keys);
            }
        }
        
        // Log additional capabilities
        if supported_events.contains(EventType::ABSOLUTE) {
            debug!("Device {} supports absolute positioning", device_path);
        }
        
        if supported_events.contains(EventType::RELATIVE) {
            debug!("Device {} supports relative positioning", device_path);
        }
        
        Ok(())
    }

    /// Fallback implementation when evdev feature is not available
    #[cfg(not(feature = "display"))]
    async fn monitor_touch_device(device_path: &str, event_bus: &EventBus) -> Result<()> {
        warn!("evdev feature not enabled, using mock touch input for device: {}", device_path);
        
        // Mock implementation that generates periodic touch events for testing
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        
        loop {
            interval.tick().await;
            
            debug!("Mock touch event generated");
            let _ = event_bus.publish(DoorcamEvent::TouchDetected {
                timestamp: SystemTime::now(),
            }).await;
        }
    }

    /// Check if the input event represents a touch event
    #[cfg(feature = "display")]
    fn is_touch_event(event: &evdev::InputEvent) -> bool {
        match event.kind() {
            InputEventKind::Key(key) => {
                // Check for touch-related key events
                match key {
                    Key::BTN_TOUCH | Key::BTN_LEFT | Key::BTN_RIGHT | Key::BTN_MIDDLE => {
                        // Only trigger on key press (value 1), not release (value 0)
                        event.value() == 1
                    }
                    _ => false,
                }
            }
            InputEventKind::AbsAxis(_) => {
                // For absolute axis events (coordinates), we could track position
                // but for now we'll just detect any absolute movement as potential touch
                false // Don't trigger on coordinate changes, only on actual touch press
            }
            _ => false,
        }
    }

    /// Fallback implementation when evdev feature is not available
    #[cfg(not(feature = "display"))]
    #[allow(dead_code)]
    fn is_touch_event(_event: &()) -> bool {
        false
    }
}

/// Utility functions for touch device management
pub struct TouchDeviceUtils;

impl TouchDeviceUtils {
    /// Discover available touch input devices
    #[cfg(feature = "display")]
    pub fn discover_touch_devices() -> Vec<String> {
        let mut devices = Vec::new();
        
        // Check common input device paths
        for i in 0..10 {
            let device_path = format!("/dev/input/event{}", i);
            if let Ok(device) = Device::open(&device_path) {
                let supported_events = device.supported_events();
                
                // Check if device supports key events (potential touch device)
                if supported_events.contains(EventType::KEY) {
                    if let Some(keys) = device.supported_keys() {
                        let has_touch_keys = keys.contains(Key::BTN_TOUCH) || 
                                           keys.contains(Key::BTN_LEFT) || 
                                           keys.contains(Key::BTN_RIGHT);
                        
                        if has_touch_keys {
                            let device_name = device.name().unwrap_or("Unknown").to_string();
                            info!("Found potential touch device: {} ({})", device_path, device_name);
                            devices.push(device_path);
                        }
                    }
                }
            }
        }
        
        devices
    }
    
    /// Fallback when evdev feature is not available
    #[cfg(not(feature = "display"))]
    pub fn discover_touch_devices() -> Vec<String> {
        warn!("evdev feature not enabled, cannot discover touch devices");
        vec!["/dev/input/event0".to_string()] // Return default
    }
    
    /// Get device information for a given path
    #[cfg(feature = "display")]
    pub fn get_device_info(device_path: &str) -> Result<TouchDeviceInfo> {
        let device = Device::open(device_path)
            .map_err(|e| DoorcamError::component(
                "touch_device_utils".to_string(),
                format!("Failed to open device {}: {}", device_path, e)
            ))?;
        
        Ok(TouchDeviceInfo {
            path: device_path.to_string(),
            name: device.name().unwrap_or("Unknown").to_string(),
            vendor_id: device.input_id().vendor(),
            product_id: device.input_id().product(),
            supports_touch: device.supported_keys()
                .map(|keys| keys.contains(Key::BTN_TOUCH) || keys.contains(Key::BTN_LEFT))
                .unwrap_or(false),
            supports_absolute: device.supported_events().contains(EventType::ABSOLUTE),
            supports_relative: device.supported_events().contains(EventType::RELATIVE),
        })
    }
    
    /// Fallback when evdev feature is not available
    #[cfg(not(feature = "display"))]
    pub fn get_device_info(device_path: &str) -> Result<TouchDeviceInfo> {
        Ok(TouchDeviceInfo {
            path: device_path.to_string(),
            name: "Mock Device".to_string(),
            vendor_id: 0,
            product_id: 0,
            supports_touch: true,
            supports_absolute: false,
            supports_relative: false,
        })
    }
}

/// Information about a touch input device
#[derive(Debug, Clone)]
pub struct TouchDeviceInfo {
    pub path: String,
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub supports_touch: bool,
    pub supports_absolute: bool,
    pub supports_relative: bool,
}

impl TouchDeviceInfo {
    /// Check if this device is suitable for touch input
    pub fn is_suitable_for_touch(&self) -> bool {
        self.supports_touch
    }
    
    /// Get a human-readable description
    pub fn description(&self) -> String {
        format!("{} ({}) - Touch: {}, Absolute: {}, Relative: {}", 
                self.name, 
                self.path,
                self.supports_touch,
                self.supports_absolute,
                self.supports_relative)
    }
}

/// Mock touch input handler for testing without real hardware
pub struct MockTouchInputHandler {
    event_bus: Arc<EventBus>,
}

impl MockTouchInputHandler {
    /// Create a new mock touch input handler
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }

    /// Start the mock touch handler (generates periodic touch events for testing)
    pub async fn start(&self) -> Result<()> {
        info!("Starting mock touch input handler");

        let event_bus = Arc::clone(&self.event_bus);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            
            loop {
                interval.tick().await;
                
                debug!("Mock touch event generated");
                let _ = event_bus.publish(DoorcamEvent::TouchDetected {
                    timestamp: SystemTime::now(),
                }).await;
            }
        });

        Ok(())
    }

    /// Trigger a mock touch event immediately
    pub async fn trigger_touch(&self) -> Result<()> {
        debug!("Mock touch event triggered manually");
        self.event_bus.publish(DoorcamEvent::TouchDetected {
            timestamp: SystemTime::now(),
        }).await.map_err(|e| DoorcamError::component("mock_touch".to_string(), e.to_string()))?;
        
        Ok(())
    }
}



impl TouchError {
    /// Check if this error is recoverable (should retry)
    pub fn is_recoverable(&self) -> bool {
        match self {
            TouchError::Device(_) => true,
            TouchError::DeviceOpen { .. } => true,
            TouchError::DeviceRead { .. } => true,
            TouchError::EventParsing { .. } => false, // Don't retry parse errors
            TouchError::NotAvailable => false, // Don't retry if not available
            TouchError::DeviceNotFound(_) => true,
            TouchError::PermissionDenied(_) => false,
            TouchError::UnsupportedDevice(_) => false,
        }
    }
    
    /// Get a user-friendly error message
    pub fn user_message(&self) -> String {
        match self {
            TouchError::DeviceOpen { device, .. } => format!("Touch device not found at {}", device),
            TouchError::DeviceRead { details } => format!("Touch device read error: {}", details),
            TouchError::EventParsing { details } => format!("Touch event parsing failed: {}", details),
            TouchError::NotAvailable => "Touch input not available on this system".to_string(),
            TouchError::Device(msg) => format!("Touch device error: {}", msg),
            TouchError::DeviceNotFound(device) => format!("Touch device not found: {}", device),
            TouchError::PermissionDenied(device) => format!("Permission denied for touch device: {}", device),
            TouchError::UnsupportedDevice(device) => format!("Unsupported touch device: {}", device),
        }
    }
}

/// Touch event types for more detailed event handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchEventType {
    /// Touch press (finger down)
    Press,
    /// Touch release (finger up)
    Release,
    /// Touch move (finger dragging)
    Move,
}

/// Detailed touch event information
#[derive(Debug, Clone)]
pub struct TouchEvent {
    pub event_type: TouchEventType,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub pressure: Option<i32>,
    pub timestamp: SystemTime,
}

impl TouchEvent {
    /// Create a simple touch press event
    pub fn press(timestamp: SystemTime) -> Self {
        Self {
            event_type: TouchEventType::Press,
            x: None,
            y: None,
            pressure: None,
            timestamp,
        }
    }

    /// Create a touch event with coordinates
    pub fn press_at(x: i32, y: i32, timestamp: SystemTime) -> Self {
        Self {
            event_type: TouchEventType::Press,
            x: Some(x),
            y: Some(y),
            pressure: None,
            timestamp,
        }
    }
}

/// Advanced touch input handler with detailed event parsing and debouncing
pub struct AdvancedTouchInputHandler {
    device_path: String,
    event_bus: Arc<EventBus>,
    last_touch_time: std::sync::Arc<std::sync::RwLock<Option<SystemTime>>>,
    debounce_duration: Duration,
    current_position: std::sync::Arc<std::sync::RwLock<(Option<i32>, Option<i32>)>>,
}

impl AdvancedTouchInputHandler {
    /// Create a new advanced touch input handler with debouncing
    pub fn new(config: &DisplayConfig, event_bus: Arc<EventBus>) -> Self {
        Self {
            device_path: config.touch_device.clone(),
            event_bus,
            last_touch_time: std::sync::Arc::new(std::sync::RwLock::new(None)),
            debounce_duration: Duration::from_millis(200), // 200ms debounce
            current_position: std::sync::Arc::new(std::sync::RwLock::new((None, None))),
        }
    }

    /// Set the debounce duration for touch events
    pub fn set_debounce_duration(&mut self, duration: Duration) {
        self.debounce_duration = duration;
    }

    /// Start monitoring with advanced touch event processing
    pub async fn start(&self) -> Result<()> {
        info!("Starting advanced touch input handler for device: {}", self.device_path);

        let device_path = self.device_path.clone();
        let event_bus = Arc::clone(&self.event_bus);
        let last_touch_time = std::sync::Arc::clone(&self.last_touch_time);
        let current_position = std::sync::Arc::clone(&self.current_position);
        let debounce_duration = self.debounce_duration;

        tokio::spawn(async move {
            let mut retry_count = 0;
            let max_retries = 10;
            
            loop {
                match Self::monitor_advanced_touch_device(
                    &device_path, 
                    &event_bus, 
                    &last_touch_time,
                    &current_position,
                    debounce_duration
                ).await {
                    Ok(_) => {
                        info!("Advanced touch device monitoring ended normally");
                        break;
                    }
                    Err(e) => {
                        error!("Advanced touch device error: {}", e);
                        retry_count += 1;
                        
                        // Publish system error event
                        let _ = event_bus.publish(DoorcamEvent::SystemError {
                            component: "advanced_touch_input".to_string(),
                            error: format!("Attempt {}/{}: {}", retry_count, max_retries, e),
                        }).await;
                        
                        if retry_count >= max_retries {
                            error!("Advanced touch input handler failed after {} attempts, giving up", max_retries);
                            break;
                        }
                        
                        // Exponential backoff
                        let delay = Duration::from_secs(5) * 2_u32.pow(retry_count.min(5));
                        warn!("Retrying advanced touch device connection in {:?} (attempt {}/{})", delay, retry_count, max_retries);
                        sleep(delay).await;
                    }
                }
            }
        });

        Ok(())
    }

    /// Monitor touch device with debouncing and advanced parsing using evdev
    #[cfg(feature = "display")]
    async fn monitor_advanced_touch_device(
        device_path: &str,
        event_bus: &EventBus,
        last_touch_time: &std::sync::Arc<std::sync::RwLock<Option<SystemTime>>>,
        current_position: &std::sync::Arc<std::sync::RwLock<(Option<i32>, Option<i32>)>>,
        debounce_duration: Duration,
    ) -> Result<()> {
        let mut device = match Device::open(device_path) {
            Ok(device) => device,
            Err(e) => {
                let touch_error = match e.kind() {
                    std::io::ErrorKind::NotFound => TouchError::DeviceNotFound(device_path.to_string()),
                    std::io::ErrorKind::PermissionDenied => TouchError::PermissionDenied(device_path.to_string()),
                    _ => TouchError::Device(format!("Failed to open {}: {}", device_path, e)),
                };
                
                return Err(DoorcamError::component(
                    "advanced_touch_input".to_string(),
                    touch_error.user_message()
                ));
            }
        };
        
        info!("Advanced touch device opened successfully: {} ({})", device_path, device.name().unwrap_or("Unknown"));
        debug!("Device capabilities: {:?}", device.supported_events());
        
        // Validate device capabilities
        TouchInputHandler::validate_touch_device(&device, device_path)?;

        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: u32 = 5;

        loop {
            match device.fetch_events() {
                Ok(events) => {
                    consecutive_errors = 0; // Reset error count on successful read
                    
                    for event in events {
                        if let Some(touch_event) = Self::parse_advanced_touch_event(&event, current_position) {
                            // Apply debouncing
                            let now = SystemTime::now();
                            let should_process = {
                                let last_time = last_touch_time.read().unwrap();
                                match *last_time {
                                    Some(last) => now.duration_since(last).unwrap_or_default() >= debounce_duration,
                                    None => true,
                                }
                            };

                            if should_process {
                                debug!("Advanced touch event: {:?}", touch_event);
                                
                                // Update last touch time
                                {
                                    let mut last_time = last_touch_time.write().unwrap();
                                    *last_time = Some(now);
                                }
                                
                                // Publish touch detected event
                                let _ = event_bus.publish(DoorcamEvent::TouchDetected {
                                    timestamp: touch_event.timestamp,
                                }).await;
                            } else {
                                debug!("Touch event debounced");
                            }
                        }
                    }
                }
                Err(e) => {
                    consecutive_errors += 1;
                    
                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        return Err(DoorcamError::component(
                            "advanced_touch_input".to_string(),
                            format!("Too many consecutive errors reading from touch device: {}", e)
                        ));
                    }
                    
                    warn!("Error reading from advanced touch device (attempt {}): {}", consecutive_errors, e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            }
            
            // Small delay to prevent busy waiting
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Fallback implementation when evdev feature is not available
    #[cfg(not(feature = "display"))]
    async fn monitor_advanced_touch_device(
        device_path: &str,
        event_bus: &EventBus,
        last_touch_time: &std::sync::Arc<std::sync::RwLock<Option<SystemTime>>>,
        _current_position: &std::sync::Arc<std::sync::RwLock<(Option<i32>, Option<i32>)>>,
        debounce_duration: Duration,
    ) -> Result<()> {
        warn!("evdev feature not enabled, using mock advanced touch input for device: {}", device_path);
        
        // Mock implementation with debouncing
        let mut interval = tokio::time::interval(Duration::from_secs(25));
        
        loop {
            interval.tick().await;
            
            let now = SystemTime::now();
            let should_process = {
                let last_time = last_touch_time.read().unwrap();
                match *last_time {
                    Some(last) => now.duration_since(last).unwrap_or_default() >= debounce_duration,
                    None => true,
                }
            };

            if should_process {
                debug!("Mock advanced touch event generated");
                
                // Update last touch time
                {
                    let mut last_time = last_touch_time.write().unwrap();
                    *last_time = Some(now);
                }
                
                let _ = event_bus.publish(DoorcamEvent::TouchDetected {
                    timestamp: now,
                }).await;
            }
        }
    }

    /// Parse evdev input event into TouchEvent with coordinate tracking
    #[cfg(feature = "display")]
    fn parse_advanced_touch_event(
        event: &evdev::InputEvent, 
        current_position: &std::sync::Arc<std::sync::RwLock<(Option<i32>, Option<i32>)>>
    ) -> Option<TouchEvent> {
        match event.kind() {
            InputEventKind::Key(key) => {
                match key {
                    Key::BTN_TOUCH | Key::BTN_LEFT | Key::BTN_RIGHT | Key::BTN_MIDDLE => {
                        let touch_type = if event.value() == 1 {
                            TouchEventType::Press
                        } else if event.value() == 0 {
                            TouchEventType::Release
                        } else {
                            return None;
                        };
                        
                        // Get current position
                        let (x, y) = {
                            let pos = current_position.read().unwrap();
                            *pos
                        };
                        
                        Some(TouchEvent {
                            event_type: touch_type,
                            x,
                            y,
                            pressure: None,
                            timestamp: SystemTime::now(),
                        })
                    }
                    _ => None,
                }
            }
            InputEventKind::AbsAxis(axis) => {
                // Update position tracking but don't generate touch event
                match axis {
                    evdev::AbsoluteAxisType::ABS_X => {
                        let mut pos = current_position.write().unwrap();
                        pos.0 = Some(event.value());
                    }
                    evdev::AbsoluteAxisType::ABS_Y => {
                        let mut pos = current_position.write().unwrap();
                        pos.1 = Some(event.value());
                    }
                    _ => {}
                }
                None // Don't generate touch event for coordinate updates
            }
            _ => None,
        }
    }

    /// Fallback implementation when evdev feature is not available
    #[cfg(not(feature = "display"))]
    #[allow(dead_code)]
    fn parse_advanced_touch_event(
        _event: &(), 
        _current_position: &std::sync::Arc<std::sync::RwLock<(Option<i32>, Option<i32>)>>
    ) -> Option<TouchEvent> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DisplayConfig;
    use std::time::SystemTime;

    fn create_test_config() -> DisplayConfig {
        DisplayConfig {
            framebuffer_device: "/dev/fb0".to_string(),
            backlight_device: "/sys/class/backlight/rpi_backlight/brightness".to_string(),
            touch_device: "/dev/input/event0".to_string(),
            activation_period_seconds: 30,
            rotation: None,
        }
    }

    #[tokio::test]
    async fn test_mock_touch_handler() {
        let event_bus = Arc::new(EventBus::new(10));
        let mut receiver = event_bus.subscribe();
        
        let mock_handler = MockTouchInputHandler::new(Arc::clone(&event_bus));
        
        // Trigger a mock touch event
        mock_handler.trigger_touch().await.unwrap();
        
        // Should receive the touch event
        let event = receiver.recv().await.unwrap();
        match event {
            DoorcamEvent::TouchDetected { .. } => {
                // Success
            }
            _ => panic!("Expected TouchDetected event"),
        }
    }

    #[test]
    fn test_touch_event_creation() {
        let timestamp = SystemTime::now();
        let touch_event = TouchEvent::press(timestamp);
        
        assert_eq!(touch_event.event_type, TouchEventType::Press);
        assert_eq!(touch_event.x, None);
        assert_eq!(touch_event.y, None);
        assert_eq!(touch_event.timestamp, timestamp);
    }

    #[test]
    fn test_touch_event_with_coordinates() {
        let timestamp = SystemTime::now();
        let touch_event = TouchEvent::press_at(100, 200, timestamp);
        
        assert_eq!(touch_event.event_type, TouchEventType::Press);
        assert_eq!(touch_event.x, Some(100));
        assert_eq!(touch_event.y, Some(200));
        assert_eq!(touch_event.timestamp, timestamp);
    }

    #[cfg(feature = "display")]
    #[test]
    fn test_is_touch_event() {
        use evdev::{InputEvent, EventType, Key};

        
        // Create a mock touch press event
        let press_event = InputEvent::new(
            EventType::KEY,
            Key::BTN_TOUCH.code(),
            1, // Press
        );
        
        assert!(TouchInputHandler::is_touch_event(&press_event));
        
        // Create a mock touch release event
        let release_event = InputEvent::new(
            EventType::KEY,
            Key::BTN_TOUCH.code(),
            0, // Release
        );
        
        assert!(!TouchInputHandler::is_touch_event(&release_event));
        
        // Create a non-touch key event
        let other_key_event = InputEvent::new(
            EventType::KEY,
            Key::KEY_A.code(),
            1, // Press
        );
        
        assert!(!TouchInputHandler::is_touch_event(&other_key_event));
    }

    #[cfg(not(feature = "display"))]
    #[test]
    fn test_is_touch_event_fallback() {
        // When evdev feature is not available, the function should return false
        assert!(!TouchInputHandler::is_touch_event(&()));
    }

    #[tokio::test]
    async fn test_touch_handler_creation() {
        let config = create_test_config();
        let event_bus = Arc::new(EventBus::new(10));
        
        let handler = TouchInputHandler::new(&config, Arc::clone(&event_bus));
        assert_eq!(handler.device_path, "/dev/input/event0");
    }

    #[tokio::test]
    async fn test_advanced_touch_handler_creation() {
        let config = create_test_config();
        let event_bus = Arc::new(EventBus::new(10));
        
        let mut handler = AdvancedTouchInputHandler::new(&config, Arc::clone(&event_bus));
        handler.set_debounce_duration(Duration::from_millis(100));
        
        assert_eq!(handler.device_path, "/dev/input/event0");
        assert_eq!(handler.debounce_duration, Duration::from_millis(100));
    }

    #[cfg(feature = "display")]
    #[test]
    fn test_parse_advanced_touch_event() {
        use evdev::{InputEvent, EventType, Key, AbsoluteAxisType};
        use std::sync::{Arc, RwLock};
        
        let current_position = Arc::new(RwLock::new((None, None)));
        
        // Test touch press event
        let press_event = InputEvent::new(
            EventType::KEY,
            Key::BTN_TOUCH.code(),
            1, // Press
        );
        
        let touch_event = AdvancedTouchInputHandler::parse_advanced_touch_event(&press_event, &current_position);
        assert!(touch_event.is_some());
        
        let event = touch_event.unwrap();
        assert_eq!(event.event_type, TouchEventType::Press);
        
        // Test coordinate update (should not generate touch event)
        let x_event = InputEvent::new(
            EventType::ABSOLUTE,
            AbsoluteAxisType::ABS_X.0,
            100,
        );
        
        let coord_event = AdvancedTouchInputHandler::parse_advanced_touch_event(&x_event, &current_position);
        assert!(coord_event.is_none());
        
        // Check that position was updated
        let pos = current_position.read().unwrap();
        assert_eq!(pos.0, Some(100));
    }

    #[cfg(not(feature = "display"))]
    #[test]
    fn test_parse_advanced_touch_event_fallback() {
        use std::sync::{Arc, RwLock};
        
        let current_position = Arc::new(RwLock::new((None, None)));
        
        // When evdev feature is not available, the function should return None
        let result = AdvancedTouchInputHandler::parse_advanced_touch_event(&(), &current_position);
        assert!(result.is_none());
    }
}