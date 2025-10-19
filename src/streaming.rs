use crate::{
    config::StreamConfig,
    events::{DoorcamEvent, EventBus},
    frame::{FrameData, FrameFormat},
    ring_buffer::RingBuffer,
    error::{DoorcamError, Result, StreamError},
};
use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use bytes::Bytes;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, trace, warn};

/// MJPEG streaming server that serves camera frames over HTTP
pub struct StreamServer {
    config: StreamConfig,
    ring_buffer: Arc<RingBuffer>,
    event_bus: Arc<EventBus>,
}

/// Shared state for the Axum server
#[derive(Clone)]
struct ServerState {
    ring_buffer: Arc<RingBuffer>,
    event_bus: Arc<EventBus>,
}

impl StreamServer {
    /// Create a new streaming server
    pub fn new(
        config: StreamConfig,
        ring_buffer: Arc<RingBuffer>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            config,
            ring_buffer,
            event_bus,
        }
    }

    /// Start the HTTP server and begin serving MJPEG streams
    pub async fn start(&self) -> Result<()> {
        let state = ServerState {
            ring_buffer: Arc::clone(&self.ring_buffer),
            event_bus: Arc::clone(&self.event_bus),
        };

        let app = Router::new()
            .route("/stream.mjpg", get(mjpeg_stream_handler))
            .route("/health", get(health_handler))
            .with_state(state);

        let addr = format!("{}:{}", self.config.ip, self.config.port);
        
        info!("Starting MJPEG streaming server on {}", addr);
        
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| StreamError::BindFailed { 
                address: addr.clone(), 
                source: e 
            })?;

        info!("MJPEG server listening on {}", addr);

        // Publish server start event
        let _ = self.event_bus.publish(DoorcamEvent::SystemError {
            component: "stream_server".to_string(),
            error: format!("Server started on {}", addr),
        }).await;

        axum::serve(listener, app)
            .await
            .map_err(|e| StreamError::StartupFailed { 
                details: format!("Server error: {}", e) 
            })?;

        Ok(())
    }
}

/// Handler for MJPEG streaming endpoint
async fn mjpeg_stream_handler(
    State(state): State<ServerState>,
) -> impl IntoResponse {
    info!("New MJPEG stream client connected");

    let stream = async_stream::stream! {
        let mut last_frame_id = 0u64;
        let mut frame_interval = interval(Duration::from_millis(33)); // ~30 FPS default
        let mut frames_streamed = 0u64;
        let mut bytes_streamed = 0u64;
        let stream_start = std::time::Instant::now();

        // Adaptive frame rate based on available frames
        let mut adaptive_interval = Duration::from_millis(33);
        let mut last_frame_time = std::time::Instant::now();

        loop {
            frame_interval.tick().await;

            match state.ring_buffer.get_latest_frame().await {
                Some(frame) => {
                    // Only send new frames to avoid duplicates
                    if frame.id > last_frame_id {
                        last_frame_id = frame.id;

                        // Adaptive frame rate: adjust based on frame availability
                        let now = std::time::Instant::now();
                        let time_since_last = now.duration_since(last_frame_time);
                        
                        // If frames are coming faster than our interval, speed up
                        // If frames are coming slower, slow down to match
                        if time_since_last < adaptive_interval {
                            adaptive_interval = std::cmp::max(
                                Duration::from_millis(16), // Min 60 FPS
                                time_since_last
                            );
                        } else if time_since_last > adaptive_interval * 2 {
                            adaptive_interval = std::cmp::min(
                                Duration::from_millis(100), // Max 10 FPS
                                time_since_last
                            );
                        }
                        
                        last_frame_time = now;

                        match prepare_frame_for_streaming(&frame).await {
                            Ok(jpeg_data) => {
                                let frame_size = jpeg_data.len();
                                frames_streamed += 1;
                                bytes_streamed += frame_size as u64;

                                debug!(
                                    "Streaming frame {} ({} bytes, {} total frames, {:.1} MB total)",
                                    frame.id, 
                                    frame_size,
                                    frames_streamed,
                                    bytes_streamed as f64 / 1_048_576.0
                                );

                                // Send multipart boundary and headers
                                let boundary = format!(
                                    "--FRAME\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nX-Frame-ID: {}\r\nX-Timestamp: {}\r\n\r\n",
                                    frame_size,
                                    frame.id,
                                    frame.timestamp.duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis()
                                );

                                yield Ok::<_, axum::Error>(Bytes::from(boundary));
                                yield Ok(Bytes::from(jpeg_data));
                                yield Ok(Bytes::from("\r\n"));

                                // Update frame interval for next iteration
                                frame_interval = interval(adaptive_interval);
                            }
                            Err(e) => {
                                error!("Failed to prepare frame {} for streaming: {}", frame.id, e);
                                
                                // Publish error event
                                let _ = state.event_bus.publish(DoorcamEvent::SystemError {
                                    component: "stream_server".to_string(),
                                    error: format!("Frame preparation failed: {}", e),
                                }).await;
                                
                                // Continue with next frame instead of breaking the stream
                            }
                        }
                    } else {
                        // No new frame available, wait a bit longer
                        trace!("No new frame available (last: {})", last_frame_id);
                    }
                }
                None => {
                    // No frames available yet, continue waiting
                    trace!("No frames available for streaming");
                    
                    // Slow down when no frames are available
                    adaptive_interval = Duration::from_millis(100);
                    frame_interval = interval(adaptive_interval);
                }
            }

            // Log streaming statistics periodically
            if frames_streamed > 0 && frames_streamed % 100 == 0 {
                let elapsed = stream_start.elapsed();
                let fps = frames_streamed as f64 / elapsed.as_secs_f64();
                let mbps = (bytes_streamed as f64 / elapsed.as_secs_f64()) / 1_048_576.0;
                
                info!(
                    "Streaming stats: {} frames, {:.1} FPS, {:.2} MB/s, {} total MB",
                    frames_streamed,
                    fps,
                    mbps,
                    bytes_streamed as f64 / 1_048_576.0
                );
            }
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "multipart/x-mixed-replace; boundary=FRAME")
        .header(header::CACHE_CONTROL, "no-cache, private")
        .header(header::PRAGMA, "no-cache")
        .header(header::CONNECTION, "close")
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Methods", "GET")
        .body(axum::body::Body::from_stream(stream))
        .unwrap()
}

/// Handler for health check endpoint
async fn health_handler(
    State(state): State<ServerState>,
) -> impl IntoResponse {
    let latest_frame = state.ring_buffer.get_latest_frame().await;
    let stats = state.ring_buffer.stats();
    
    let health_info = serde_json::json!({
        "status": "healthy",
        "frames_available": latest_frame.is_some(),
        "latest_frame_id": latest_frame.map(|f| f.id),
        "buffer_stats": {
            "frames_pushed": stats.frames_pushed,
            "frames_retrieved": stats.frames_retrieved,
            "utilization_percent": stats.utilization_percent,
        },
        "server_info": {
            "subscribers": state.event_bus.subscriber_count(),
        }
    });

    (StatusCode::OK, axum::Json(health_info))
}

/// Prepare a frame for streaming by ensuring it's in JPEG format
async fn prepare_frame_for_streaming(frame: &FrameData) -> Result<Vec<u8>> {
    match frame.format {
        FrameFormat::Mjpeg => {
            // Already in JPEG format, return as-is
            debug!("Frame {} already in MJPEG format, using directly", frame.id);
            Ok(frame.data.as_ref().clone())
        }
        FrameFormat::Yuyv => {
            // Convert YUYV to JPEG
            debug!("Converting YUYV frame {} to JPEG for streaming", frame.id);
            encode_yuyv_to_jpeg(frame).await
        }
        FrameFormat::Rgb24 => {
            // Convert RGB24 to JPEG
            debug!("Converting RGB24 frame {} to JPEG for streaming", frame.id);
            encode_rgb24_to_jpeg(frame).await
        }
    }
}

/// Encode YUYV frame data to JPEG format
async fn encode_yuyv_to_jpeg(frame: &FrameData) -> Result<Vec<u8>> {
    // For now, this is a placeholder implementation
    // TODO: Replace with actual JPEG encoding using OpenCV in later tasks
    warn!(
        "YUYV to JPEG encoding for frame {} not yet implemented - using placeholder",
        frame.id
    );
    
    create_placeholder_jpeg(frame.width, frame.height, "YUYV")
}

/// Encode RGB24 frame data to JPEG format
async fn encode_rgb24_to_jpeg(frame: &FrameData) -> Result<Vec<u8>> {
    // For now, this is a placeholder implementation
    // TODO: Replace with actual JPEG encoding using OpenCV in later tasks
    warn!(
        "RGB24 to JPEG encoding for frame {} not yet implemented - using placeholder",
        frame.id
    );
    
    create_placeholder_jpeg(frame.width, frame.height, "RGB24")
}

/// Create a placeholder JPEG for non-MJPEG formats
/// This is a temporary implementation until OpenCV integration is added
fn create_placeholder_jpeg(width: u32, height: u32, source_format: &str) -> Result<Vec<u8>> {
    // Create a more comprehensive JPEG header with proper dimensions
    let mut jpeg_data = Vec::new();
    
    // SOI (Start of Image)
    jpeg_data.extend_from_slice(&[0xFF, 0xD8]);
    
    // APP0 (JFIF header)
    jpeg_data.extend_from_slice(&[0xFF, 0xE0]);
    jpeg_data.extend_from_slice(&[0x00, 0x10]); // Length
    jpeg_data.extend_from_slice(b"JFIF\0"); // Identifier
    jpeg_data.extend_from_slice(&[0x01, 0x01]); // Version 1.1
    jpeg_data.extend_from_slice(&[0x01]); // Units (1 = pixels per inch)
    jpeg_data.extend_from_slice(&[0x00, 0x48]); // X density (72)
    jpeg_data.extend_from_slice(&[0x00, 0x48]); // Y density (72)
    jpeg_data.extend_from_slice(&[0x00, 0x00]); // Thumbnail width/height (0 = no thumbnail)
    
    // SOF0 (Start of Frame - Baseline DCT)
    jpeg_data.extend_from_slice(&[0xFF, 0xC0]);
    jpeg_data.extend_from_slice(&[0x00, 0x11]); // Length
    jpeg_data.extend_from_slice(&[0x08]); // Precision (8 bits)
    jpeg_data.extend_from_slice(&[(height >> 8) as u8, height as u8]); // Height
    jpeg_data.extend_from_slice(&[(width >> 8) as u8, width as u8]); // Width
    jpeg_data.extend_from_slice(&[0x03]); // Number of components (3 for YCbCr)
    // Component 1 (Y)
    jpeg_data.extend_from_slice(&[0x01, 0x22, 0x00]);
    // Component 2 (Cb)
    jpeg_data.extend_from_slice(&[0x02, 0x11, 0x01]);
    // Component 3 (Cr)
    jpeg_data.extend_from_slice(&[0x03, 0x11, 0x01]);
    
    // DHT (Define Huffman Table) - simplified
    jpeg_data.extend_from_slice(&[0xFF, 0xC4]);
    jpeg_data.extend_from_slice(&[0x00, 0x1F]); // Length
    jpeg_data.extend_from_slice(&[0x00]); // Table class and destination
    // Simplified Huffman table (16 bytes + symbols)
    jpeg_data.extend_from_slice(&[0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01]);
    jpeg_data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    jpeg_data.extend_from_slice(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]);
    jpeg_data.extend_from_slice(&[0x08, 0x09, 0x0A, 0x0B]);
    
    // SOS (Start of Scan)
    jpeg_data.extend_from_slice(&[0xFF, 0xDA]);
    jpeg_data.extend_from_slice(&[0x00, 0x0C]); // Length
    jpeg_data.extend_from_slice(&[0x03]); // Number of components
    jpeg_data.extend_from_slice(&[0x01, 0x00]); // Component 1
    jpeg_data.extend_from_slice(&[0x02, 0x11]); // Component 2
    jpeg_data.extend_from_slice(&[0x03, 0x11]); // Component 3
    jpeg_data.extend_from_slice(&[0x00, 0x3F, 0x00]); // Spectral selection
    
    // Minimal scan data (black image)
    jpeg_data.extend_from_slice(&[0xFF, 0x00]); // Minimal entropy-coded data
    
    // EOI (End of Image)
    jpeg_data.extend_from_slice(&[0xFF, 0xD9]);
    
    debug!(
        "Created placeholder JPEG for {}x{} frame from {} format ({} bytes)",
        width, height, source_format, jpeg_data.len()
    );
    
    Ok(jpeg_data)
}

/// Stream server builder for configuration
pub struct StreamServerBuilder {
    config: Option<StreamConfig>,
    ring_buffer: Option<Arc<RingBuffer>>,
    event_bus: Option<Arc<EventBus>>,
}

impl StreamServerBuilder {
    /// Create a new stream server builder
    pub fn new() -> Self {
        Self {
            config: None,
            ring_buffer: None,
            event_bus: None,
        }
    }

    /// Set the stream configuration
    pub fn config(mut self, config: StreamConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set the ring buffer
    pub fn ring_buffer(mut self, ring_buffer: Arc<RingBuffer>) -> Self {
        self.ring_buffer = Some(ring_buffer);
        self
    }

    /// Set the event bus
    pub fn event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Build the stream server
    pub fn build(self) -> Result<StreamServer> {
        let config = self.config.ok_or_else(|| {
            DoorcamError::Stream(StreamError::StartupFailed { 
                details: "Stream configuration is required".to_string() 
            })
        })?;

        let ring_buffer = self.ring_buffer.ok_or_else(|| {
            DoorcamError::Stream(StreamError::StartupFailed { 
                details: "Ring buffer is required".to_string() 
            })
        })?;

        let event_bus = self.event_bus.ok_or_else(|| {
            DoorcamError::Stream(StreamError::StartupFailed { 
                details: "Event bus is required".to_string() 
            })
        })?;

        Ok(StreamServer::new(config, ring_buffer, event_bus))
    }
}

impl Default for StreamServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Stream server statistics and monitoring
#[derive(Debug, Clone)]
pub struct StreamStats {
    pub active_connections: u32,
    pub total_connections: u64,
    pub frames_streamed: u64,
    pub bytes_streamed: u64,
    pub errors: u64,
}

impl Default for StreamStats {
    fn default() -> Self {
        Self {
            active_connections: 0,
            total_connections: 0,
            frames_streamed: 0,
            bytes_streamed: 0,
            errors: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::StreamConfig,
        events::EventBus,
        frame::{FrameData, FrameFormat},
        ring_buffer::RingBuffer,
    };
    use std::time::{Duration, SystemTime};
    use tokio::time::timeout;

    fn create_test_frame(id: u64, format: FrameFormat) -> FrameData {
        let data = match format {
            FrameFormat::Mjpeg => {
                // Minimal JPEG data
                vec![0xFF, 0xD8, 0xFF, 0xD9] // SOI + EOI
            }
            FrameFormat::Yuyv => {
                vec![0u8; 640 * 480 * 2] // YUYV data
            }
            FrameFormat::Rgb24 => {
                vec![0u8; 640 * 480 * 3] // RGB24 data
            }
        };

        FrameData::new(id, SystemTime::now(), data, 640, 480, format)
    }

    #[tokio::test]
    async fn test_stream_server_builder() {
        let config = StreamConfig {
            ip: "127.0.0.1".to_string(),
            port: 8080,
        };
        let ring_buffer = Arc::new(RingBuffer::new(10, Duration::from_secs(1)));
        let event_bus = Arc::new(EventBus::new(10));

        let server = StreamServerBuilder::new()
            .config(config)
            .ring_buffer(ring_buffer)
            .event_bus(event_bus)
            .build()
            .unwrap();

        assert_eq!(server.config.ip, "127.0.0.1");
        assert_eq!(server.config.port, 8080);
    }

    #[tokio::test]
    async fn test_prepare_frame_for_streaming_mjpeg() {
        let frame = create_test_frame(1, FrameFormat::Mjpeg);
        let result = prepare_frame_for_streaming(&frame).await.unwrap();
        
        // Should return the original JPEG data
        assert_eq!(result, frame.data.as_ref().clone());
    }

    #[tokio::test]
    async fn test_prepare_frame_for_streaming_yuyv() {
        let frame = create_test_frame(1, FrameFormat::Yuyv);
        let result = prepare_frame_for_streaming(&frame).await.unwrap();
        
        // Should return placeholder JPEG (starts with JPEG SOI marker)
        assert!(result.len() > 0);
        assert_eq!(result[0], 0xFF);
        assert_eq!(result[1], 0xD8);
        // Should end with EOI marker
        let len = result.len();
        assert_eq!(result[len - 2], 0xFF);
        assert_eq!(result[len - 1], 0xD9);
    }

    #[tokio::test]
    async fn test_prepare_frame_for_streaming_rgb24() {
        let frame = create_test_frame(1, FrameFormat::Rgb24);
        let result = prepare_frame_for_streaming(&frame).await.unwrap();
        
        // Should return placeholder JPEG (starts with JPEG SOI marker)
        assert!(result.len() > 0);
        assert_eq!(result[0], 0xFF);
        assert_eq!(result[1], 0xD8);
        // Should end with EOI marker
        let len = result.len();
        assert_eq!(result[len - 2], 0xFF);
        assert_eq!(result[len - 1], 0xD9);
    }

    #[tokio::test]
    async fn test_create_placeholder_jpeg() {
        let jpeg = create_placeholder_jpeg(640, 480, "TEST").unwrap();
        
        // Should start with JPEG SOI marker
        assert_eq!(jpeg[0], 0xFF);
        assert_eq!(jpeg[1], 0xD8);
        
        // Should end with JPEG EOI marker
        let len = jpeg.len();
        assert_eq!(jpeg[len - 2], 0xFF);
        assert_eq!(jpeg[len - 1], 0xD9);
        
        // Should be a reasonable size for a JPEG header
        assert!(jpeg.len() > 50);
    }

    #[tokio::test]
    async fn test_builder_validation() {
        // Missing config
        let result = StreamServerBuilder::new()
            .ring_buffer(Arc::new(RingBuffer::new(10, Duration::from_secs(1))))
            .event_bus(Arc::new(EventBus::new(10)))
            .build();
        assert!(result.is_err());

        // Missing ring buffer
        let result = StreamServerBuilder::new()
            .config(StreamConfig {
                ip: "127.0.0.1".to_string(),
                port: 8080,
            })
            .event_bus(Arc::new(EventBus::new(10)))
            .build();
        assert!(result.is_err());

        // Missing event bus
        let result = StreamServerBuilder::new()
            .config(StreamConfig {
                ip: "127.0.0.1".to_string(),
                port: 8080,
            })
            .ring_buffer(Arc::new(RingBuffer::new(10, Duration::from_secs(1))))
            .build();
        assert!(result.is_err());
    }

    // Integration test for the streaming functionality
    #[tokio::test]
    async fn test_streaming_integration() {
        let ring_buffer = Arc::new(RingBuffer::new(10, Duration::from_secs(1)));
        let event_bus = Arc::new(EventBus::new(10));

        // Add a test frame to the ring buffer
        let frame = create_test_frame(1, FrameFormat::Mjpeg);
        ring_buffer.push_frame(frame).await;

        // Verify frame is available
        let latest = ring_buffer.get_latest_frame().await;
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().id, 1);

        // Test frame preparation
        let latest_frame = ring_buffer.get_latest_frame().await.unwrap();
        let jpeg_data = prepare_frame_for_streaming(&latest_frame).await.unwrap();
        assert!(jpeg_data.len() > 0);
    }
}