mod core;
mod encode;
mod integration;
mod metadata;
mod overlay;
#[cfg(test)]
mod tests;

pub use core::VideoCapture;
pub use integration::{VideoCaptureIntegration, VideoCaptureIntegrationBuilder};
pub use metadata::{CaptureMetadata, CaptureStats};
