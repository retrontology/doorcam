pub mod config;
pub mod error;
pub mod events;
pub mod recovery;
pub mod health;
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
pub mod storage;
pub mod storage_integration;
pub mod app_orchestration;

#[cfg(feature = "streaming")]
pub mod streaming;

#[cfg(feature = "streaming")]
pub mod streaming_integration;

pub use config::DoorcamConfig;
pub use error::{DoorcamError, Result};
pub use recovery::{RecoveryManager, RecoveryAction, RecoveryConfig, CameraRecovery, TouchRecovery, HealthMonitor, ComponentHealth, GracefulDegradation};
pub use health::{SystemHealthManager, HealthChecker, HealthCheckResult, SystemMetrics};
pub use app_orchestration::{DoorcamOrchestrator, ComponentState, ShutdownReason};
pub use events::{
    DoorcamEvent, EventBus, EventFilter, EventReceiver,
    EventRouter, EventRoute, EventHandler, EventPipeline, EventProcessor,
    EventMetrics, EventDebugger
};
pub use frame::{FrameData, FrameFormat, ProcessedFrame, Rotation};
pub use ring_buffer::{RingBuffer, RingBufferBuilder};
pub use camera::{CameraInterface, CameraInterfaceBuilder};
pub use integration::{CameraRingBufferIntegration, CameraRingBufferIntegrationBuilder, IntegrationStatus, HealthStatus};
pub use analyzer::MotionAnalyzer;
pub use analyzer_integration::{MotionAnalyzerIntegration, MotionAnalyzerIntegrationBuilder, MotionAnalysisMetrics};
pub use display::{DisplayController, DisplayConverter};
pub use touch::{TouchInputHandler, MockTouchInputHandler, AdvancedTouchInputHandler, TouchEvent, TouchEventType};
pub use display_integration::{DisplayIntegration, DisplayIntegrationBuilder, DisplayIntegrationWithStats, DisplayStats};
pub use capture::{VideoCapture, CaptureStats, CaptureMetadata};
pub use capture_integration::{VideoCaptureIntegration, VideoCaptureIntegrationBuilder};
pub use storage::{EventStorage, StoredEventMetadata, StoredEventType, StorageStats, CleanupResult};
pub use storage_integration::{EventStorageIntegration, EventStorageIntegrationBuilder, StorageIntegrationStats, CombinedStorageStats, StorageHealthStatus, HealthStatus as StorageHealthStatusLevel};

#[cfg(feature = "streaming")]
pub use streaming::{StreamServer, StreamServerBuilder, StreamStats};

#[cfg(feature = "streaming")]
pub use streaming_integration::{StreamingIntegration, StreamingStats, FrameRateAdapter, QualityAdapter};