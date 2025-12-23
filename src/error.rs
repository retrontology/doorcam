#![allow(dead_code)]

use std::time::Duration;
use thiserror::Error;

/// Main error type for the doorcam system
#[derive(Error, Debug)]
pub enum DoorcamError {
    #[error("Configuration error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] toml::ser::Error),

    #[error("Deserialization error: {0}")]
    Deserialization(#[from] toml::de::Error),

    #[error("Camera error: {0}")]
    Camera(#[from] CameraError),

    #[error("Motion analysis error: {0}")]
    Analyzer(#[from] AnalyzerError),

    #[error("Stream server error: {0}")]
    Stream(#[from] StreamError),

    #[error("Display error: {0}")]
    Display(#[from] DisplayError),

    #[error("Touch input error: {0}")]
    Touch(#[from] TouchError),

    #[error("Video capture error: {0}")]
    Capture(#[from] CaptureError),

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Event bus error: {0}")]
    EventBus(#[from] EventBusError),

    #[error("Frame processing error: {0}")]
    Processing(#[from] ProcessingError),

    #[error("Ring buffer error: {0}")]
    RingBuffer(#[from] RingBufferError),

    #[error("System error: {message}")]
    System { message: String },

    #[error("Component error in {component}: {message}")]
    Component { component: String, message: String },

    #[error("Recovery failed for {component} after {attempts} attempts")]
    RecoveryFailed { component: String, attempts: u32 },

    #[error("Graceful shutdown requested")]
    Shutdown,
}

/// Camera-specific error types
#[derive(Error, Debug, Clone)]
pub enum CameraError {
    #[error("Failed to open camera device {device}")]
    DeviceOpen { device: u32 },

    #[error("Failed to open camera device {device}: {details}")]
    DeviceOpenWithSource { device: u32, details: String },

    #[error("Unsupported format: {format}")]
    UnsupportedFormat { format: String },

    #[error("Failed to configure camera: {details}")]
    Configuration { details: String },

    #[error("Capture stream error: {details}")]
    CaptureStream { details: String },

    #[error("Camera not available (feature disabled or platform unsupported)")]
    NotAvailable,

    #[error("Camera disconnected")]
    Disconnected,

    #[error("Frame timeout after {timeout:?}")]
    FrameTimeout { timeout: Duration },
}

/// Motion analyzer error types
#[derive(Error, Debug)]
pub enum AnalyzerError {
    #[error("Image processing initialization failed: {details}")]
    ImageProcInit { details: String },

    #[error("Background model creation failed: {details}")]
    BackgroundModel { details: String },

    #[error("Frame processing failed: {details}")]
    FrameProcessing { details: String },

    #[error("Motion detection algorithm failed: {details}")]
    MotionDetection { details: String },

    #[error("Feature not available (motion analysis disabled)")]
    NotAvailable,

    #[cfg(feature = "motion_analysis")]
    #[error("Image processing error: {0}")]
    ImageError(#[from] image::ImageError),
}

/// Stream server error types
#[derive(Error, Debug)]
pub enum StreamError {
    #[error("Failed to bind to {address}: {source}")]
    BindFailed {
        address: String,
        source: std::io::Error,
    },

    #[error("Server startup failed: {details}")]
    StartupFailed { details: String },

    #[error("Client connection error: {details}")]
    ClientConnection { details: String },

    #[error("Frame encoding failed: {details}")]
    FrameEncoding { details: String },

    #[error("Network error: {0}")]
    Network(#[from] std::io::Error),

    #[error("HTTP server error: {details}")]
    Http { details: String },
}

/// Display controller error types
#[derive(Error, Debug)]
pub enum DisplayError {
    #[error("Framebuffer error: {details}")]
    Framebuffer { details: String },

    #[error("Failed to open framebuffer device {device}: {source}")]
    FramebufferOpen {
        device: String,
        source: std::io::Error,
    },

    #[error("Backlight control error: {details}")]
    Backlight { details: String },

    #[error("Failed to open backlight device {device}: {source}")]
    BacklightOpen {
        device: String,
        source: std::io::Error,
    },

    #[error("Frame rendering failed: {details}")]
    Rendering { details: String },

    #[error("Display format conversion failed: {details}")]
    FormatConversion { details: String },

    #[error("Display not available (feature disabled)")]
    NotAvailable,
}

/// Touch input error types
#[derive(Error, Debug, Clone)]
pub enum TouchError {
    #[error("Failed to open touch device {device}: {details}")]
    DeviceOpen { device: String, details: String },

    #[error("Touch device read error: {details}")]
    DeviceRead { details: String },

    #[error("Event parsing failed: {details}")]
    EventParsing { details: String },

    #[error("Touch device not available")]
    NotAvailable,

    #[error("Device error: {0}")]
    Device(String),

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Unsupported device: {0}")]
    UnsupportedDevice(String),
}

/// Video capture error types
#[derive(Error, Debug)]
pub enum CaptureError {
    #[error("Failed to create capture directory {path}: {source}")]
    DirectoryCreation {
        path: String,
        source: std::io::Error,
    },

    #[error("Frame save failed: {details}")]
    FrameSave { details: String },

    #[error("Video encoding failed: {details}")]
    VideoEncoding { details: String },

    #[error("Preroll frame retrieval failed: {details}")]
    PrerollRetrieval { details: String },

    #[error("Capture session management error: {details}")]
    SessionManagement { details: String },

    #[error("Metadata write failed: {details}")]
    MetadataWrite { details: String },
}

/// Storage management error types
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Cleanup operation failed: {details}")]
    CleanupFailed { details: String },

    #[error("Directory scan failed for {path}: {source}")]
    DirectoryScan {
        path: String,
        source: std::io::Error,
    },

    #[error("File deletion failed for {path}: {source}")]
    FileDeletion {
        path: String,
        source: std::io::Error,
    },

    #[error("Storage space check failed: {details}")]
    SpaceCheck { details: String },

    #[error("Retention policy validation failed: {details}")]
    RetentionPolicy { details: String },
}

/// Event bus error types
#[derive(Error, Debug)]
pub enum EventBusError {
    #[error("Failed to publish event: {details}")]
    PublishFailed { details: String },

    #[error("Failed to subscribe to events: {details}")]
    SubscribeFailed { details: String },

    #[error("Event channel closed")]
    ChannelClosed,

    #[error("Event serialization failed: {details}")]
    Serialization { details: String },
}

/// Frame processing error types
#[derive(Error, Debug)]
pub enum ProcessingError {
    #[error("Frame conversion failed from {from} to {to}: {details}")]
    Conversion {
        from: String,
        to: String,
        details: String,
    },

    #[error("Frame rotation failed: {details}")]
    Rotation { details: String },

    #[error("JPEG encoding failed: {details}")]
    JpegEncoding { details: String },

    #[error("Frame format not supported: {format}")]
    UnsupportedFormat { format: String },

    #[error("Image processing library error: {details}")]
    ImageLibrary { details: String },
}

/// Ring buffer error types
#[derive(Error, Debug)]
pub enum RingBufferError {
    #[error("Buffer overflow: attempted to store more than {capacity} frames")]
    Overflow { capacity: usize },

    #[error("Buffer underflow: no frames available")]
    Underflow,

    #[error("Invalid frame timestamp: {details}")]
    InvalidTimestamp { details: String },

    #[error("Frame retrieval failed: {details}")]
    FrameRetrieval { details: String },

    #[error("Buffer corruption detected: {details}")]
    Corruption { details: String },
}

impl DoorcamError {
    /// Create a system error with a message
    pub fn system<S: Into<String>>(message: S) -> Self {
        Self::System {
            message: message.into(),
        }
    }

    /// Create a component error with component name and message
    pub fn component<S: Into<String>>(component: S, message: S) -> Self {
        Self::Component {
            component: component.into(),
            message: message.into(),
        }
    }

    /// Create a recovery failed error
    pub fn recovery_failed<S: Into<String>>(component: S, attempts: u32) -> Self {
        Self::RecoveryFailed {
            component: component.into(),
            attempts,
        }
    }

    /// Check if this error is recoverable
    pub fn is_recoverable(&self) -> bool {
        match self {
            DoorcamError::Camera(CameraError::Disconnected) => true,
            DoorcamError::Camera(CameraError::DeviceOpen { .. }) => true,
            DoorcamError::Camera(CameraError::DeviceOpenWithSource { .. }) => true,
            DoorcamError::Camera(CameraError::FrameTimeout { .. }) => true,
            DoorcamError::Touch(TouchError::DeviceOpen { .. }) => true,
            DoorcamError::Touch(TouchError::DeviceRead { .. }) => true,
            DoorcamError::Stream(StreamError::ClientConnection { .. }) => true,
            DoorcamError::Display(DisplayError::FramebufferOpen { .. }) => true,
            DoorcamError::Display(DisplayError::BacklightOpen { .. }) => true,
            DoorcamError::EventBus(EventBusError::ChannelClosed) => true,
            DoorcamError::Io(_) => true,
            DoorcamError::System { .. } => false,
            DoorcamError::Shutdown => false,
            DoorcamError::RecoveryFailed { .. } => false,
            _ => false,
        }
    }

    /// Get the component name associated with this error
    pub fn component_name(&self) -> String {
        match self {
            DoorcamError::Camera(_) => "camera".to_string(),
            DoorcamError::Analyzer(_) => "analyzer".to_string(),
            DoorcamError::Stream(_) => "stream".to_string(),
            DoorcamError::Display(_) => "display".to_string(),
            DoorcamError::Touch(_) => "touch".to_string(),
            DoorcamError::Capture(_) => "capture".to_string(),
            DoorcamError::Storage(_) => "storage".to_string(),
            DoorcamError::EventBus(_) => "event_bus".to_string(),
            DoorcamError::Processing(_) => "processing".to_string(),
            DoorcamError::RingBuffer(_) => "ring_buffer".to_string(),
            DoorcamError::Config(_) => "config".to_string(),
            DoorcamError::Component { component, .. } => component.clone(),
            _ => "system".to_string(),
        }
    }

    /// Get error severity level for logging
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            DoorcamError::Shutdown => ErrorSeverity::Info,
            DoorcamError::Camera(CameraError::NotAvailable) => ErrorSeverity::Warning,
            DoorcamError::Display(DisplayError::NotAvailable) => ErrorSeverity::Warning,
            DoorcamError::Analyzer(AnalyzerError::NotAvailable) => ErrorSeverity::Warning,
            DoorcamError::RecoveryFailed { .. } => ErrorSeverity::Critical,
            DoorcamError::Config(_) => ErrorSeverity::Critical,
            _ if self.is_recoverable() => ErrorSeverity::Warning,
            _ => ErrorSeverity::Error,
        }
    }
}

/// Error severity levels for structured logging
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

impl ErrorSeverity {
    /// Convert to tracing level
    pub fn to_tracing_level(&self) -> tracing::Level {
        match self {
            ErrorSeverity::Info => tracing::Level::INFO,
            ErrorSeverity::Warning => tracing::Level::WARN,
            ErrorSeverity::Error => tracing::Level::ERROR,
            ErrorSeverity::Critical => tracing::Level::ERROR,
        }
    }
}

/// Convenience type alias for Results
pub type Result<T> = std::result::Result<T, DoorcamError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_error_display_formatting() {
        let camera_error = DoorcamError::Camera(CameraError::DeviceOpen { device: 0 });
        assert_eq!(
            camera_error.to_string(),
            "Camera error: Failed to open camera device 0"
        );

        let analyzer_error = DoorcamError::Analyzer(AnalyzerError::FrameProcessing {
            details: "Test error".to_string(),
        });
        assert_eq!(
            analyzer_error.to_string(),
            "Motion analysis error: Frame processing failed: Test error"
        );

        let system_error = DoorcamError::system("Test system error");
        assert_eq!(system_error.to_string(), "System error: Test system error");
    }

    #[test]
    fn test_error_source_chains() {
        use std::error::Error;

        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
        let doorcam_error = DoorcamError::Io(io_error);

        assert!(doorcam_error.source().is_some());
        assert_eq!(
            doorcam_error.source().unwrap().to_string(),
            "File not found"
        );
    }

    #[test]
    fn test_recoverable_error_classification() {
        // Recoverable errors
        assert!(DoorcamError::Camera(CameraError::Disconnected).is_recoverable());
        assert!(DoorcamError::Camera(CameraError::DeviceOpen { device: 0 }).is_recoverable());
        assert!(DoorcamError::Camera(CameraError::FrameTimeout {
            timeout: Duration::from_secs(1)
        })
        .is_recoverable());
        assert!(DoorcamError::Touch(TouchError::DeviceOpen {
            device: "/dev/input/event0".to_string(),
            details: "Test".to_string()
        })
        .is_recoverable());
        assert!(DoorcamError::Touch(TouchError::DeviceRead {
            details: "Test".to_string()
        })
        .is_recoverable());

        // Non-recoverable errors
        assert!(!DoorcamError::system("Critical error").is_recoverable());
        assert!(!DoorcamError::Shutdown.is_recoverable());
        assert!(!DoorcamError::recovery_failed("camera", 5).is_recoverable());
        assert!(
            !DoorcamError::Touch(TouchError::PermissionDenied("Test".to_string())).is_recoverable()
        );
    }

    #[test]
    fn test_component_name_extraction() {
        assert_eq!(
            DoorcamError::Camera(CameraError::Disconnected).component_name(),
            "camera"
        );
        assert_eq!(
            DoorcamError::Analyzer(AnalyzerError::NotAvailable).component_name(),
            "analyzer"
        );
        assert_eq!(
            DoorcamError::Touch(TouchError::NotAvailable).component_name(),
            "touch"
        );
        assert_eq!(DoorcamError::system("test").component_name(), "system");
        assert_eq!(
            DoorcamError::component("custom", "test").component_name(),
            "custom"
        );
    }

    #[test]
    fn test_error_severity_levels() {
        use ErrorSeverity::*;

        assert_eq!(DoorcamError::Shutdown.severity(), Info);
        assert_eq!(
            DoorcamError::Camera(CameraError::NotAvailable).severity(),
            Warning
        );
        assert_eq!(
            DoorcamError::recovery_failed("test", 3).severity(),
            Critical
        );
        assert_eq!(
            DoorcamError::Camera(CameraError::Disconnected).severity(),
            Warning
        );
        assert_eq!(DoorcamError::system("error").severity(), Error);
    }

    #[test]
    fn test_error_severity_to_tracing_level() {
        use ErrorSeverity::*;

        assert_eq!(Info.to_tracing_level(), tracing::Level::INFO);
        assert_eq!(Warning.to_tracing_level(), tracing::Level::WARN);
        assert_eq!(Error.to_tracing_level(), tracing::Level::ERROR);
        assert_eq!(Critical.to_tracing_level(), tracing::Level::ERROR);
    }

    #[test]
    fn test_camera_error_types() {
        let device_error = CameraError::DeviceOpen { device: 1 };
        assert_eq!(device_error.to_string(), "Failed to open camera device 1");

        let config_error = CameraError::Configuration {
            details: "Invalid format".to_string(),
        };
        assert_eq!(
            config_error.to_string(),
            "Failed to configure camera: Invalid format"
        );

        let timeout_error = CameraError::FrameTimeout {
            timeout: Duration::from_millis(500),
        };
        assert!(timeout_error.to_string().contains("Frame timeout"));
    }

    #[test]
    fn test_analyzer_error_types() {
        let processing_error = AnalyzerError::FrameProcessing {
            details: "Invalid frame format".to_string(),
        };
        assert_eq!(
            processing_error.to_string(),
            "Frame processing failed: Invalid frame format"
        );

        let motion_error = AnalyzerError::MotionDetection {
            details: "Algorithm failed".to_string(),
        };
        assert_eq!(
            motion_error.to_string(),
            "Motion detection algorithm failed: Algorithm failed"
        );

        let not_available_error = AnalyzerError::NotAvailable;
        assert_eq!(
            not_available_error.to_string(),
            "Feature not available (motion analysis disabled)"
        );
    }

    #[test]
    fn test_error_builder_methods() {
        let system_error = DoorcamError::system("Test message");
        match system_error {
            DoorcamError::System { message } => assert_eq!(message, "Test message"),
            _ => panic!("Expected System error"),
        }

        let component_error = DoorcamError::component("test_component", "Test message");
        match component_error {
            DoorcamError::Component { component, message } => {
                assert_eq!(component, "test_component");
                assert_eq!(message, "Test message");
            }
            _ => panic!("Expected Component error"),
        }

        let recovery_error = DoorcamError::recovery_failed("camera", 3);
        match recovery_error {
            DoorcamError::RecoveryFailed {
                component,
                attempts,
            } => {
                assert_eq!(component, "camera");
                assert_eq!(attempts, 3);
            }
            _ => panic!("Expected RecoveryFailed error"),
        }
    }
}
