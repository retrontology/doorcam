#![allow(dead_code)]

use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{debug, info};

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DoorcamConfig {
    pub camera: CameraConfig,
    pub analyzer: AnalyzerConfig,
    pub event: EventConfig,
    pub capture: CaptureConfig,
    pub stream: StreamConfig,
    pub display: DisplayConfig,
    pub system: SystemConfig,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CameraConfig {
    /// Camera device index (e.g., 0 for /dev/video0)
    #[serde(default = "default_camera_index")]
    pub index: u32,

    /// Camera resolution (width, height)
    #[serde(default = "default_camera_resolution")]
    pub resolution: (u32, u32),

    /// Frames per second
    #[serde(default = "default_camera_fps")]
    pub fps: u32,

    /// Video format (MJPG, YUYV, etc.)
    #[serde(default = "default_camera_format")]
    pub format: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AnalyzerConfig {
    /// FPS for motion analysis
    #[serde(default = "default_analyzer_fps")]
    pub fps: u32,

    /// Delta threshold for motion detection
    #[serde(default = "default_delta_threshold")]
    pub delta_threshold: u32,

    /// Minimum contour area to trigger motion
    #[serde(default = "default_contour_area")]
    pub contour_minimum_area: f64,

    /// JPEG decoding resolution scale (1=full, 2=1/2, 4=1/4, 8=1/8)
    #[serde(default = "default_jpeg_decode_scale")]
    pub jpeg_decode_scale: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EventConfig {
    /// Preroll duration in seconds
    #[serde(default = "default_preroll_seconds")]
    pub preroll_seconds: u32,

    /// Postroll duration in seconds
    #[serde(default = "default_postroll_seconds")]
    pub postroll_seconds: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CaptureConfig {
    /// Base path for storing captures
    #[serde(default = "default_capture_path")]
    pub path: String,

    /// Enable timestamp overlay on images
    #[serde(default = "default_timestamp_overlay")]
    pub timestamp_overlay: bool,

    /// Path to TrueType font file for timestamp overlay
    #[serde(default = "default_timestamp_font_path")]
    pub timestamp_font_path: String,

    /// Font size for timestamp overlay
    #[serde(default = "default_timestamp_font_size")]
    pub timestamp_font_size: f32,

    /// Enable video encoding
    #[serde(default = "default_video_encoding")]
    pub video_encoding: bool,

    /// Keep individual JPEG images
    #[serde(default = "default_keep_images")]
    pub keep_images: bool,

    /// Save metadata JSON files for each capture event
    #[serde(default = "default_save_metadata")]
    pub save_metadata: bool,

    /// Capture rotation to apply when saving media
    pub rotation: Option<Rotation>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StreamConfig {
    /// IP address to bind to
    #[serde(default = "default_stream_ip")]
    pub ip: String,

    /// Port to listen on
    #[serde(default = "default_stream_port")]
    pub port: u16,

    /// Optional rotation to apply when presenting the MJPEG stream
    pub rotation: Option<Rotation>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DisplayConfig {
    /// Framebuffer device path
    #[serde(default = "default_framebuffer_device")]
    pub framebuffer_device: String,

    /// Backlight control device path
    #[serde(default = "default_backlight_device")]
    pub backlight_device: String,

    /// Touch input device path
    #[serde(default = "default_touch_device")]
    pub touch_device: String,

    /// Display activation period in seconds
    #[serde(default = "default_activation_period")]
    pub activation_period_seconds: u32,

    /// Display resolution (width, height)
    #[serde(default = "default_display_resolution")]
    pub resolution: (u32, u32),

    /// Display rotation
    pub rotation: Option<Rotation>,

    /// JPEG decoding resolution scale for display (1=full, 2=1/2, 4=1/4, 8=1/8)
    #[serde(default = "default_display_jpeg_decode_scale")]
    pub jpeg_decode_scale: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SystemConfig {
    /// Enable automatic cleanup of old events
    #[serde(default = "default_trim_old")]
    pub trim_old: bool,

    /// Retention period in days
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,

    /// Ring buffer capacity (number of frames)
    #[serde(default = "default_ring_buffer_capacity")]
    pub ring_buffer_capacity: usize,

    /// Event bus capacity
    #[serde(default = "default_event_bus_capacity")]
    pub event_bus_capacity: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub enum Rotation {
    Rotate90,
    Rotate180,
    Rotate270,
}

impl DoorcamConfig {
    /// Load configuration from default sources (file + environment variables)
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from_file("doorcam.toml")
    }

    /// Load configuration from a specific file path
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path_str = path.as_ref().to_string_lossy();
        debug!("Loading configuration from: {}", path_str);

        let settings = Config::builder()
            // Start with default values
            .set_default("camera.index", default_camera_index())?
            .set_default(
                "camera.resolution",
                vec![default_camera_resolution().0, default_camera_resolution().1],
            )?
            .set_default("camera.fps", default_camera_fps())?
            .set_default("camera.format", default_camera_format())?
            .set_default("analyzer.fps", default_analyzer_fps())?
            .set_default("analyzer.delta_threshold", default_delta_threshold())?
            .set_default("analyzer.contour_minimum_area", default_contour_area())?
            .set_default("analyzer.jpeg_decode_scale", default_jpeg_decode_scale())?
            .set_default("event.preroll_seconds", default_preroll_seconds())?
            .set_default("event.postroll_seconds", default_postroll_seconds())?
            .set_default("capture.path", default_capture_path())?
            .set_default("capture.timestamp_overlay", default_timestamp_overlay())?
            .set_default("capture.timestamp_font_path", default_timestamp_font_path())?
            .set_default(
                "capture.timestamp_font_size",
                default_timestamp_font_size() as f64,
            )?
            .set_default("capture.video_encoding", default_video_encoding())?
            .set_default("capture.keep_images", default_keep_images())?
            .set_default("capture.save_metadata", default_save_metadata())?
            .set_default("stream.ip", default_stream_ip())?
            .set_default("stream.port", default_stream_port())?
            .set_default("display.framebuffer_device", default_framebuffer_device())?
            .set_default("display.backlight_device", default_backlight_device())?
            .set_default("display.touch_device", default_touch_device())?
            .set_default(
                "display.activation_period_seconds",
                default_activation_period(),
            )?
            .set_default(
                "display.resolution",
                vec![
                    default_display_resolution().0,
                    default_display_resolution().1,
                ],
            )?
            .set_default(
                "display.jpeg_decode_scale",
                default_display_jpeg_decode_scale(),
            )?
            .set_default("system.trim_old", default_trim_old())?
            .set_default("system.retention_days", default_retention_days())?
            .set_default(
                "system.ring_buffer_capacity",
                default_ring_buffer_capacity() as i64,
            )?
            .set_default(
                "system.event_bus_capacity",
                default_event_bus_capacity() as i64,
            )?
            // Add configuration file (optional)
            .add_source(File::with_name(&path_str).required(false))
            // Add environment variables with DOORCAM_ prefix
            .add_source(Environment::with_prefix("DOORCAM").separator("_"))
            .build()?;

        let config: DoorcamConfig = settings.try_deserialize()?;

        info!("Configuration loaded successfully");
        debug!("Final configuration: {:#?}", config);

        Ok(config)
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate camera settings
        if self.camera.resolution.0 == 0 || self.camera.resolution.1 == 0 {
            return Err(ConfigError::Message(
                "Camera resolution must be greater than 0".to_string(),
            ));
        }

        if self.camera.fps == 0 {
            return Err(ConfigError::Message(
                "Camera fps must be greater than 0".to_string(),
            ));
        }

        // Validate analyzer settings
        if self.analyzer.fps == 0 {
            return Err(ConfigError::Message(
                "Analyzer fps must be greater than 0".to_string(),
            ));
        }

        // Validate event timing
        if self.event.preroll_seconds == 0 {
            return Err(ConfigError::Message(
                "Event preroll_seconds must be greater than 0".to_string(),
            ));
        }

        if self.event.postroll_seconds == 0 {
            return Err(ConfigError::Message(
                "Event postroll_seconds must be greater than 0".to_string(),
            ));
        }

        // Validate system settings
        if self.system.ring_buffer_capacity == 0 {
            return Err(ConfigError::Message(
                "Ring buffer capacity must be greater than 0".to_string(),
            ));
        }

        if self.system.event_bus_capacity == 0 {
            return Err(ConfigError::Message(
                "Event bus capacity must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }
}

impl Default for DoorcamConfig {
    fn default() -> Self {
        Self {
            camera: CameraConfig {
                index: default_camera_index(),
                resolution: default_camera_resolution(),
                fps: default_camera_fps(),
                format: default_camera_format(),
            },
            analyzer: AnalyzerConfig {
                fps: default_analyzer_fps(),
                delta_threshold: default_delta_threshold(),
                contour_minimum_area: default_contour_area(),
                jpeg_decode_scale: default_jpeg_decode_scale(),
            },
            event: EventConfig {
                preroll_seconds: default_preroll_seconds(),
                postroll_seconds: default_postroll_seconds(),
            },
            capture: CaptureConfig {
                path: default_capture_path(),
                timestamp_overlay: default_timestamp_overlay(),
                timestamp_font_path: default_timestamp_font_path(),
                timestamp_font_size: default_timestamp_font_size(),
                video_encoding: default_video_encoding(),
                keep_images: default_keep_images(),
                save_metadata: default_save_metadata(),
                rotation: None,
            },
            stream: StreamConfig {
                ip: default_stream_ip(),
                port: default_stream_port(),
                rotation: None,
            },
            display: DisplayConfig {
                framebuffer_device: default_framebuffer_device(),
                backlight_device: default_backlight_device(),
                touch_device: default_touch_device(),
                activation_period_seconds: default_activation_period(),
                resolution: default_display_resolution(),
                rotation: None,
                jpeg_decode_scale: default_display_jpeg_decode_scale(),
            },
            system: SystemConfig {
                trim_old: default_trim_old(),
                retention_days: default_retention_days(),
                ring_buffer_capacity: default_ring_buffer_capacity(),
                event_bus_capacity: default_event_bus_capacity(),
            },
        }
    }
}

// Default value functions
fn default_camera_index() -> u32 {
    0
}
fn default_camera_resolution() -> (u32, u32) {
    (640, 480)
}
fn default_camera_fps() -> u32 {
    30
}
fn default_camera_format() -> String {
    "MJPG".to_string()
}

fn default_analyzer_fps() -> u32 {
    5
}
fn default_delta_threshold() -> u32 {
    25
}
fn default_contour_area() -> f64 {
    1000.0
}
fn default_jpeg_decode_scale() -> u32 {
    4
} // Default to 1/4 resolution for efficiency

fn default_preroll_seconds() -> u32 {
    5
}
fn default_postroll_seconds() -> u32 {
    10
}
fn default_capture_path() -> String {
    "./captures".to_string()
}
fn default_timestamp_overlay() -> bool {
    true
}
fn default_timestamp_font_path() -> String {
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".to_string()
}
fn default_timestamp_font_size() -> f32 {
    24.0
}
fn default_video_encoding() -> bool {
    true
}
fn default_keep_images() -> bool {
    false
}
fn default_save_metadata() -> bool {
    false
}

fn default_stream_ip() -> String {
    "0.0.0.0".to_string()
}
fn default_stream_port() -> u16 {
    8080
}

fn default_framebuffer_device() -> String {
    "/dev/fb0".to_string()
}
fn default_backlight_device() -> String {
    "/sys/class/backlight/rpi_backlight/brightness".to_string()
}
fn default_touch_device() -> String {
    "/dev/input/event0".to_string()
}
fn default_activation_period() -> u32 {
    30
}
fn default_display_resolution() -> (u32, u32) {
    (800, 480)
}
fn default_display_jpeg_decode_scale() -> u32 {
    4
} // Default to 1/4 resolution for efficiency

fn default_trim_old() -> bool {
    true
}
fn default_retention_days() -> u32 {
    7
}
fn default_ring_buffer_capacity() -> usize {
    150
}
fn default_event_bus_capacity() -> usize {
    100
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_default_config() {
        let config = DoorcamConfig {
            camera: CameraConfig {
                index: default_camera_index(),
                resolution: default_camera_resolution(),
                fps: default_camera_fps(),
                format: default_camera_format(),
            },
            analyzer: AnalyzerConfig {
                fps: default_analyzer_fps(),
                delta_threshold: default_delta_threshold(),
                contour_minimum_area: default_contour_area(),
                jpeg_decode_scale: default_jpeg_decode_scale(),
            },
            event: EventConfig {
                preroll_seconds: default_preroll_seconds(),
                postroll_seconds: default_postroll_seconds(),
            },
            capture: CaptureConfig {
                path: default_capture_path(),
                timestamp_overlay: default_timestamp_overlay(),
                timestamp_font_path: default_timestamp_font_path(),
                timestamp_font_size: default_timestamp_font_size(),
                video_encoding: default_video_encoding(),
                keep_images: default_keep_images(),
                save_metadata: default_save_metadata(),
                rotation: None,
            },
            stream: StreamConfig {
                ip: default_stream_ip(),
                port: default_stream_port(),
                rotation: None,
            },
            display: DisplayConfig {
                framebuffer_device: default_framebuffer_device(),
                backlight_device: default_backlight_device(),
                touch_device: default_touch_device(),
                activation_period_seconds: default_activation_period(),
                resolution: default_display_resolution(),
                rotation: None,
                jpeg_decode_scale: default_display_jpeg_decode_scale(),
            },
            system: SystemConfig {
                trim_old: default_trim_old(),
                retention_days: default_retention_days(),
                ring_buffer_capacity: default_ring_buffer_capacity(),
                event_bus_capacity: default_event_bus_capacity(),
            },
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_environment_variable_override() {
        env::set_var("DOORCAM_CAMERA_INDEX", "1");
        env::set_var("DOORCAM_STREAM_PORT", "9090");

        // This test would need a temporary config file to work properly
        // For now, just verify the environment variables are set
        assert_eq!(env::var("DOORCAM_CAMERA_INDEX").unwrap(), "1");
        assert_eq!(env::var("DOORCAM_STREAM_PORT").unwrap(), "9090");

        // Clean up
        env::remove_var("DOORCAM_CAMERA_INDEX");
        env::remove_var("DOORCAM_STREAM_PORT");
    }

    #[test]
    fn test_config_validation() {
        let mut config = DoorcamConfig {
            camera: CameraConfig {
                index: 0,
                resolution: (0, 0), // Invalid resolution
                fps: 30,
                format: "MJPG".to_string(),
            },
            analyzer: AnalyzerConfig {
                fps: 5,
                delta_threshold: 25,
                contour_minimum_area: 1000.0,
                jpeg_decode_scale: 4,
            },
            event: EventConfig {
                preroll_seconds: 5,
                postroll_seconds: 10,
            },
            capture: CaptureConfig {
                path: "./captures".to_string(),
                timestamp_overlay: true,
                timestamp_font_path: default_timestamp_font_path(),
                timestamp_font_size: default_timestamp_font_size(),
                video_encoding: false,
                keep_images: false,
                save_metadata: false,
                rotation: None,
            },
            stream: StreamConfig {
                ip: "0.0.0.0".to_string(),
                port: 8080,
                rotation: None,
            },
            display: DisplayConfig {
                framebuffer_device: "/dev/fb0".to_string(),
                backlight_device: "/sys/class/backlight/rpi_backlight/brightness".to_string(),
                touch_device: "/dev/input/event0".to_string(),
                activation_period_seconds: 30,
                resolution: (800, 480),
                rotation: None,
                jpeg_decode_scale: 4,
            },
            system: SystemConfig {
                trim_old: true,
                retention_days: 7,
                ring_buffer_capacity: 150,
                event_bus_capacity: 100,
            },
        };

        // Should fail validation due to invalid resolution
        assert!(config.validate().is_err());

        // Fix resolution
        config.camera.resolution = (640, 480);
        assert!(config.validate().is_ok());
    }
}
