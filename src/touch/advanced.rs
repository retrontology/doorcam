use crate::config::DisplayConfig;
use crate::error::{DoorcamError, Result, TouchError};
use crate::events::{DoorcamEvent, EventBus};
use crate::touch::types::TouchErrorExt;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::time::Duration;
use tracing::{debug, error, info, warn};

use evdev::{Device, InputEventKind, Key};

use super::handler::TouchInputHandler;

/// Touch event types for more detailed event handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchEventType {
    Press,
    Release,
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
    pub fn press(timestamp: SystemTime) -> Self {
        Self {
            event_type: TouchEventType::Press,
            x: None,
            y: None,
            pressure: None,
            timestamp,
        }
    }

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
    pub(crate) device_path: String,
    event_bus: Arc<EventBus>,
    last_touch_time: std::sync::Arc<std::sync::RwLock<Option<SystemTime>>>,
    pub(crate) debounce_duration: Duration,
    current_position: std::sync::Arc<std::sync::RwLock<(Option<i32>, Option<i32>)>>,
}

impl AdvancedTouchInputHandler {
    pub fn new(config: &DisplayConfig, event_bus: Arc<EventBus>) -> Self {
        Self {
            device_path: config.touch_device.clone(),
            event_bus,
            last_touch_time: std::sync::Arc::new(std::sync::RwLock::new(None)),
            debounce_duration: Duration::from_millis(200),
            current_position: std::sync::Arc::new(std::sync::RwLock::new((None, None))),
        }
    }

    pub fn set_debounce_duration(&mut self, duration: Duration) {
        self.debounce_duration = duration;
    }

    pub async fn start(&self) -> Result<()> {
        info!(
            "Starting advanced touch input handler for device: {}",
            self.device_path
        );

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
                    debounce_duration,
                )
                .await
                {
                    Ok(_) => {
                        info!("Advanced touch device monitoring ended normally");
                        break;
                    }
                    Err(e) => {
                        error!("Advanced touch device error: {}", e);
                        retry_count += 1;

                        let _ = event_bus
                            .publish(DoorcamEvent::SystemError {
                                component: "advanced_touch_input".to_string(),
                                error: format!("Attempt {}/{}: {}", retry_count, max_retries, e),
                            })
                            .await;

                        if retry_count >= max_retries {
                            error!(
                                "Advanced touch input handler failed after {} attempts, giving up",
                                max_retries
                            );
                            break;
                        }

                        let delay = Duration::from_secs(5) * 2_u32.pow(retry_count.min(5));
                        warn!(
                            "Retrying advanced touch device connection in {:?} (attempt {}/{})",
                            delay, retry_count, max_retries
                        );
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        });

        Ok(())
    }

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
                    std::io::ErrorKind::NotFound => {
                        TouchError::DeviceNotFound(device_path.to_string())
                    }
                    std::io::ErrorKind::PermissionDenied => {
                        TouchError::PermissionDenied(device_path.to_string())
                    }
                    _ => TouchError::Device(format!("Failed to open {}: {}", device_path, e)),
                };

                return Err(DoorcamError::component(
                    "advanced_touch_input".to_string(),
                    touch_error.user_message(),
                ));
            }
        };

        info!(
            "Advanced touch device opened successfully: {} ({})",
            device_path,
            device.name().unwrap_or("Unknown")
        );
        debug!("Device capabilities: {:?}", device.supported_events());

        TouchInputHandler::validate_touch_device(&device, device_path)?;

        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: u32 = 5;

        loop {
            match device.fetch_events() {
                Ok(events) => {
                    consecutive_errors = 0;

                    for event in events {
                        if let Some(touch_event) =
                            Self::parse_advanced_touch_event(&event, current_position)
                        {
                            let now = SystemTime::now();
                            let should_process = {
                                let last_time = last_touch_time.read().unwrap();
                                match *last_time {
                                    Some(last) => {
                                        now.duration_since(last).unwrap_or_default()
                                            >= debounce_duration
                                    }
                                    None => true,
                                }
                            };

                            if should_process {
                                debug!("Advanced touch event: {:?}", touch_event);

                                {
                                    let mut last_time = last_touch_time.write().unwrap();
                                    *last_time = Some(now);
                                }

                                let _ = event_bus
                                    .publish(DoorcamEvent::TouchDetected {
                                        timestamp: touch_event.timestamp,
                                    })
                                    .await;
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
                            format!(
                                "Too many consecutive errors reading from touch device: {}",
                                e
                            ),
                        ));
                    }

                    warn!(
                        "Error reading from advanced touch device (attempt {}): {}",
                        consecutive_errors, e
                    );
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub(crate) fn parse_advanced_touch_event(
        event: &evdev::InputEvent,
        current_position: &std::sync::Arc<std::sync::RwLock<(Option<i32>, Option<i32>)>>,
    ) -> Option<TouchEvent> {
        match event.kind() {
            InputEventKind::Key(key) => match key {
                Key::BTN_TOUCH | Key::BTN_LEFT | Key::BTN_RIGHT | Key::BTN_MIDDLE => {
                    let touch_type = if event.value() == 1 {
                        TouchEventType::Press
                    } else if event.value() == 0 {
                        TouchEventType::Release
                    } else {
                        return None;
                    };

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
            },
            InputEventKind::AbsAxis(axis) => {
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
                None
            }
            _ => None,
        }
    }
}
