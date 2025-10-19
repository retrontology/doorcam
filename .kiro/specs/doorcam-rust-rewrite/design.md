# Design Document

## Overview

The Rust rewrite of the doorcam system leverages Rust's performance, memory safety, and async ecosystem to create a robust door camera application. The design emphasizes a ring buffer architecture for consistent frame flow, type safety, hardware acceleration, and idiomatic Rust patterns.

## Architecture

### High-Level Architecture

The system uses a centralized ring buffer architecture where all components consume frames from a single source:

```
┌─────────────────┐    ┌─────────────────┐
│ Camera Interface│───▶│   Ring Buffer   │
│    (V4L2)       │    │  (Frame_Buffer) │
└─────────────────┘    └─────────────────┘
                                │
                    ┌───────────┼───────────┐
                    │           │           │
                    ▼           ▼           ▼
        ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
        │ Motion Analyzer │ │ Display         │ │ Stream Server   │
        │                 │ │ Controller      │ │   (MJPEG)       │
        └─────────────────┘ └─────────────────┘ └─────────────────┘
                │                       ▲
                │ Motion Events          │ Touch Events
                ▼                       │
        ┌─────────────────┐             │              ┌─────────────────┐
        │  Event Bus      │─────────────┘              │ Touch Input     │
        │                 │◀───────────────────────────│ Handler         │
        └─────────────────┘                            └─────────────────┘
                │
                │ Motion Events Only
                ▼
        ┌─────────────────┐
        │ Video Capture   │
        │  (motion only)  │
        └─────────────────┘
                │
                ▼
        ┌─────────────────┐
        │ Event Storage   │
        │                 │
        └─────────────────┘
```

### Core Components

1. **Camera Interface** - V4L2 video capture with hardware acceleration
2. **Ring Buffer (Frame_Buffer)** - Lock-free circular buffer for frame management
3. **Motion Analyzer** - Background subtraction and contour analysis
4. **Event Bus** - Async message passing for component coordination
5. **Display Controller** - HyperPixel 4.0 framebuffer rendering with touch support
6. **Stream Server** - HTTP/MJPEG streaming with concurrent client support
7. **Video Capture** - Motion-triggered recording with preroll/postroll
8. **Event Storage** - File management and automatic cleanup
9. **Configuration Manager** - TOML-based configuration with environment overrides

## Components and Interfaces

### Configuration System

The configuration system uses `serde` with TOML format and environment variable overrides:

```rust
use serde::{Deserialize, Serialize};
use config::{Config, ConfigError, Environment, File};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DoorcamConfig {
    pub camera: CameraConfig,
    pub analyzer: AnalyzerConfig,
    pub capture: CaptureConfig,
    pub stream: StreamConfig,
    pub display: DisplayConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CameraConfig {
    pub index: u32,
    pub resolution: (u32, u32),
    pub max_fps: u32,
    pub format: String, // "MJPG", "YUYV", etc.
    pub rotation: Option<Rotation>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AnalyzerConfig {
    pub max_fps: u32,
    pub delta_threshold: u32,
    pub contour_minimum_area: f64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CaptureConfig {
    pub preroll_seconds: u32,
    pub postroll_seconds: u32,
    pub path: String,
    pub timestamp_overlay: bool,
    pub video_encoding: bool,
    pub keep_images: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StreamConfig {
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DisplayConfig {
    pub framebuffer_device: String,
    pub backlight_device: String,
    pub touch_device: String,
    pub activation_period_seconds: u32,
    pub rotation: Option<Rotation>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum Rotation {
    Rotate90,
    Rotate180,
    Rotate270,
}

impl DoorcamConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let settings = Config::builder()
            .add_source(File::with_name("doorcam.toml").required(false))
            .add_source(Environment::with_prefix("DOORCAM").separator("_"))
            .build()?;
        
        settings.try_deserialize()
    }
}
```

### Event System

Async event bus for component communication:

```rust
use tokio::sync::{broadcast, mpsc};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DoorcamEvent {
    MotionDetected { 
        contour_area: f64, 
        timestamp: SystemTime 
    },
    FrameReady { 
        frame_id: u64, 
        timestamp: SystemTime 
    },
    TouchDetected { 
        timestamp: SystemTime 
    },
    CaptureStarted { 
        event_id: String 
    },
    CaptureCompleted { 
        event_id: String, 
        file_count: u32 
    },
    SystemError { 
        component: String, 
        error: String 
    },
}

pub struct EventBus {
    sender: broadcast::Sender<DoorcamEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }
    
    pub fn subscribe(&self) -> broadcast::Receiver<DoorcamEvent> {
        self.sender.subscribe()
    }
    
    pub async fn publish(&self, event: DoorcamEvent) -> Result<usize, broadcast::error::SendError<DoorcamEvent>> {
        self.sender.send(event)
    }
}
```

### Ring Buffer (Frame_Buffer)

Lock-free circular buffer for frame management:

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::RwLock;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct FrameData {
    pub id: u64,
    pub timestamp: SystemTime,
    pub data: Arc<Vec<u8>>, // Raw frame data
    pub width: u32,
    pub height: u32,
    pub format: FrameFormat,
}

#[derive(Debug, Clone)]
pub enum FrameFormat {
    Mjpeg,
    Yuyv,
    Rgb24,
}

pub struct RingBuffer {
    frames: Vec<RwLock<Option<FrameData>>>,
    write_index: AtomicUsize,
    capacity: usize,
    preroll_duration: std::time::Duration,
}

impl RingBuffer {
    pub fn new(capacity: usize, preroll_duration: std::time::Duration) -> Self {
        let mut frames = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            frames.push(RwLock::new(None));
        }
        
        Self {
            frames,
            write_index: AtomicUsize::new(0),
            capacity,
            preroll_duration,
        }
    }
    
    pub async fn push_frame(&self, frame: FrameData) {
        let index = self.write_index.fetch_add(1, Ordering::Relaxed) % self.capacity;
        let mut slot = self.frames[index].write().await;
        *slot = Some(frame);
    }
    
    pub async fn get_latest_frame(&self) -> Option<FrameData> {
        let current_index = self.write_index.load(Ordering::Relaxed);
        if current_index == 0 {
            return None;
        }
        
        let index = (current_index - 1) % self.capacity;
        let slot = self.frames[index].read().await;
        slot.clone()
    }
    
    pub async fn get_preroll_frames(&self) -> Vec<FrameData> {
        let now = SystemTime::now();
        let cutoff = now - self.preroll_duration;
        let mut frames = Vec::new();
        
        let current_index = self.write_index.load(Ordering::Relaxed);
        
        // Collect frames within preroll window
        for i in 0..self.capacity {
            let index = (current_index + self.capacity - 1 - i) % self.capacity;
            let slot = self.frames[index].read().await;
            
            if let Some(frame) = slot.as_ref() {
                if frame.timestamp >= cutoff {
                    frames.push(frame.clone());
                } else {
                    break; // Frames are ordered by time
                }
            }
        }
        
        frames.reverse(); // Return in chronological order
        frames
    }
}
```

### Camera Interface

V4L2 camera capture with hardware acceleration:

```rust
use v4l::prelude::*;
use v4l::buffer::Type;
use v4l::io::traits::CaptureStream;
use tokio::time::{interval, Duration};
use std::sync::Arc;

pub struct CameraInterface {
    device: Arc<v4l::Device>,
    config: CameraConfig,
    frame_counter: AtomicU64,
}

impl CameraInterface {
    pub async fn new(config: CameraConfig) -> Result<Self, CameraError> {
        let device_path = format!("/dev/video{}", config.index);
        let device = v4l::Device::new(&device_path)
            .map_err(|e| CameraError::DeviceOpen { 
                device: config.index, 
                source: e.to_string() 
            })?;
        
        // Configure camera
        let mut fmt = device.format()?;
        fmt.width = config.resolution.0;
        fmt.height = config.resolution.1;
        fmt.fourcc = match config.format.as_str() {
            "MJPG" => v4l::FourCC::new(b"MJPG"),
            "YUYV" => v4l::FourCC::new(b"YUYV"),
            _ => return Err(CameraError::UnsupportedFormat(config.format.clone())),
        };
        
        device.set_format(&fmt)?;
        
        // Set frame rate
        let mut params = device.params()?;
        params.interval = v4l::Fraction::new(1, config.max_fps);
        device.set_params(&params)?;
        
        Ok(Self {
            device: Arc::new(device),
            config,
            frame_counter: AtomicU64::new(0),
        })
    }
    
    pub async fn start_capture(
        &self, 
        ring_buffer: Arc<RingBuffer>,
        event_bus: Arc<EventBus>
    ) -> Result<(), CameraError> {
        let device = Arc::clone(&self.device);
        let config = self.config.clone();
        let frame_counter = &self.frame_counter;
        
        tokio::spawn(async move {
            let mut stream = MmapStream::with_buffers(&device, Type::VideoCapture, 4)
                .expect("Failed to create capture stream");
            
            let mut interval = interval(Duration::from_millis(1000 / config.max_fps as u64));
            
            loop {
                interval.tick().await;
                
                match stream.next() {
                    Ok((buffer, _meta)) => {
                        let frame_id = frame_counter.fetch_add(1, Ordering::Relaxed);
                        let timestamp = SystemTime::now();
                        
                        let frame_data = FrameData {
                            id: frame_id,
                            timestamp,
                            data: Arc::new(buffer.to_vec()),
                            width: config.resolution.0,
                            height: config.resolution.1,
                            format: match config.format.as_str() {
                                "MJPG" => FrameFormat::Mjpeg,
                                "YUYV" => FrameFormat::Yuyv,
                                _ => FrameFormat::Rgb24,
                            },
                        };
                        
                        ring_buffer.push_frame(frame_data).await;
                        
                        let _ = event_bus.publish(DoorcamEvent::FrameReady {
                            frame_id,
                            timestamp,
                        }).await;
                    }
                    Err(e) => {
                        tracing::error!("Camera capture error: {}", e);
                        let _ = event_bus.publish(DoorcamEvent::SystemError {
                            component: "camera".to_string(),
                            error: e.to_string(),
                        }).await;
                    }
                }
            }
        });
        
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CameraError {
    #[error("Failed to open camera device {device}: {source}")]
    DeviceOpen { device: u32, source: String },
    
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
    
    #[error("V4L2 error: {0}")]
    V4l2(#[from] v4l::Error),
}
```

### Motion Analyzer

Background subtraction with OpenCV:

```rust
use opencv::{core, imgproc, prelude::*};
use tokio::time::{interval, Duration};
use std::sync::Arc;

pub struct MotionAnalyzer {
    config: AnalyzerConfig,
    background_subtractor: Option<core::Ptr<dyn imgproc::BackgroundSubtractor>>,
}

impl MotionAnalyzer {
    pub async fn new(config: AnalyzerConfig) -> Result<Self, AnalyzerError> {
        let background_subtractor = imgproc::create_background_subtractor_mog2(
            500,    // history
            16.0,   // var_threshold
            false   // detect_shadows
        )?;
        
        Ok(Self {
            config,
            background_subtractor: Some(background_subtractor),
        })
    }
    
    pub async fn start_analysis(
        &mut self,
        ring_buffer: Arc<RingBuffer>,
        event_bus: Arc<EventBus>
    ) -> Result<(), AnalyzerError> {
        let mut interval = interval(Duration::from_millis(1000 / self.config.max_fps as u64));
        
        loop {
            interval.tick().await;
            
            if let Some(frame) = ring_buffer.get_latest_frame().await {
                if let Some(motion_area) = self.detect_motion(&frame).await? {
                    if motion_area > self.config.contour_minimum_area {
                        tracing::info!("Motion detected: area = {:.2}", motion_area);
                        
                        let _ = event_bus.publish(DoorcamEvent::MotionDetected {
                            contour_area: motion_area,
                            timestamp: frame.timestamp,
                        }).await;
                    }
                }
            }
        }
    }
    
    async fn detect_motion(&mut self, frame: &FrameData) -> Result<Option<f64>, AnalyzerError> {
        // Convert frame data to OpenCV Mat
        let mat = match frame.format {
            FrameFormat::Mjpeg => {
                let decoded = opencv::imgcodecs::imdecode(
                    &core::Vector::from_slice(&frame.data),
                    opencv::imgcodecs::IMREAD_COLOR
                )?;
                decoded
            }
            FrameFormat::Yuyv => {
                // Convert YUYV to BGR
                let yuyv_mat = Mat::new_rows_cols_with_data(
                    frame.height as i32,
                    frame.width as i32,
                    core::CV_8UC2,
                    frame.data.as_ptr() as *mut std::ffi::c_void,
                    core::Mat_AUTO_STEP
                )?;
                
                let mut bgr_mat = Mat::default();
                imgproc::cvt_color(&yuyv_mat, &mut bgr_mat, imgproc::COLOR_YUV2BGR_YUYV, 0)?;
                bgr_mat
            }
            _ => return Ok(None),
        };
        
        // Convert to grayscale
        let mut gray = Mat::default();
        imgproc::cvt_color(&mat, &mut gray, imgproc::COLOR_BGR2GRAY, 0)?;
        
        // Apply Gaussian blur
        let mut blurred = Mat::default();
        imgproc::gaussian_blur(
            &gray, 
            &mut blurred, 
            core::Size::new(21, 21), 
            0.0, 
            0.0, 
            core::BORDER_DEFAULT
        )?;
        
        // Background subtraction
        if let Some(ref mut bg_sub) = self.background_subtractor {
            let mut fg_mask = Mat::default();
            bg_sub.apply(&blurred, &mut fg_mask, -1.0)?;
            
            // Find contours
            let mut contours = core::Vector::<core::Vector<core::Point>>::new();
            imgproc::find_contours(
                &fg_mask,
                &mut contours,
                imgproc::RETR_EXTERNAL,
                imgproc::CHAIN_APPROX_SIMPLE,
                core::Point::new(0, 0)
            )?;
            
            // Find largest contour
            let mut max_area = 0.0;
            for i in 0..contours.len() {
                let area = imgproc::contour_area(&contours.get(i)?, false)?;
                if area > max_area {
                    max_area = area;
                }
            }
            
            return Ok(if max_area > 0.0 { Some(max_area) } else { None });
        }
        
        Ok(None)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AnalyzerError {
    #[error("OpenCV error: {0}")]
    OpenCV(#[from] opencv::Error),
}
```

### Stream Server

HTTP/MJPEG streaming with Axum:

```rust
use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use tokio::sync::watch;
use bytes::Bytes;
use std::sync::Arc;

pub struct StreamServer {
    config: StreamConfig,
}

impl StreamServer {
    pub fn new(config: StreamConfig) -> Self {
        Self { config }
    }
    
    pub async fn start(
        &self,
        ring_buffer: Arc<RingBuffer>
    ) -> Result<(), StreamError> {
        let app = Router::new()
            .route("/stream.mjpg", get(mjpeg_stream))
            .with_state(ring_buffer);
        
        let addr = format!("{}:{}", self.config.ip, self.config.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        
        tracing::info!("MJPEG server listening on {}", addr);
        axum::serve(listener, app).await?;
        
        Ok(())
    }
}

async fn mjpeg_stream(
    State(ring_buffer): State<Arc<RingBuffer>>,
) -> impl IntoResponse {
    let stream = async_stream::stream! {
        let mut last_frame_id = 0u64;
        
        loop {
            if let Some(frame) = ring_buffer.get_latest_frame().await {
                if frame.id > last_frame_id {
                    last_frame_id = frame.id;
                    
                    // Ensure frame is JPEG encoded
                    let jpeg_data = match frame.format {
                        FrameFormat::Mjpeg => frame.data.as_ref().clone(),
                        _ => {
                            // Convert to JPEG if needed
                            // This would require OpenCV encoding
                            continue;
                        }
                    };
                    
                    let boundary = format!(
                        "--FRAME\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                        jpeg_data.len()
                    );
                    
                    yield Ok::<_, axum::Error>(Bytes::from(boundary));
                    yield Ok(Bytes::from(jpeg_data));
                    yield Ok(Bytes::from("\r\n"));
                }
            }
            
            tokio::time::sleep(tokio::time::Duration::from_millis(33)).await; // ~30 FPS
        }
    };
    
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "multipart/x-mixed-replace; boundary=FRAME")
        .header(header::CACHE_CONTROL, "no-cache, private")
        .header(header::PRAGMA, "no-cache")
        .body(axum::body::Body::from_stream(stream))
        .unwrap()
}

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("Network error: {0}")]
    Network(#[from] std::io::Error),
    
    #[error("Axum error: {0}")]
    Axum(#[from] axum::Error),
}
```

## Data Models

### Frame Processing Pipeline

```rust
use std::sync::Arc;
use opencv::core::Mat;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct ProcessedFrame {
    pub original: FrameData,
    pub rotated: Option<Arc<Vec<u8>>>,
    pub jpeg_encoded: Option<Arc<Vec<u8>>>,
    pub display_ready: Option<Arc<Vec<u8>>>,
}

impl ProcessedFrame {
    pub async fn from_frame(frame: FrameData, rotation: Option<Rotation>) -> Result<Self, ProcessingError> {
        let mut processed = Self {
            original: frame,
            rotated: None,
            jpeg_encoded: None,
            display_ready: None,
        };
        
        if let Some(rot) = rotation {
            processed.apply_rotation(rot).await?;
        }
        
        Ok(processed)
    }
    
    pub async fn get_jpeg(&mut self) -> Result<Arc<Vec<u8>>, ProcessingError> {
        if let Some(ref jpeg) = self.jpeg_encoded {
            return Ok(Arc::clone(jpeg));
        }
        
        // Encode to JPEG
        let source_data = self.rotated.as_ref().unwrap_or(&self.original.data);
        let jpeg_data = self.encode_jpeg(source_data).await?;
        self.jpeg_encoded = Some(Arc::clone(&jpeg_data));
        
        Ok(jpeg_data)
    }
    
    async fn apply_rotation(&mut self, rotation: Rotation) -> Result<(), ProcessingError> {
        // Rotation implementation using OpenCV
        // This would convert the frame data, apply rotation, and store result
        Ok(())
    }
    
    async fn encode_jpeg(&self, data: &[u8]) -> Result<Arc<Vec<u8>>, ProcessingError> {
        // JPEG encoding implementation
        Ok(Arc::new(Vec::new())) // Placeholder
    }
}
```

## Error Handling

### Comprehensive Error Types

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DoorcamError {
    #[error("Camera error: {0}")]
    Camera(#[from] CameraError),
    
    #[error("Configuration error: {0}")]
    Config(#[from] config::ConfigError),
    
    #[error("Motion analysis error: {0}")]
    Analyzer(#[from] AnalyzerError),
    
    #[error("Stream server error: {0}")]
    Stream(#[from] StreamError),
    
    #[error("Display error: {0}")]
    Display(#[from] DisplayError),
    
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
    
    #[error("Processing error: {0}")]
    Processing(#[from] ProcessingError),
}

#[derive(Error, Debug)]
pub enum ProcessingError {
    #[error("Frame conversion failed: {0}")]
    Conversion(String),
    
    #[error("Encoding failed: {0}")]
    Encoding(String),
    
    #[error("OpenCV error: {0}")]
    OpenCV(#[from] opencv::Error),
}
```

### Error Recovery

```rust
pub struct ErrorRecovery {
    camera_retry_count: u32,
    max_retries: u32,
    backoff_base: Duration,
}

impl ErrorRecovery {
    pub async fn handle_camera_error(&mut self, error: &CameraError) -> RecoveryAction {
        match error {
            CameraError::DeviceOpen { .. } => {
                if self.camera_retry_count < self.max_retries {
                    self.camera_retry_count += 1;
                    let delay = self.backoff_base * 2_u32.pow(self.camera_retry_count);
                    
                    tracing::warn!(
                        "Camera error, retrying in {:?} (attempt {}/{})",
                        delay, self.camera_retry_count, self.max_retries
                    );
                    
                    tokio::time::sleep(delay).await;
                    RecoveryAction::Retry
                } else {
                    tracing::error!("Camera recovery failed after {} attempts", self.max_retries);
                    RecoveryAction::Shutdown
                }
            }
            _ => RecoveryAction::Continue,
        }
    }
}

pub enum RecoveryAction {
    Retry,
    Continue,
    Shutdown,
}
```

## Testing Strategy

### Unit Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio_test;
    
    #[tokio::test]
    async fn test_ring_buffer_operations() {
        let buffer = RingBuffer::new(10, Duration::from_secs(5));
        
        let frame = FrameData {
            id: 1,
            timestamp: SystemTime::now(),
            data: Arc::new(vec![0u8; 1024]),
            width: 640,
            height: 480,
            format: FrameFormat::Mjpeg,
        };
        
        buffer.push_frame(frame.clone()).await;
        
        let latest = buffer.get_latest_frame().await;
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().id, 1);
    }
    
    #[tokio::test]
    async fn test_motion_detection_threshold() {
        let config = AnalyzerConfig {
            max_fps: 5,
            delta_threshold: 25,
            contour_minimum_area: 1000.0,
        };
        
        let mut analyzer = MotionAnalyzer::new(config).await.unwrap();
        
        // Test with synthetic frame data
        // ... test implementation
    }
}
```

### Integration Testing

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_end_to_end_capture_flow() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(temp_dir.path());
        
        let event_bus = Arc::new(EventBus::new(100));
        let ring_buffer = Arc::new(RingBuffer::new(30, Duration::from_secs(5)));
        
        // Simulate motion detection
        event_bus.publish(DoorcamEvent::MotionDetected {
            contour_area: 5000.0,
            timestamp: SystemTime::now(),
        }).await.unwrap();
        
        // Verify capture files are created
        // ... verification logic
    }
}
```

### Display Controller

HyperPixel 4.0 framebuffer rendering with touch support:

```rust
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct DisplayController {
    config: DisplayConfig,
    framebuffer_fd: std::fs::File,
    backlight_fd: std::fs::File,
    is_active: Arc<AtomicBool>,
}

impl DisplayController {
    pub async fn new(config: DisplayConfig) -> Result<Self, DisplayError> {
        let framebuffer_fd = OpenOptions::new()
            .write(true)
            .open(&config.framebuffer_device)?;
            
        let backlight_fd = OpenOptions::new()
            .write(true)
            .open(&config.backlight_device)?;
        
        Ok(Self {
            config,
            framebuffer_fd,
            backlight_fd,
            is_active: Arc::new(AtomicBool::new(false)),
        })
    }
    
    pub async fn start(
        &mut self,
        ring_buffer: Arc<RingBuffer>,
        event_bus: Arc<EventBus>
    ) -> Result<(), DisplayError> {
        let mut event_receiver = event_bus.subscribe();
        let is_active = Arc::clone(&self.is_active);
        
        // Event handling
        tokio::spawn(async move {
            while let Ok(event) = event_receiver.recv().await {
                match event {
                    DoorcamEvent::MotionDetected { .. } | 
                    DoorcamEvent::TouchDetected { .. } => {
                        is_active.store(true, Ordering::Relaxed);
                        
                        // Auto-deactivate after period
                        let is_active_clone = Arc::clone(&is_active);
                        let duration = Duration::from_secs(config.activation_period_seconds as u64);
                        tokio::spawn(async move {
                            tokio::time::sleep(duration).await;
                            is_active_clone.store(false, Ordering::Relaxed);
                        });
                    }
                    _ => {}
                }
            }
        });
        
        Ok(())
    }
    
    async fn render_frame(&mut self, frame: &FrameData) -> Result<(), DisplayError> {
        // Convert frame to RGB565 for framebuffer and apply rotation if needed
        let display_data = self.convert_frame_for_display(frame).await?;
        self.framebuffer_fd.write_all(&display_data)?;
        Ok(())
    }
}
```

### Touch Input Handler

Input device monitoring using evdev:

```rust
use evdev::{Device, EventType, InputEventKind};

pub struct TouchInputHandler {
    device_path: String,
}

impl TouchInputHandler {
    pub fn new(device_path: String) -> Self {
        Self { device_path }
    }
    
    pub async fn start(&self, event_bus: Arc<EventBus>) -> Result<(), TouchError> {
        let device_path = self.device_path.clone();
        
        tokio::spawn(async move {
            let mut device = Device::open(&device_path)
                .expect("Failed to open touch device");
            
            loop {
                match device.fetch_events() {
                    Ok(events) => {
                        for event in events {
                            if matches!(event.kind(), InputEventKind::Key(_)) {
                                let _ = event_bus.publish(DoorcamEvent::TouchDetected {
                                    timestamp: SystemTime::now(),
                                }).await;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Touch input error: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });
        
        Ok(())
    }
}
```

### Video Capture

Event-triggered recording with preroll/postroll:

```rust
pub struct VideoCapture {
    config: CaptureConfig,
    is_recording: Arc<AtomicBool>,
}

impl VideoCapture {
    pub async fn start(
        &self,
        ring_buffer: Arc<RingBuffer>,
        event_bus: Arc<EventBus>
    ) -> Result<(), CaptureError> {
        let mut event_receiver = event_bus.subscribe();
        
        tokio::spawn(async move {
            while let Ok(event) = event_receiver.recv().await {
                if let DoorcamEvent::MotionDetected { timestamp, .. } = event {
                    if !self.is_recording.load(Ordering::Relaxed) {
                        self.start_recording(ring_buffer.clone(), timestamp).await;
                    }
                }
            }
        });
        
        Ok(())
    }
    
    async fn start_recording(&self, ring_buffer: Arc<RingBuffer>, trigger_time: SystemTime) {
        self.is_recording.store(true, Ordering::Relaxed);
        
        // Get preroll frames
        let preroll_frames = ring_buffer.get_preroll_frames().await;
        
        // Create event directory
        let event_id = format!("{:?}", trigger_time);
        let event_dir = std::path::Path::new(&self.config.path).join(&event_id);
        std::fs::create_dir_all(&event_dir).unwrap();
        
        // Save preroll frames
        for (i, frame) in preroll_frames.iter().enumerate() {
            self.save_frame(frame, &event_dir, i).await;
        }
        
        // Continue recording for postroll duration
        let postroll_duration = Duration::from_secs(self.config.postroll_seconds as u64);
        let end_time = SystemTime::now() + postroll_duration;
        
        while SystemTime::now() < end_time {
            if let Some(frame) = ring_buffer.get_latest_frame().await {
                self.save_frame(&frame, &event_dir, 0).await;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        self.is_recording.store(false, Ordering::Relaxed);
        
        // Optionally encode to video
        if self.config.video_encoding {
            self.encode_to_video(&event_dir).await;
        }
    }
    
    async fn save_frame(&self, frame: &FrameData, dir: &std::path::Path, index: usize) {
        let filename = format!("{:010}.jpg", index);
        let filepath = dir.join(filename);
        
        // Save JPEG data to file
        if let FrameFormat::Mjpeg = frame.format {
            std::fs::write(filepath, &*frame.data).unwrap();
        }
    }
}
```

This design provides a robust, performant foundation for the Rust doorcam rewrite with clear separation of concerns, efficient frame management through the ring buffer architecture, and comprehensive error handling.