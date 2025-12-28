// Core building blocks
pub mod core;

// Feature modules
pub mod analyzer;
pub mod camera;
pub mod capture;
pub mod display;
pub mod streaming;
pub mod touch;

// Integration layers

// Infrastructure
pub mod infrastructure;

// Application coordination
pub mod app;

// Re-export common types at the crate root
pub use analyzer::MotionAnalyzer;
pub use analyzer::{
    MotionAnalysisMetrics, MotionAnalyzerIntegration, MotionAnalyzerIntegrationBuilder,
};
pub use app::{keyboard_input, ComponentState, DoorcamOrchestrator, ShutdownReason};
pub use camera::{
    calculate_ring_buffer_capacity, CameraRingBufferIntegration,
    CameraRingBufferIntegrationBuilder, HealthCheckResult as CameraHealthCheckResult,
    HealthStatus as CameraHealthStatus, IntegrationStatus as CameraIntegrationStatus,
};
pub use camera::{CameraInterface, CameraInterfaceBuilder};
pub use capture::{
    CaptureMetadata, CaptureStats, VideoCapture, VideoCaptureIntegration,
    VideoCaptureIntegrationBuilder,
};
pub use config::DoorcamConfig;
pub use core::{config, error, events, frame, health, recovery, ring_buffer};
pub use display::{
    DisplayController, DisplayConverter, DisplayIntegration, DisplayIntegrationBuilder,
    DisplayIntegrationWithStats, DisplayStats,
};
pub use error::{DoorcamError, Result};
pub use events::{
    DoorcamEvent, EventBus, EventDebugger, EventFilter, EventHandler, EventMetrics, EventPipeline,
    EventProcessor, EventReceiver, EventRoute, EventRouter,
};
pub use frame::{FrameData, FrameFormat, ProcessedFrame, Rotation};
pub use health::{HealthCheckResult, HealthChecker, SystemHealthManager, SystemMetrics};
pub use infrastructure::{storage, wal};
pub use keyboard_input::KeyboardInputHandler;
pub use recovery::{
    CameraRecovery, ComponentHealth, GracefulDegradation, HealthMonitor, RecoveryAction,
    RecoveryConfig, RecoveryManager, TouchRecovery,
};
pub use ring_buffer::{RingBuffer, RingBufferBuilder};
pub use storage::{
    CleanupResult, EventStorage, StorageStats, StoredEventMetadata, StoredEventType,
};
pub use storage::{
    CombinedStorageStats, EventStorageIntegration, EventStorageIntegrationBuilder,
    StorageHealthStatus, StorageHealthStatusLevel, StorageIntegrationStats,
};
pub use touch::{
    AdvancedTouchInputHandler, MockTouchInputHandler, TouchEvent, TouchEventType, TouchInputHandler,
};

pub use streaming::{FrameRateAdapter, QualityAdapter, StreamingIntegration, StreamingStats};
pub use streaming::{StreamServer, StreamServerBuilder, StreamStats};
