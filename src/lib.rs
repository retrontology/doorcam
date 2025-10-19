pub mod config;
pub mod error;
pub mod events;
pub mod frame;
pub mod ring_buffer;
pub mod camera;
pub mod integration;
pub mod analyzer;
pub mod analyzer_integration;
pub mod display;
pub mod touch;
pub mod display_integration;
pub mod capture;
pub mod capture_integration;

#[cfg(feature = "streaming")]
pub mod streaming;

#[cfg(feature = "streaming")]
pub mod streaming_integration;

pub use config::DoorcamConfig;
pub use error::{DoorcamError, Result};
pub use events::{
    DoorcamEvent, EventBus, EventBusError, EventFilter, EventReceiver,
    EventRouter, EventRoute, EventHandler, EventPipeline, EventProcessor,
    EventMetrics, EventDebugger
};
pub use frame::{FrameData, FrameFormat, ProcessedFrame, Rotation};
pub use ring_buffer::{RingBuffer, RingBufferBuilder};
pub use camera::{CameraInterface, CameraInterfaceBuilder, CameraError};
pub use integration::{CameraRingBufferIntegration, CameraRingBufferIntegrationBuilder, IntegrationStatus, HealthCheckResult, HealthStatus};
pub use analyzer::MotionAnalyzer;
pub use analyzer_integration::{MotionAnalyzerIntegration, MotionAnalyzerIntegrationBuilder, MotionAnalysisMetrics};
pub use display::{DisplayController, DisplayConverter, DisplayError};
pub use touch::{TouchInputHandler, MockTouchInputHandler, AdvancedTouchInputHandler, TouchEvent, TouchEventType, TouchError};
pub use display_integration::{DisplayIntegration, DisplayIntegrationBuilder, DisplayIntegrationWithStats, DisplayStats};
pub use capture::{VideoCapture, CaptureStats, CaptureMetadata};
pub use capture_integration::{VideoCaptureIntegration, VideoCaptureIntegrationBuilder};

#[cfg(feature = "streaming")]
pub use streaming::{StreamServer, StreamServerBuilder, StreamStats};

#[cfg(feature = "streaming")]
pub use streaming_integration::{StreamingIntegration, StreamingStats, FrameRateAdapter, QualityAdapter};