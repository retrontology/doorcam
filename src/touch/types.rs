use crate::error::TouchError;

pub trait TouchErrorExt {
    fn is_recoverable(&self) -> bool;
    fn user_message(&self) -> String;
}

impl TouchErrorExt for TouchError {
    fn is_recoverable(&self) -> bool {
        matches!(
            self,
            TouchError::Device(_)
                | TouchError::DeviceOpen { .. }
                | TouchError::DeviceRead { .. }
                | TouchError::DeviceNotFound(_)
        )
    }

    fn user_message(&self) -> String {
        match self {
            TouchError::DeviceOpen { device, .. } => {
                format!("Touch device not found at {}", device)
            }
            TouchError::DeviceRead { details } => format!("Touch device read error: {}", details),
            TouchError::EventParsing { details } => {
                format!("Touch event parsing failed: {}", details)
            }
            TouchError::NotAvailable => "Touch input not available on this system".to_string(),
            TouchError::Device(msg) => format!("Touch device error: {}", msg),
            TouchError::DeviceNotFound(device) => format!("Touch device not found: {}", device),
            TouchError::PermissionDenied(device) => {
                format!("Permission denied for touch device: {}", device)
            }
            TouchError::UnsupportedDevice(device) => {
                format!("Unsupported touch device: {}", device)
            }
        }
    }
}
