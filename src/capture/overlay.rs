use crate::config::CaptureConfig;
use crate::error::{DoorcamError, Result};
use crate::frame::{FrameData, ProcessedFrame, Rotation as FrameRotation};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::debug;

/// Resolve configured timezone, falling back to UTC on parse errors
pub(crate) fn resolve_timestamp_timezone(tz_name: &str) -> Tz {
    match tz_name.parse::<Tz>() {
        Ok(tz) => tz,
        Err(_) => {
            tracing::warn!(
                "Invalid timestamp timezone '{}', falling back to UTC",
                tz_name
            );
            chrono_tz::UTC
        }
    }
}

/// Prepare JPEG data for storage/encoding with optional rotation and overlay
pub(crate) async fn prepare_jpeg(
    frame: &FrameData,
    config: &CaptureConfig,
) -> Result<Arc<Vec<u8>>> {
    let rotation = map_capture_rotation(config.rotation.as_ref());

    if config.timestamp_overlay || rotation.is_some() {
        let processed_frame = ProcessedFrame::from_frame(frame.clone(), rotation).await?;
        let base_jpeg = processed_frame.get_jpeg().await?;

        if config.timestamp_overlay {
            add_timestamp_overlay_static(&base_jpeg, frame.timestamp, config).await
        } else {
            Ok(base_jpeg)
        }
    } else {
        Ok(frame.data.clone())
    }
}

/// Add timestamp overlay to JPEG image (static version for use in static context)
pub(crate) async fn add_timestamp_overlay_static(
    jpeg_data: &[u8],
    timestamp: SystemTime,
    config: &CaptureConfig,
) -> Result<Arc<Vec<u8>>> {
    use image::{DynamicImage, ImageFormat, Rgba};
    use imageproc::drawing::{draw_text_mut, text_size};
    use rusttype::{Font, Scale};
    use std::fs;

    let mut img = image::load_from_memory_with_format(jpeg_data, ImageFormat::Jpeg)
        .map_err(|e| {
            DoorcamError::component(
                "video_capture",
                &format!("Failed to decode JPEG for overlay: {}", e),
            )
        })?
        .to_rgba8();

    let timezone = resolve_timestamp_timezone(&config.timestamp_timezone);
    let datetime = DateTime::<Utc>::from(timestamp).with_timezone(&timezone);
    let timestamp_text = datetime.format("%Y-%m-%d %H:%M:%S%.3f %Z").to_string();

    let font_data = fs::read(&config.timestamp_font_path).map_err(|e| {
        DoorcamError::component(
            "video_capture",
            &format!(
                "Failed to read font file '{}': {}",
                config.timestamp_font_path, e
            ),
        )
    })?;

    let font = Font::try_from_vec(font_data).ok_or_else(|| {
        DoorcamError::component(
            "video_capture",
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
                "video_capture",
                &format!("Failed to encode JPEG with overlay: {}", e),
            )
        })?;

    debug!(
        "Added timestamp overlay: {} (font: {}, size: {})",
        timestamp_text, config.timestamp_font_path, config.timestamp_font_size
    );
    Ok(Arc::new(output))
}

/// Map configuration rotation into frame processing rotation
pub(crate) fn map_capture_rotation(
    rotation: Option<&crate::config::Rotation>,
) -> Option<FrameRotation> {
    rotation.map(|rot| match rot {
        crate::config::Rotation::Rotate90 => FrameRotation::Rotate90,
        crate::config::Rotation::Rotate180 => FrameRotation::Rotate180,
        crate::config::Rotation::Rotate270 => FrameRotation::Rotate270,
    })
}
