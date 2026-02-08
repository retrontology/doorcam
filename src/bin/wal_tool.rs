use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use clap::Parser;
use doorcam::{
    config::{CaptureConfig, DoorcamConfig, Rotation as CaptureRotation},
    error::DoorcamError,
    frame::{FrameData, ProcessedFrame, Rotation as FrameRotation},
    wal::WalFrameReader,
};
use serde::Serialize;
use tokio::fs;
use tracing::{error, info, warn};

#[cfg(target_os = "linux")]
use gstreamer::prelude::*;
#[cfg(target_os = "linux")]
use gstreamer::Pipeline;
#[cfg(target_os = "linux")]
use gstreamer_app::AppSrc;

/// Convert a WAL file into media assets (images, video, metadata).
#[derive(Parser, Debug)]
#[command(name = "waltool")]
#[command(about = "Convert doorcam WAL files into images, video, and metadata")]
struct Args {
    /// Path to a WAL file or directory containing WAL files
    #[arg(short, long)]
    input: PathBuf,

    /// Output base directory (defaults to capture.path in config or ./captures)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Path to doorcam configuration file (for capture settings)
    #[arg(short = 'c', long, default_value = "doorcam.toml")]
    config: PathBuf,

    /// Extract JPEG images from the WAL
    #[arg(long)]
    images: bool,

    /// Encode MP4 video from the WAL (requires video_encoding feature on Linux)
    #[arg(long)]
    video: bool,

    /// Write metadata JSON describing the WAL and outputs
    #[arg(long)]
    metadata: bool,

    /// Overwrite existing outputs instead of skipping
    #[arg(long)]
    overwrite: bool,
}

#[derive(Debug, Serialize, Default)]
struct WalOutputs {
    images_dir: Option<String>,
    video_path: Option<String>,
    metadata_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct WalExportMetadata {
    event_id: String,
    wal_path: String,
    frame_count: usize,
    start_timestamp: DateTime<Utc>,
    end_timestamp: DateTime<Utc>,
    outputs: WalOutputs,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let actions = Actions::from_flags(args.images, args.video, args.metadata);

    let capture_config = load_capture_config(&args.config)?;
    let output_base = args
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from(&capture_config.path));

    let wal_paths = collect_wal_paths(&args.input)
        .await
        .context("Failed to discover WAL files")?;

    if wal_paths.is_empty() {
        return Err(anyhow!("No WAL files found at {}", args.input.display()));
    }

    info!(
        "Processing {} WAL file(s) into {}",
        wal_paths.len(),
        output_base.display()
    );

    for wal_path in wal_paths {
        if let Err(e) = process_wal(
            &wal_path,
            &output_base,
            &capture_config,
            &actions,
            args.overwrite,
        )
        .await
        {
            error!("Failed to process {}: {}", wal_path.display(), e);
        }
    }

    Ok(())
}

#[derive(Clone, Copy)]
struct Actions {
    images: bool,
    video: bool,
    metadata: bool,
}

impl Actions {
    fn from_flags(images: bool, video: bool, metadata: bool) -> Self {
        if images || video || metadata {
            Self {
                images,
                video,
                metadata,
            }
        } else {
            // Default to all if nothing specified
            Self {
                images: true,
                video: true,
                metadata: true,
            }
        }
    }
}

async fn process_wal(
    wal_path: &Path,
    output_base: &Path,
    capture_config: &CaptureConfig,
    actions: &Actions,
    overwrite: bool,
) -> Result<()> {
    let event_id = wal_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let mut outputs = WalOutputs::default();
    let event_dir = output_base.join(&event_id);

    let frames_dir = if actions.images {
        Some(prepare_frames_dir(&event_dir, overwrite).await?)
    } else {
        None
    };

    let video_path = if actions.video {
        let path = output_base.join(format!("{}.mp4", event_id));
        if path.exists() && !overwrite {
            return Err(anyhow!(
                "Video file {} exists (use --overwrite to replace)",
                path.display()
            ));
        }
        Some(path)
    } else {
        None
    };

    let timezone = if actions.images {
        Some(resolve_timestamp_timezone(
            &capture_config.timestamp_timezone,
        ))
    } else {
        None
    };

    let mut reader = WalFrameReader::open(wal_path.to_path_buf())
        .await
        .with_context(|| format!("Failed to open WAL {}", wal_path.display()))?;

    info!(
        "Reading {} frames from WAL {}",
        reader.frame_count(),
        wal_path.display()
    );

    let (frame_count, start_ts, end_ts) = if actions.video {
        #[cfg(target_os = "linux")]
        {
            stream_wal_with_video(
                &mut reader,
                frames_dir.as_deref(),
                timezone.as_ref(),
                video_path.as_ref().unwrap(),
                capture_config,
                overwrite,
            )
            .await?
        }

        #[cfg(not(target_os = "linux"))]
        {
            return Err(anyhow!(
                "Video encoding not available (requires video_encoding feature on Linux)"
            ));
        }
    } else {
        stream_wal_without_video(
            &mut reader,
            frames_dir.as_deref(),
            timezone.as_ref(),
            capture_config,
            overwrite,
        )
        .await?
    };

    if frame_count == 0 {
        warn!("WAL {} contained no frames; skipping", wal_path.display());
        return Ok(());
    }

    if let Some(dir) = &frames_dir {
        outputs.images_dir = Some(dir.display().to_string());
    }

    if let Some(path) = &video_path {
        outputs.video_path = Some(path.display().to_string());
    }

    let metadata_path = output_base
        .join("metadata")
        .join(format!("{}.json", event_id));

    let mut metadata = WalExportMetadata {
        event_id: event_id.clone(),
        wal_path: wal_path.display().to_string(),
        frame_count,
        start_timestamp: DateTime::<Utc>::from(start_ts.unwrap_or(SystemTime::UNIX_EPOCH)),
        end_timestamp: DateTime::<Utc>::from(end_ts.unwrap_or(SystemTime::UNIX_EPOCH)),
        outputs,
    };

    if actions.metadata {
        metadata.outputs.metadata_path = Some(metadata_path.display().to_string());
        write_metadata(&metadata, &metadata_path, overwrite).await?;
        info!("Wrote metadata to {}", metadata_path.display());
    }

    info!("Finished processing {}", wal_path.display());
    Ok(())
}

fn load_capture_config(config_path: &Path) -> Result<CaptureConfig> {
    if config_path.exists() {
        let cfg = DoorcamConfig::load_from_file(config_path)
            .with_context(|| format!("Failed to load config from {}", config_path.display()))?;
        Ok(cfg.capture)
    } else {
        warn!(
            "Config file {} not found, using built-in defaults",
            config_path.display()
        );
        Ok(DoorcamConfig::default().capture)
    }
}

async fn collect_wal_paths(input: &Path) -> Result<Vec<PathBuf>> {
    if input.is_file() && input.extension().and_then(|s| s.to_str()) == Some("wal") {
        return Ok(vec![input.to_path_buf()]);
    }

    if input.is_dir() {
        let mut wal_paths = Vec::new();
        let mut entries = fs::read_dir(input)
            .await
            .with_context(|| format!("Failed to read directory {}", input.display()))?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("wal") {
                wal_paths.push(path);
            }
        }
        return Ok(wal_paths);
    }

    Err(anyhow!(
        "Input {} is neither a WAL file nor a directory",
        input.display()
    ))
}

async fn write_metadata(
    metadata: &WalExportMetadata,
    metadata_path: &Path,
    overwrite: bool,
) -> Result<()> {
    if let Some(parent) = metadata_path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create metadata directory {}", parent.display()))?;
    }

    if metadata_path.exists() && !overwrite {
        return Err(anyhow!(
            "Metadata file {} exists (use --overwrite to replace)",
            metadata_path.display()
        ));
    }

    let data = serde_json::to_vec_pretty(metadata)?;
    fs::write(&metadata_path, data)
        .await
        .with_context(|| format!("Failed to write {}", metadata_path.display()))?;

    Ok(())
}

async fn prepare_jpeg(frame: &FrameData, config: &CaptureConfig) -> Result<Arc<Vec<u8>>> {
    let rotation = map_capture_rotation(config.rotation.as_ref());

    // If we need to rotate or overlay, go through processing pipeline
    if config.timestamp_overlay || rotation.is_some() {
        let processed_frame = ProcessedFrame::from_frame(frame.clone(), rotation).await?;
        let base_jpeg = processed_frame.get_jpeg().await?;

        if config.timestamp_overlay {
            add_timestamp_overlay(&base_jpeg, frame.timestamp, config).await
        } else {
            Ok(base_jpeg)
        }
    } else {
        Ok(frame.data.clone())
    }
}

async fn add_timestamp_overlay(
    jpeg_data: &[u8],
    timestamp: SystemTime,
    config: &CaptureConfig,
) -> Result<Arc<Vec<u8>>> {
    use image::{DynamicImage, ImageFormat, Rgba};
    use imageproc::drawing::{draw_text_mut, text_size};
    use rusttype::{Font, Scale};
    use std::fs as stdfs;

    let mut img = image::load_from_memory_with_format(jpeg_data, ImageFormat::Jpeg)
        .map_err(|e| DoorcamError::component("waltool", &format!("JPEG decode failed: {}", e)))?
        .to_rgba8();

    let timezone = resolve_timestamp_timezone(&config.timestamp_timezone);
    let datetime = DateTime::<Utc>::from(timestamp).with_timezone(&timezone);
    let timestamp_text = datetime.format("%Y-%m-%d %H:%M:%S%.3f %Z").to_string();

    let font_data = stdfs::read(&config.timestamp_font_path).map_err(|e| {
        DoorcamError::component(
            "waltool",
            &format!(
                "Failed to read font file '{}': {}",
                config.timestamp_font_path, e
            ),
        )
    })?;

    let font = Font::try_from_vec(font_data).ok_or_else(|| {
        DoorcamError::component(
            "waltool",
            &format!("Failed to parse font file '{}'", config.timestamp_font_path),
        )
    })?;

    let scale = Scale::uniform(config.timestamp_font_size);
    let x: u32 = 10;
    let y: u32 = img
        .height()
        .saturating_sub((config.timestamp_font_size * 1.5) as u32);

    let (text_width, text_height) = text_size(scale, &font, &timestamp_text);

    for dy in 0..(text_height as u32 + 10) {
        for dx in 0..(text_width as u32 + 10) {
            let px = x.saturating_sub(5) + dx;
            let py = y.saturating_sub(5) + dy;
            if px < img.width() && py < img.height() {
                let pixel = img.get_pixel(px, py);
                img.put_pixel(
                    px,
                    py,
                    Rgba([pixel[0] / 3, pixel[1] / 3, pixel[2] / 3, 255]),
                );
            }
        }
    }

    draw_text_mut(
        &mut img,
        Rgba([255, 255, 255, 255]),
        x as i32,
        y as i32,
        scale,
        &font,
        &timestamp_text,
    );

    let mut output = Vec::new();
    DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut output), ImageFormat::Jpeg)
        .map_err(|e| {
            DoorcamError::component(
                "waltool",
                &format!("Failed to encode JPEG with overlay: {}", e),
            )
        })?;

    Ok(Arc::new(output))
}

fn map_capture_rotation(rotation: Option<&CaptureRotation>) -> Option<FrameRotation> {
    rotation.map(|rot| match rot {
        CaptureRotation::Rotate90 => FrameRotation::Rotate90,
        CaptureRotation::Rotate180 => FrameRotation::Rotate180,
        CaptureRotation::Rotate270 => FrameRotation::Rotate270,
    })
}

fn resolve_timestamp_timezone(tz_name: &str) -> Tz {
    match tz_name.parse::<Tz>() {
        Ok(tz) => tz,
        Err(_) => {
            warn!(
                "Invalid timestamp timezone '{}', falling back to UTC",
                tz_name
            );
            chrono_tz::UTC
        }
    }
}

#[cfg(target_os = "linux")]
async fn stream_wal_with_video(
    reader: &mut WalFrameReader,
    frames_dir: Option<&Path>,
    timezone: Option<&Tz>,
    video_path: &Path,
    config: &CaptureConfig,
    overwrite: bool,
) -> Result<(usize, Option<SystemTime>, Option<SystemTime>)> {
    // Initialize GStreamer if not already done
    gstreamer::init().map_err(|e| {
        DoorcamError::component("waltool", &format!("Failed to initialize GStreamer: {}", e))
    })?;

    let pipeline_desc = format!(
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

    stream_wal_with_pipeline(
        "waltool",
        &pipeline_desc,
        reader,
        frames_dir,
        timezone,
        video_path,
        config,
        overwrite,
    )
    .await
}

#[cfg(target_os = "linux")]
async fn stream_wal_with_pipeline(
    label: &str,
    pipeline_desc: &str,
    reader: &mut WalFrameReader,
    frames_dir: Option<&Path>,
    timezone: Option<&Tz>,
    video_path: &Path,
    config: &CaptureConfig,
    overwrite: bool,
) -> Result<(usize, Option<SystemTime>, Option<SystemTime>)> {
    let pipeline = gstreamer::parse::launch(pipeline_desc)
        .map_err(|e| {
            DoorcamError::component(
                "waltool",
                &format!("[{}] Failed to create pipeline: {}", label, e),
            )
        })?
        .downcast::<Pipeline>()
        .map_err(|_| {
            DoorcamError::component(
                "waltool",
                &format!("[{}] Failed to downcast to Pipeline", label),
            )
        })?;

    let appsrc = pipeline
        .by_name("src")
        .ok_or_else(|| {
            DoorcamError::component(
                "waltool",
                &format!("[{}] Failed to get appsrc element", label),
            )
        })?
        .downcast::<AppSrc>()
        .map_err(|_| {
            DoorcamError::component(
                "waltool",
                &format!("[{}] Failed to downcast to AppSrc", label),
            )
        })?;

    appsrc.set_property("format", gstreamer::Format::Time);
    appsrc.set_property("is-live", false);

    pipeline.set_state(gstreamer::State::Playing).map_err(|e| {
        DoorcamError::component(
            "waltool",
            &format!("[{}] Failed to start pipeline: {}", label, e),
        )
    })?;

    let mut frame_count = 0usize;
    let (start_ts, mut current_frame) = match reader.next_frame().await? {
        Some(frame) => (Some(frame.timestamp), frame),
        None => {
            appsrc.end_of_stream().map_err(|e| {
                DoorcamError::component(
                    "waltool",
                    &format!("[{}] Failed to signal EOS: {:?}", label, e),
                )
            })?;
            return Ok((0, None, None));
        }
    };

    let base_time = current_frame
        .timestamp
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos() as u64;

    info!("[{}] Encoding frames to {}", label, video_path.display());

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
                "waltool",
                &format!("[{}] Frame processing failed: {}", label, e),
            )
        })?;

        if let (Some(dir), Some(tz)) = (frames_dir, timezone) {
            write_frame_jpeg(&jpeg_data, current_frame.timestamp, dir, tz, overwrite).await?;
        }

        let mut buffer = gstreamer::Buffer::with_size(jpeg_data.len()).map_err(|e| {
            DoorcamError::component(
                "waltool",
                &format!("[{}] Failed to create buffer: {}", label, e),
            )
        })?;

        {
            let buffer_ref = buffer.get_mut().unwrap();
            let mut map = buffer_ref.map_writable().map_err(|e| {
                DoorcamError::component(
                    "waltool",
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
                "waltool",
                &format!("[{}] Failed to push buffer: {:?}", label, e),
            )
        })?;

        frame_count += 1;

        match next_frame {
            Some(next) => current_frame = next,
            None => break,
        }
    }

    appsrc.end_of_stream().map_err(|e| {
        DoorcamError::component(
            "waltool",
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
                return Err(DoorcamError::component("waltool", &error_msg).into());
            }
            _ => {}
        }
    }

    pipeline.set_state(gstreamer::State::Null).map_err(|e| {
        DoorcamError::component(
            "waltool",
            &format!("[{}] Failed to stop pipeline: {}", label, e),
        )
    })?;

    let end_ts = Some(current_frame.timestamp);

    info!(
        "[{}] Video encoding completed: {} frames -> {}",
        label,
        frame_count,
        video_path.display()
    );
    Ok((frame_count, start_ts, end_ts))
}

#[cfg(not(target_os = "linux"))]
async fn stream_wal_with_video(
    _reader: &mut WalFrameReader,
    _frames_dir: Option<&Path>,
    _timezone: Option<&Tz>,
    _video_path: &Path,
    _config: &CaptureConfig,
    _overwrite: bool,
) -> Result<(usize, Option<SystemTime>, Option<SystemTime>)> {
    Err(anyhow!(
        "Video encoding not available (requires video_encoding feature on Linux)"
    ))
}

async fn stream_wal_without_video(
    reader: &mut WalFrameReader,
    frames_dir: Option<&Path>,
    timezone: Option<&Tz>,
    config: &CaptureConfig,
    overwrite: bool,
) -> Result<(usize, Option<SystemTime>, Option<SystemTime>)> {
    let mut frame_count = 0usize;
    let mut start_ts = None;
    let mut end_ts = None;

    while let Some(frame) = reader.next_frame().await? {
        let jpeg_data = if frames_dir.is_some() {
            Some(
                prepare_jpeg(&frame, config)
                    .await
                    .with_context(|| format!("Failed to prepare JPEG for frame {}", frame.id))?,
            )
        } else {
            None
        };

        if let (Some(dir), Some(tz), Some(jpeg)) = (frames_dir, timezone, jpeg_data.as_ref()) {
            write_frame_jpeg(jpeg, frame.timestamp, dir, tz, overwrite).await?;
        }

        if start_ts.is_none() {
            start_ts = Some(frame.timestamp);
        }
        end_ts = Some(frame.timestamp);
        frame_count += 1;
    }

    if let Some(dir) = frames_dir {
        info!("Extracted {} JPEG files to {}", frame_count, dir.display());
    }

    Ok((frame_count, start_ts, end_ts))
}

async fn prepare_frames_dir(event_dir: &Path, overwrite: bool) -> Result<PathBuf> {
    let frames_dir = event_dir.join("frames");

    if frames_dir.exists() && !overwrite {
        return Err(anyhow!(
            "Frames directory {} already exists (use --overwrite to replace)",
            frames_dir.display()
        ));
    }

    fs::create_dir_all(&frames_dir)
        .await
        .with_context(|| format!("Failed to create frames directory {}", frames_dir.display()))?;

    Ok(frames_dir)
}

async fn write_frame_jpeg(
    jpeg_data: &[u8],
    timestamp: SystemTime,
    frames_dir: &Path,
    timezone: &Tz,
    overwrite: bool,
) -> Result<()> {
    let timestamp = DateTime::<Utc>::from(timestamp).with_timezone(timezone);
    let filename = format!("{}.jpg", timestamp.format("%Y%m%d_%H%M%S_%3f"));
    let file_path = frames_dir.join(&filename);

    if file_path.exists() && !overwrite {
        warn!("Skipping existing frame {}", file_path.display());
        return Ok(());
    }

    fs::write(&file_path, jpeg_data)
        .await
        .with_context(|| format!("Failed to write {}", file_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use doorcam::frame::FrameFormat;
    use doorcam::wal::WalWriter;
    use tempfile::TempDir;

    async fn write_test_wal(
        wal_dir: &Path,
        event_id: &str,
        timestamps: &[SystemTime],
    ) -> Result<PathBuf> {
        let mut writer = WalWriter::new(event_id.to_string(), wal_dir, 30)
            .await
            .context("Failed to create WAL writer")?;

        for (idx, timestamp) in timestamps.iter().enumerate() {
            let frame = FrameData::new(
                idx as u64,
                *timestamp,
                vec![0u8; 128],
                640,
                480,
                FrameFormat::Mjpeg,
            );
            writer
                .append_frame(&frame)
                .await
                .context("Failed to append frame")?;
        }

        writer.close().await.context("Failed to close WAL")
    }

    #[tokio::test]
    async fn stream_wal_without_video_returns_counts_and_timestamps() -> Result<()> {
        let temp_dir = TempDir::new().context("Failed to create temp dir")?;
        let wal_dir = temp_dir.path().join("wal");
        fs::create_dir_all(&wal_dir)
            .await
            .context("Failed to create WAL dir")?;

        let timestamps = vec![
            SystemTime::UNIX_EPOCH + Duration::from_millis(1_000),
            SystemTime::UNIX_EPOCH + Duration::from_millis(2_000),
            SystemTime::UNIX_EPOCH + Duration::from_millis(3_000),
        ];

        let wal_path = write_test_wal(&wal_dir, "event-1", &timestamps).await?;
        let mut reader = WalFrameReader::open(wal_path).await?;
        let capture_config = DoorcamConfig::default().capture;

        let (frame_count, start_ts, end_ts) =
            stream_wal_without_video(&mut reader, None, None, &capture_config, true).await?;

        assert_eq!(frame_count, 3);
        assert_eq!(start_ts, Some(timestamps[0]));
        assert_eq!(end_ts, Some(timestamps[2]));

        Ok(())
    }

    #[tokio::test]
    async fn stream_wal_without_video_writes_images() -> Result<()> {
        let temp_dir = TempDir::new().context("Failed to create temp dir")?;
        let wal_dir = temp_dir.path().join("wal");
        fs::create_dir_all(&wal_dir)
            .await
            .context("Failed to create WAL dir")?;

        let event_dir = temp_dir.path().join("event-2");
        let frames_dir = prepare_frames_dir(&event_dir, true).await?;

        let timestamps = vec![
            SystemTime::UNIX_EPOCH + Duration::from_millis(10),
            SystemTime::UNIX_EPOCH + Duration::from_millis(20),
        ];

        let wal_path = write_test_wal(&wal_dir, "event-2", &timestamps).await?;
        let mut reader = WalFrameReader::open(wal_path).await?;
        let mut capture_config = DoorcamConfig::default().capture;
        capture_config.timestamp_overlay = false;
        capture_config.rotation = None;
        let timezone = resolve_timestamp_timezone(&capture_config.timestamp_timezone);

        let (frame_count, start_ts, end_ts) = stream_wal_without_video(
            &mut reader,
            Some(&frames_dir),
            Some(&timezone),
            &capture_config,
            true,
        )
        .await?;

        assert_eq!(frame_count, 2);
        assert_eq!(start_ts, Some(timestamps[0]));
        assert_eq!(end_ts, Some(timestamps[1]));

        let mut files = std::fs::read_dir(&frames_dir)
            .with_context(|| format!("Failed to read {}", frames_dir.display()))?;
        let mut count = 0usize;
        while files.next().transpose()?.is_some() {
            count += 1;
        }
        assert_eq!(count, 2);

        Ok(())
    }
}
