use crate::{
    config::CaptureConfig,
    error::{DoorcamError, Result},
};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::overlay::prepare_jpeg;
use super::overlay::resolve_timestamp_timezone;

#[cfg(target_os = "linux")]
use gstreamer::prelude::*;
#[cfg(target_os = "linux")]
use gstreamer::Pipeline;
#[cfg(target_os = "linux")]
use gstreamer_app::AppSrc;

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
        create_video_gstreamer_from_wal(&mut reader, &job.capture_dir, &video_path, config).await?;
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

/// Create video using GStreamer from a WAL stream
#[cfg(target_os = "linux")]
pub(crate) async fn create_video_gstreamer_from_wal(
    reader: &mut crate::wal::WalFrameReader,
    capture_dir: &Path,
    video_path: &Path,
    config: &CaptureConfig,
) -> Result<()> {
    gstreamer::init().map_err(|e| {
        DoorcamError::component(
            "video_capture",
            &format!("Failed to initialize GStreamer: {}", e),
        )
    })?;

    let sw_pipeline_desc = format!(
        "appsrc name=src format=time is-live=false do-timestamp=true caps=image/jpeg,framerate=30/1 ! \
         jpegparse ! \
         jpegdec ! \
         videoconvert ! video/x-raw,format=I420 ! \
         x264enc speed-preset=medium bitrate=10000 key-int-max=60 ! \
         video/x-h264,stream-format=byte-stream,alignment=au,profile=high ! \
         h264parse config-interval=1 ! \
         mp4mux faststart=true ! \
         filesink location={}",
        video_path.to_string_lossy()
    );

    encode_wal_with_pipeline(
        "software",
        &sw_pipeline_desc,
        reader,
        capture_dir,
        video_path,
        config,
    )
    .await?;

    Ok(())
}

/// Encode frames using a provided GStreamer pipeline description (software path).
#[cfg(target_os = "linux")]
pub(crate) async fn encode_wal_with_pipeline(
    label: &str,
    pipeline_desc: &str,
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

    info!("Creating GStreamer video pipeline ({})", label);
    debug!("Pipeline ({}): {}", label, pipeline_desc);

    let pipeline = gstreamer::parse::launch(pipeline_desc)
        .map_err(|e| {
            DoorcamError::component(
                "video_capture",
                &format!("[{}] Failed to create pipeline: {}", label, e),
            )
        })?
        .downcast::<Pipeline>()
        .map_err(|_| {
            DoorcamError::component(
                "video_capture",
                &format!("[{}] Failed to downcast to Pipeline", label),
            )
        })?;

    let appsrc = pipeline
        .by_name("src")
        .ok_or_else(|| {
            DoorcamError::component(
                "video_capture",
                &format!("[{}] Failed to get appsrc element", label),
            )
        })?
        .downcast::<AppSrc>()
        .map_err(|_| {
            DoorcamError::component(
                "video_capture",
                &format!("[{}] Failed to downcast to AppSrc", label),
            )
        })?;

    appsrc.set_property("format", gstreamer::Format::Time);
    appsrc.set_property("is-live", false);

    pipeline.set_state(gstreamer::State::Playing).map_err(|e| {
        DoorcamError::component(
            "video_capture",
            &format!("[{}] Failed to start pipeline: {}", label, e),
        )
    })?;

    info!("Started GStreamer encoding pipeline ({})", label);
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
            "[{}] Extracting JPEG files to {}",
            label,
            capture_dir.join("frames").display()
        );
    }

    let mut frame_index: u32 = 0;
    let mut prev_frame = reader.next_frame().await?;

    info!(
        "[{}] Encoding frames from WAL {} to {}",
        label,
        reader.path().display(),
        video_path.display()
    );

    if let Some(mut current_frame) = prev_frame.take() {
        let base_time = current_frame
            .timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_nanos() as u64;

        loop {
            let next_frame = reader.next_frame().await?;
            let next_duration = if let Some(next) = &next_frame {
                let next_ns = next
                    .timestamp
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_nanos() as u64;
                let frame_ns = current_frame
                    .timestamp
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_nanos() as u64;
                next_ns.saturating_sub(frame_ns)
            } else {
                1_000_000_000 / 30
            };

            let jpeg_data = prepare_jpeg(&current_frame, config).await.map_err(|e| {
                DoorcamError::component(
                    "video_capture",
                    &format!("[{}] Frame processing failed: {}", label, e),
                )
            })?;

            if let (Some(dir), Some(tz)) = (&frames_dir, &timezone) {
                write_frame_jpeg(&jpeg_data, current_frame.timestamp, dir, tz).await?;
            }

            let mut buffer = gstreamer::Buffer::with_size(jpeg_data.len()).map_err(|e| {
                DoorcamError::component(
                    "video_capture",
                    &format!("[{}] Failed to create buffer: {}", label, e),
                )
            })?;

            {
                let buffer_ref = buffer.get_mut().unwrap();
                let mut map = buffer_ref.map_writable().map_err(|e| {
                    DoorcamError::component(
                        "video_capture",
                        &format!("[{}] Failed to map buffer: {}", label, e),
                    )
                })?;
                map.copy_from_slice(&jpeg_data);
            }

            let frame_ns = current_frame
                .timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_nanos() as u64;
            let relative_ns = frame_ns.saturating_sub(base_time);

            buffer
                .get_mut()
                .unwrap()
                .set_pts(gstreamer::ClockTime::from_nseconds(relative_ns));
            buffer
                .get_mut()
                .unwrap()
                .set_duration(gstreamer::ClockTime::from_nseconds(next_duration));

            appsrc.push_buffer(buffer).map_err(|e| {
                DoorcamError::component(
                    "video_capture",
                    &format!("[{}] Failed to push buffer: {:?}", label, e),
                )
            })?;

            if frame_index % 30 == 0 && frame_index > 0 {
                debug!("[{}] Encoded {} frames", label, frame_index);
            }
            frame_index += 1;

            match next_frame {
                Some(next) => current_frame = next,
                None => break,
            }
        }
    }

    appsrc.end_of_stream().map_err(|e| {
        DoorcamError::component(
            "video_capture",
            &format!("[{}] Failed to signal EOS: {:?}", label, e),
        )
    })?;

    let bus = pipeline.bus().unwrap();
    for msg in bus.iter_timed(gstreamer::ClockTime::from_seconds(30)) {
        match msg.view() {
            gstreamer::MessageView::Eos(..) => {
                info!("[{}] Video encoding completed successfully", label);
                break;
            }
            gstreamer::MessageView::Error(err) => {
                let error_msg = format!(
                    "[{}] Video encoding error: {} ({})",
                    label,
                    err.error(),
                    err.debug().unwrap_or_default()
                );
                let _ = pipeline.set_state(gstreamer::State::Null);
                return Err(DoorcamError::component("video_capture", &error_msg));
            }
            _ => {}
        }
    }

    pipeline.set_state(gstreamer::State::Null).map_err(|e| {
        DoorcamError::component(
            "video_capture",
            &format!("[{}] Failed to stop pipeline: {}", label, e),
        )
    })?;

    info!(
        "[{}] GStreamer video encoding completed: {} frames",
        label, frame_index
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
