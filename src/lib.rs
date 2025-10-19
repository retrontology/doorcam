pub mod config;
pub mod error;
pub mod frame;
pub mod ring_buffer;
pub mod camera;
pub mod integration;

pub use config::DoorcamConfig;
pub use error::{DoorcamError, Result};
pub use frame::{FrameData, FrameFormat, ProcessedFrame, Rotation};
pub use ring_buffer::{RingBuffer, RingBufferBuilder};
pub use camera::{CameraInterface, CameraInterfaceBuilder, CameraError};
pub use integration::{CameraRingBufferIntegration, CameraRingBufferIntegrationBuilder, IntegrationStatus, HealthCheckResult, HealthStatus};