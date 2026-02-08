use crate::error::{DoorcamError, Result};
use evdev::{Device, EventType, Key};
use tracing::info;

/// Utility functions for touch device management
pub struct TouchDeviceUtils;

impl TouchDeviceUtils {
    /// Discover available touch input devices
    pub fn discover_touch_devices() -> Vec<String> {
        let mut devices = Vec::new();

        for i in 0..10 {
            let device_path = format!("/dev/input/event{}", i);
            if let Ok(device) = Device::open(&device_path) {
                let supported_events = device.supported_events();

                if supported_events.contains(EventType::KEY) {
                    if let Some(keys) = device.supported_keys() {
                        let has_touch_keys = keys.contains(Key::BTN_TOUCH)
                            || keys.contains(Key::BTN_LEFT)
                            || keys.contains(Key::BTN_RIGHT);

                        if has_touch_keys {
                            let device_name = device.name().unwrap_or("Unknown").to_string();
                            info!(
                                "Found potential touch device: {} ({})",
                                device_path, device_name
                            );
                            devices.push(device_path);
                        }
                    }
                }
            }
        }

        devices
    }

    /// Get device information for a given path
    pub fn get_device_info(device_path: &str) -> Result<TouchDeviceInfo> {
        let device = Device::open(device_path).map_err(|e| {
            DoorcamError::component(
                "touch_device_utils".to_string(),
                format!("Failed to open device {}: {}", device_path, e),
            )
        })?;

        Ok(TouchDeviceInfo {
            path: device_path.to_string(),
            name: device.name().unwrap_or("Unknown").to_string(),
            vendor_id: device.input_id().vendor(),
            product_id: device.input_id().product(),
            supports_touch: device
                .supported_keys()
                .map(|keys| keys.contains(Key::BTN_TOUCH) || keys.contains(Key::BTN_LEFT))
                .unwrap_or(false),
            supports_absolute: device.supported_events().contains(EventType::ABSOLUTE),
            supports_relative: device.supported_events().contains(EventType::RELATIVE),
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
        format!(
            "{} ({}) - Touch: {}, Absolute: {}, Relative: {}",
            self.name,
            self.path,
            self.supports_touch,
            self.supports_absolute,
            self.supports_relative
        )
    }
}
