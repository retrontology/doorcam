pub mod analyzer;
pub mod analyzer_integration;
pub mod app_orchestration;
pub mod camera;
pub mod capture;
pub mod capture_integration;
pub mod config;
pub mod display;
pub mod display_integration;
pub mod error;
pub mod events;
pub mod frame;
pub mod health;
pub mod integration;
pub mod keyboard_input;
pub mod recovery;
pub mod ring_buffer;
pub mod storage;
pub mod storage_integration;
pub mod touch;
pub mod wal;

#[cfg(feature = "streaming")]
pub mod streaming;

#[cfg(feature = "streaming")]
pub mod streaming_integration;

pub use analyzer::MotionAnalyzer;
pub use analyzer_integration::{
    MotionAnalysisMetrics, MotionAnalyzerIntegration, MotionAnalyzerIntegrationBuilder,
};
pub use app_orchestration::{ComponentState, DoorcamOrchestrator, ShutdownReason};
pub use camera::{CameraInterface, CameraInterfaceBuilder};
pub use capture::{CaptureMetadata, CaptureStats, VideoCapture};
pub use capture_integration::{VideoCaptureIntegration, VideoCaptureIntegrationBuilder};
pub use config::DoorcamConfig;
pub use display::{DisplayController, DisplayConverter};
pub use display_integration::{
    DisplayIntegration, DisplayIntegrationBuilder, DisplayIntegrationWithStats, DisplayStats,
};
pub use error::{DoorcamError, Result};
pub use events::{
    DoorcamEvent, EventBus, EventDebugger, EventFilter, EventHandler, EventMetrics, EventPipeline,
    EventProcessor, EventReceiver, EventRoute, EventRouter,
};
pub use frame::{FrameData, FrameFormat, ProcessedFrame, Rotation};
pub use health::{HealthCheckResult, HealthChecker, SystemHealthManager, SystemMetrics};
pub use integration::{
    CameraRingBufferIntegration, CameraRingBufferIntegrationBuilder, HealthStatus,
    IntegrationStatus,
};
pub use keyboard_input::KeyboardInputHandler;
pub use recovery::{
    CameraRecovery, ComponentHealth, GracefulDegradation, HealthMonitor, RecoveryAction,
    RecoveryConfig, RecoveryManager, TouchRecovery,
};
pub use ring_buffer::{RingBuffer, RingBufferBuilder};
pub use storage::{
    CleanupResult, EventStorage, StorageStats, StoredEventMetadata, StoredEventType,
};
pub use storage_integration::{
    CombinedStorageStats, EventStorageIntegration, EventStorageIntegrationBuilder,
    HealthStatus as StorageHealthStatusLevel, StorageHealthStatus, StorageIntegrationStats,
};
pub use touch::{
    AdvancedTouchInputHandler, MockTouchInputHandler, TouchEvent, TouchEventType, TouchInputHandler,
};

#[cfg(feature = "streaming")]
pub use streaming::{StreamServer, StreamServerBuilder, StreamStats};

#[cfg(feature = "streaming")]
pub use streaming_integration::{
    FrameRateAdapter, QualityAdapter, StreamingIntegration, StreamingStats,
};
