use crate::config::DisplayConfig;
use crate::error::{DoorcamError, Result, TouchError};
use crate::events::{DoorcamEvent, EventBus};
use crate::recovery::{RecoveryAction, TouchRecovery};
use crate::touch::types::TouchErrorExt;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

use evdev::{Device, EventType, InputEventKind, Key};

/// Touch input handler for processing touch events from input devices using evdev
pub struct TouchInputHandler {
    pub(crate) device_path: String,
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
        info!(
            "Starting touch input handler for device: {}",
            self.device_path
        );

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

                        let _ = event_bus
                            .publish(DoorcamEvent::SystemError {
                                component: "touch_input".to_string(),
                                error: format!("Attempt {}/{}: {}", retry_count, max_retries, e),
                            })
                            .await;

                        if retry_count >= max_retries {
                            error!(
                                "Touch input handler failed after {} attempts, giving up",
                                max_retries
                            );
                            break;
                        }

                        let delay = retry_delay * 2_u32.pow(retry_count.min(5));
                        warn!(
                            "Retrying touch device connection in {:?} (attempt {}/{})",
                            delay, retry_count, max_retries
                        );
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

        recovery
            .recover_touch(|| async { self.test_touch_device().await })
            .await
    }

    /// Test touch device connectivity (used for recovery)
    async fn test_touch_device(&self) -> std::result::Result<(), TouchError> {
        let device = Device::open(&self.device_path).map_err(|e| TouchError::DeviceOpen {
            device: self.device_path.clone(),
            details: e.to_string(),
        })?;

        let supported_events = device.supported_events();
        if !supported_events.contains(EventType::KEY) {
            return Err(TouchError::Device(format!(
                "Device {} does not support key events",
                self.device_path
            )));
        }

        info!("Touch device {} test successful", self.device_path);
        Ok(())
    }

    /// Reset recovery state after successful operation
    pub async fn reset_recovery(&self) {
        let mut recovery = self.recovery.lock().await;
        recovery.reset();
    }

    /// Monitor touch device for input events using evdev
    async fn monitor_touch_device(device_path: &str, event_bus: &EventBus) -> Result<()> {
        let mut device = match Device::open(device_path) {
            Ok(device) => device,
            Err(e) => {
                let touch_error = match e.kind() {
                    std::io::ErrorKind::NotFound => {
                        TouchError::DeviceNotFound(device_path.to_string())
                    }
                    std::io::ErrorKind::PermissionDenied => {
                        TouchError::PermissionDenied(device_path.to_string())
                    }
                    _ => TouchError::Device(format!("Failed to open {}: {}", device_path, e)),
                };

                return Err(DoorcamError::component(
                    "touch_input".to_string(),
                    touch_error.user_message(),
                ));
            }
        };

        info!(
            "Touch device opened successfully: {} ({})",
            device_path,
            device.name().unwrap_or("Unknown")
        );
        debug!("Device capabilities: {:?}", device.supported_events());

        Self::validate_touch_device(&device, device_path)?;

        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: u32 = 5;

        loop {
            match device.fetch_events() {
                Ok(events) => {
                    consecutive_errors = 0;

                    for event in events {
                        if Self::is_touch_event(&event) {
                            debug!("Touch event detected: {:?}", event);

                            let _ = event_bus
                                .publish(DoorcamEvent::TouchDetected {
                                    timestamp: SystemTime::now(),
                                })
                                .await;
                        }
                    }
                }
                Err(e) => {
                    consecutive_errors += 1;

                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        return Err(DoorcamError::component(
                            "touch_input".to_string(),
                            format!(
                                "Too many consecutive errors reading from touch device: {}",
                                e
                            ),
                        ));
                    }

                    warn!(
                        "Error reading from touch device (attempt {}): {}",
                        consecutive_errors, e
                    );
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Validate that the device supports touch input
    pub(crate) fn validate_touch_device(device: &Device, device_path: &str) -> Result<()> {
        let supported_events = device.supported_events();

        if !supported_events.contains(EventType::KEY) {
            return Err(DoorcamError::component(
                "touch_input".to_string(),
                TouchError::UnsupportedDevice(format!(
                    "{} does not support key events",
                    device_path
                ))
                .user_message(),
            ));
        }

        if let Some(keys) = device.supported_keys() {
            let has_touch_keys = keys.contains(Key::BTN_TOUCH)
                || keys.contains(Key::BTN_LEFT)
                || keys.contains(Key::BTN_RIGHT);

            if !has_touch_keys {
                warn!("Device {} does not have standard touch keys, but will still monitor key events", device_path);
            } else {
                debug!("Device {} supports touch keys: {:?}", device_path, keys);
            }
        }

        if supported_events.contains(EventType::ABSOLUTE) {
            debug!("Device {} supports absolute positioning", device_path);
        }

        if supported_events.contains(EventType::RELATIVE) {
            debug!("Device {} supports relative positioning", device_path);
        }

        Ok(())
    }

    /// Check if the input event represents a touch event
    pub(crate) fn is_touch_event(event: &evdev::InputEvent) -> bool {
        match event.kind() {
            InputEventKind::Key(key) => match key {
                Key::BTN_TOUCH | Key::BTN_LEFT | Key::BTN_RIGHT | Key::BTN_MIDDLE => {
                    event.value() == 1
                }
                _ => false,
            },
            InputEventKind::AbsAxis(_) => false,
            _ => false,
        }
    }
}
