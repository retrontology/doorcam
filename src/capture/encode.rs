use crate::{
    config::CaptureConfig,
    error::{DoorcamError, Result},
};
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::Stdio;
use std::time::SystemTime;
use tokio::sync::mpsc;
#[cfg(target_os = "linux")]
use tokio::{io::AsyncWriteExt, process::Command, time::sleep};
use tracing::{debug, info, warn};

use super::overlay::prepare_jpeg;
use super::overlay::resolve_timestamp_timezone;

/// Video generation job for the background queue
pub(crate) struct VideoGenerationJob {
    pub(crate) event_id: String,
    pub(crate) capture_dir: PathBuf,
    pub(crate) wal_path: PathBuf,
    pub(crate) frame_count: u32,
}

/// Background worker that processes video generation jobs
pub(crate) async fn video_generation_worker(
    mut queue_rx: mpsc::UnboundedReceiver<VideoGenerationJob>,
    config: CaptureConfig,
) {
    info!("Video generation worker started");

    while let Some(job) = queue_rx.recv().await {
        let event_id = job.event_id.clone();
        let frame_count = job.frame_count;

        info!(
            "Processing video generation job for capture {} ({} frames)",
            event_id, frame_count
        );

        if let Err(e) = generate_video_from_wal(job, &config).await {
            tracing::error!("Failed to generate video for capture {}: {}", event_id, e);
        } else {
            info!("Video generation completed for capture {}", event_id);
        }
    }

    info!("Video generation worker stopped");
}

/// Generate video from WAL file (called by background worker)
pub(crate) async fn generate_video_from_wal(
    job: VideoGenerationJob,
    config: &CaptureConfig,
) -> Result<()> {
    let video_filename = format!("{}.mp4", job.event_id);
    let video_path = PathBuf::from(&config.path).join(video_filename);

    info!("Generating video file from WAL: {}", video_path.display());

    let mut reader = crate::wal::WalFrameReader::open(job.wal_path.clone()).await?;
    let wal_frame_count = reader.frame_count();

    info!("Read {} frames from WAL for encoding", wal_frame_count);

    #[cfg(target_os = "linux")]
    {
        create_video_ffmpeg_from_wal(&mut reader, &job.capture_dir, &video_path, config).await?;
    }

    #[cfg(not(target_os = "linux"))]
    {
        return Err(DoorcamError::component(
            "video_capture",
            "Video encoding not available on this platform",
        ));
    }

    if let Err(e) = crate::wal::delete_wal(&job.wal_path).await {
        warn!("Failed to delete WAL file: {}", e);
    }

    info!("Video file created: {}", video_path.display());
    Ok(())
}

/// Create video using FFmpeg from a WAL stream (streaming JPEGs over stdin).
#[cfg(target_os = "linux")]
pub(crate) async fn create_video_ffmpeg_from_wal(
    reader: &mut crate::wal::WalFrameReader,
    capture_dir: &Path,
    video_path: &Path,
    config: &CaptureConfig,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use libc::{setpriority, PRIO_PROCESS};
        let _ = unsafe { setpriority(PRIO_PROCESS as u32, 0, 10) };
    }

    info!("Starting FFmpeg encode to {}", video_path.display());

    let mut frames_dir = None;
    let mut timezone = None;
    if config.keep_images {
        let dir = capture_dir.join("frames");
        tokio::fs::create_dir_all(&dir).await.map_err(|e| {
            DoorcamError::component(
                "video_capture",
                &format!("Failed to create frames directory: {}", e),
            )
        })?;
        frames_dir = Some(dir);
        timezone = Some(resolve_timestamp_timezone(&config.timestamp_timezone));
        info!(
            "[ffmpeg] Extracting JPEG files to {}",
            capture_dir.join("frames").display()
        );
    }

    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "mjpeg",
        "-use_wallclock_as_timestamps",
        "1",
        "-i",
        "pipe:0",
        "-vsync",
        "vfr",
        "-c:v",
        "h264_v4l2m2m",
        "-b:v",
        "10M",
        "-maxrate",
        "10M",
        "-bufsize",
        "10M",
        "-g",
        "60",
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
    ])
    .arg(video_path)
    .stdin(Stdio::piped())
    .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        DoorcamError::component("video_capture", &format!("Failed to start FFmpeg: {}", e))
    })?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| DoorcamError::component("video_capture", "Failed to open FFmpeg stdin"))?;

    let mut frame_index: u32 = 0;
    let mut prev_timestamp: Option<SystemTime> = None;

    while let Some(frame) = reader.next_frame().await? {
        let jpeg_data = prepare_jpeg(&frame, config).await.map_err(|e| {
            DoorcamError::component(
                "video_capture",
                &format!("[ffmpeg] Frame processing failed: {}", e),
            )
        })?;

        if let (Some(dir), Some(tz)) = (&frames_dir, &timezone) {
            write_frame_jpeg(&jpeg_data, frame.timestamp, dir, tz).await?;
        }

        if let Some(prev) = prev_timestamp {
            if let Ok(delta) = frame.timestamp.duration_since(prev) {
                if !delta.is_zero() {
                    sleep(delta).await;
                }
            }
        }

        stdin.write_all(&jpeg_data).await.map_err(|e| {
            DoorcamError::component(
                "video_capture",
                &format!("[ffmpeg] Failed to write frame to stdin: {}", e),
            )
        })?;

        if frame_index % 30 == 0 && frame_index > 0 {
            debug!("[ffmpeg] Encoded {} frames", frame_index);
        }
        frame_index += 1;
        prev_timestamp = Some(frame.timestamp);
    }

    stdin.shutdown().await.map_err(|e| {
        DoorcamError::component(
            "video_capture",
            &format!("[ffmpeg] Failed to close stdin: {}", e),
        )
    })?;
    drop(stdin);

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| DoorcamError::component("video_capture", &format!("FFmpeg failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DoorcamError::component(
            "video_capture",
            &format!("[ffmpeg] Encoding failed: {}", stderr.trim()),
        ));
    }

    info!(
        "[ffmpeg] Video encoding completed: {} frames -> {}",
        frame_index,
        video_path.display()
    );

    Ok(())
}

#[cfg(target_os = "linux")]
async fn write_frame_jpeg(
    jpeg_data: &[u8],
    timestamp: SystemTime,
    frames_dir: &Path,
    timezone: &chrono_tz::Tz,
) -> Result<()> {
    let timestamp = chrono::DateTime::<chrono::Utc>::from(timestamp).with_timezone(timezone);
    let filename = format!("{}.jpg", timestamp.format("%Y%m%d_%H%M%S_%3f"));
    let file_path = frames_dir.join(&filename);

    tokio::fs::write(&file_path, jpeg_data).await.map_err(|e| {
        DoorcamError::component(
            "video_capture",
            &format!("Failed to write JPEG file: {}", e),
        )
    })?;

    Ok(())
}
