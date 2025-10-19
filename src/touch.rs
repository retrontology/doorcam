use crate::config::DisplayConfig;
use crate::events::{DoorcamEvent, EventBus};
use crate::error::{DoorcamError, Result};
use std::fs::File;
use std::io::{Read, BufReader};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

/// Touch input handler for processing touch events from input devices
pub struct TouchInputHandler {
    device_path: String,
    event_bus: Arc<EventBus>,
}

impl TouchInputHandler {
    /// Create a new touch input handler
    pub fn new(config: &DisplayConfig, event_bus: Arc<EventBus>) -> Self {
        Self {
            device_path: config.touch_device.clone(),
            event_bus,
        }
    }

    /// Start monitoring touch input events
    pub async fn start(&self) -> Result<()> {
        info!("Starting touch input handler for device: {}", self.device_path);

        let device_path = self.device_path.clone();
        let event_bus = Arc::clone(&self.event_bus);

        tokio::spawn(async move {
            loop {
                match Self::monitor_touch_device(&device_path, &event_bus).await {
                    Ok(_) => {
                        info!("Touch device monitoring ended normally");
                        break;
                    }
                    Err(e) => {
                        error!("Touch device error: {}", e);
                        
                        // Publish system error event
                        let _ = event_bus.publish(DoorcamEvent::SystemError {
                            component: "touch_input".to_string(),
                            error: e.to_string(),
                        }).await;
                        
                        // Wait before retrying
                        sleep(Duration::from_secs(5)).await;
                        info!("Retrying touch device connection...");
                    }
                }
            }
        });

        Ok(())
    }

    /// Monitor touch device for input events
    async fn monitor_touch_device(device_path: &str, event_bus: &EventBus) -> Result<()> {
        // Try to open the touch device
        let file = match File::open(device_path) {
            Ok(f) => f,
            Err(e) => {
                return Err(DoorcamError::component(
                    "touch_input".to_string(),
                    format!("Failed to open touch device {}: {}", device_path, e)
                ));
            }
        };

        let mut reader = BufReader::new(file);
        let mut buffer = [0u8; 24]; // Standard input_event structure size
        
        info!("Touch device opened successfully: {}", device_path);

        loop {
            // Read input event structure
            match reader.read_exact(&mut buffer) {
                Ok(_) => {
                    // Parse the input event (simplified parsing)
                    if Self::is_touch_event(&buffer) {
                        debug!("Touch event detected");
                        
                        // Publish touch detected event
                        let _ = event_bus.publish(DoorcamEvent::TouchDetected {
                            timestamp: SystemTime::now(),
                        }).await;
                    }
                }
                Err(e) => {
                    return Err(DoorcamError::component(
                        "touch_input".to_string(),
                        format!("Failed to read from touch device: {}", e)
                    ));
                }
            }
        }
    }

    /// Check if the input event represents a touch event
    fn is_touch_event(buffer: &[u8; 24]) -> bool {
        // This is a simplified implementation
        // In a real implementation, we would parse the input_event structure:
        // struct input_event {
        //     struct timeval time;  // 16 bytes on 64-bit
        //     __u16 type;           // 2 bytes
        //     __u16 code;           // 2 bytes  
        //     __s32 value;          // 4 bytes
        // };
        
        // For now, we'll consider any non-zero event as a potential touch
        // In practice, we'd check for EV_KEY events with BTN_TOUCH or similar
        
        // Extract type field (bytes 16-17, little-endian)
        let event_type = u16::from_le_bytes([buffer[16], buffer[17]]);
        
        // Extract code field (bytes 18-19, little-endian)
        let event_code = u16::from_le_bytes([buffer[18], buffer[19]]);
        
        // Extract value field (bytes 20-23, little-endian)
        let event_value = i32::from_le_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]);
        
        // EV_KEY = 1, BTN_TOUCH = 0x14a (or similar touch codes)
        // For simplicity, we'll detect any key press event as a touch
        event_type == 1 && event_value == 1 // Key press events
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

/// Touch input error types
#[derive(Debug, thiserror::Error)]
pub enum TouchError {
    #[error("Device error: {0}")]
    Device(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Parse error: {0}")]
    Parse(String),
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

/// Advanced touch input handler with detailed event parsing
pub struct AdvancedTouchInputHandler {
    device_path: String,
    event_bus: Arc<EventBus>,
    last_touch_time: std::sync::Arc<std::sync::RwLock<Option<SystemTime>>>,
    debounce_duration: Duration,
}

impl AdvancedTouchInputHandler {
    /// Create a new advanced touch input handler with debouncing
    pub fn new(config: &DisplayConfig, event_bus: Arc<EventBus>) -> Self {
        Self {
            device_path: config.touch_device.clone(),
            event_bus,
            last_touch_time: std::sync::Arc::new(std::sync::RwLock::new(None)),
            debounce_duration: Duration::from_millis(200), // 200ms debounce
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
        let debounce_duration = self.debounce_duration;

        tokio::spawn(async move {
            loop {
                match Self::monitor_advanced_touch_device(
                    &device_path, 
                    &event_bus, 
                    &last_touch_time,
                    debounce_duration
                ).await {
                    Ok(_) => {
                        info!("Advanced touch device monitoring ended normally");
                        break;
                    }
                    Err(e) => {
                        error!("Advanced touch device error: {}", e);
                        
                        // Publish system error event
                        let _ = event_bus.publish(DoorcamEvent::SystemError {
                            component: "advanced_touch_input".to_string(),
                            error: e.to_string(),
                        }).await;
                        
                        // Wait before retrying
                        sleep(Duration::from_secs(5)).await;
                        info!("Retrying advanced touch device connection...");
                    }
                }
            }
        });

        Ok(())
    }

    /// Monitor touch device with debouncing and advanced parsing
    async fn monitor_advanced_touch_device(
        device_path: &str,
        event_bus: &EventBus,
        last_touch_time: &std::sync::Arc<std::sync::RwLock<Option<SystemTime>>>,
        debounce_duration: Duration,
    ) -> Result<()> {
        let file = File::open(device_path)
            .map_err(|e| DoorcamError::component(
                "advanced_touch_input".to_string(),
                format!("Failed to open touch device {}: {}", device_path, e)
            ))?;

        let mut reader = BufReader::new(file);
        let mut buffer = [0u8; 24];
        
        info!("Advanced touch device opened successfully: {}", device_path);

        loop {
            match reader.read_exact(&mut buffer) {
                Ok(_) => {
                    if let Some(touch_event) = Self::parse_touch_event(&buffer) {
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
                Err(e) => {
                    return Err(DoorcamError::component(
                        "advanced_touch_input".to_string(),
                        format!("Failed to read from touch device: {}", e)
                    ));
                }
            }
        }
    }

    /// Parse input event buffer into TouchEvent
    fn parse_touch_event(buffer: &[u8; 24]) -> Option<TouchEvent> {
        // Parse input_event structure
        let event_type = u16::from_le_bytes([buffer[16], buffer[17]]);
        let event_code = u16::from_le_bytes([buffer[18], buffer[19]]);
        let event_value = i32::from_le_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]);
        
        // Check for touch-related events
        match event_type {
            1 => { // EV_KEY
                if event_code == 0x14a || event_code == 0x110 { // BTN_TOUCH or BTN_LEFT
                    let touch_type = if event_value == 1 {
                        TouchEventType::Press
                    } else if event_value == 0 {
                        TouchEventType::Release
                    } else {
                        return None;
                    };
                    
                    Some(TouchEvent {
                        event_type: touch_type,
                        x: None,
                        y: None,
                        pressure: None,
                        timestamp: SystemTime::now(),
                    })
                } else {
                    None
                }
            }
            3 => { // EV_ABS (absolute coordinates)
                // For coordinate events, we'd need to track state
                // This is a simplified implementation
                None
            }
            _ => None,
        }
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

    #[test]
    fn test_is_touch_event() {
        // Create a mock input event buffer for a key press
        let mut buffer = [0u8; 24];
        
        // Set event type to EV_KEY (1)
        buffer[16] = 1;
        buffer[17] = 0;
        
        // Set event code to some key
        buffer[18] = 0x4a;
        buffer[19] = 0x01;
        
        // Set event value to 1 (press)
        buffer[20] = 1;
        buffer[21] = 0;
        buffer[22] = 0;
        buffer[23] = 0;
        
        assert!(TouchInputHandler::is_touch_event(&buffer));
        
        // Test release event (value = 0)
        buffer[20] = 0;
        assert!(!TouchInputHandler::is_touch_event(&buffer));
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

    #[test]
    fn test_parse_touch_event() {
        // Create a mock input event buffer for BTN_TOUCH press
        let mut buffer = [0u8; 24];
        
        // Set event type to EV_KEY (1)
        buffer[16] = 1;
        buffer[17] = 0;
        
        // Set event code to BTN_TOUCH (0x14a)
        buffer[18] = 0x4a;
        buffer[19] = 0x01;
        
        // Set event value to 1 (press)
        buffer[20] = 1;
        buffer[21] = 0;
        buffer[22] = 0;
        buffer[23] = 0;
        
        let touch_event = AdvancedTouchInputHandler::parse_touch_event(&buffer);
        assert!(touch_event.is_some());
        
        let event = touch_event.unwrap();
        assert_eq!(event.event_type, TouchEventType::Press);
    }
}