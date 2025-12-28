use crate::events::DoorcamEvent;
use crate::frame::FrameData;
use crate::streaming::prep::{prepare_frame_for_streaming, rotation_to_degrees};
use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
};
use bytes::Bytes;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{debug, error, info, trace};

use super::server::ServerState;

/// Handler for MJPEG streaming endpoint
pub async fn mjpeg_stream_handler(State(state): State<ServerState>) -> impl IntoResponse {
    info!("New MJPEG stream client connected");

    let stream = async_stream::stream! {
        let mut last_frame_id = 0u64;
        let mut frame_interval = interval(state.target_frame_interval);
        frame_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut frames_streamed = 0u64;
        let mut bytes_streamed = 0u64;
        let stream_start = std::time::Instant::now();
        let mut last_frame: Option<FrameData> = None;

        loop {
            frame_interval.tick().await;

            match state.ring_buffer.get_latest_frame().await {
                Some(frame) => {
                    if frame.id > last_frame_id {
                        last_frame_id = frame.id;
                    }
                    last_frame = Some(frame);
                }
                None => {
                    trace!("No frames available for streaming");
                }
            }

            if let Some(frame) = last_frame.as_ref() {
                match prepare_frame_for_streaming(frame).await {
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
                    }
                    Err(e) => {
                        error!("Failed to prepare frame {} for streaming: {}", frame.id, e);

                        let _ = state.event_bus.publish(DoorcamEvent::SystemError {
                            component: "stream_server".to_string(),
                            error: format!("Frame preparation failed: {}", e),
                        }).await;
                    }
                }
            }

            if frames_streamed > 0 && frames_streamed.is_multiple_of(100) {
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
        .header(
            header::CONTENT_TYPE,
            "multipart/x-mixed-replace; boundary=FRAME",
        )
        .header(header::CACHE_CONTROL, "no-cache, private")
        .header(header::PRAGMA, "no-cache")
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Methods", "GET")
        .body(axum::body::Body::from_stream(stream))
        .unwrap()
}

/// Handler for health check endpoint
pub async fn health_handler(State(state): State<ServerState>) -> impl IntoResponse {
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

/// Simple HTML page for viewing the MJPEG stream with optional CSS rotation
pub async fn stream_page_handler(State(state): State<ServerState>) -> impl IntoResponse {
    let rotation_deg = rotation_to_degrees(state.stream_rotation);

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Doorcam Stream</title>
    <style>
        :root {{ color-scheme: dark; }}
        body {{
            margin: 0;
            background: #000;
            display: flex;
            align-items: center;
            justify-content: center;
            min-height: 100vh;
        }}
        img.stream {{
            display: block;
            max-width: 100vw;
            max-height: 100vh;
            width: auto;
            height: auto;
            object-fit: contain;
            transform: rotate({rotation}deg);
            transform-origin: center center;
            background: #000;
        }}
    </style>
</head>
<body>
    <img class="stream" src="/stream.mjpg" alt="Doorcam stream">
</body>
</html>
"#,
        rotation = rotation_deg,
    );

    Html(html)
}
