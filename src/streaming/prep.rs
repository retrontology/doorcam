use crate::error::Result;
use crate::frame::{FrameData, FrameFormat};
use tracing::{debug, warn};

/// Prepare a frame for streaming by ensuring it's in JPEG format
pub async fn prepare_frame_for_streaming(frame: &FrameData) -> Result<Vec<u8>> {
    match frame.format {
        FrameFormat::Mjpeg => {
            debug!("Frame {} already in MJPEG format, using directly", frame.id);
            Ok(frame.data.as_ref().clone())
        }
        FrameFormat::Yuyv => {
            warn!(
                "YUYV to JPEG encoding for frame {} not yet implemented - using placeholder",
                frame.id
            );
            encode_placeholder(frame.width, frame.height, "YUYV")
        }
        FrameFormat::Rgb24 => {
            warn!(
                "RGB24 to JPEG encoding for frame {} not yet implemented - using placeholder",
                frame.id
            );
            encode_placeholder(frame.width, frame.height, "RGB24")
        }
    }
}

/// Create a placeholder JPEG for non-MJPEG formats
pub fn encode_placeholder(width: u32, height: u32, source_format: &str) -> Result<Vec<u8>> {
    let mut jpeg_data = Vec::new();

    jpeg_data.extend_from_slice(&[0xFF, 0xD8]);
    jpeg_data.extend_from_slice(&[0xFF, 0xE0]);
    jpeg_data.extend_from_slice(&[0x00, 0x10]);
    jpeg_data.extend_from_slice(b"JFIF\0");
    jpeg_data.extend_from_slice(&[0x01, 0x01]);
    jpeg_data.extend_from_slice(&[0x01]);
    jpeg_data.extend_from_slice(&[0x00, 0x48]);
    jpeg_data.extend_from_slice(&[0x00, 0x48]);
    jpeg_data.extend_from_slice(&[0x00, 0x00]);

    jpeg_data.extend_from_slice(&[0xFF, 0xC0]);
    jpeg_data.extend_from_slice(&[0x00, 0x11]);
    jpeg_data.extend_from_slice(&[0x08]);
    jpeg_data.extend_from_slice(&[(height >> 8) as u8, height as u8]);
    jpeg_data.extend_from_slice(&[(width >> 8) as u8, width as u8]);
    jpeg_data.extend_from_slice(&[0x03]);
    jpeg_data.extend_from_slice(&[0x01, 0x22, 0x00]);
    jpeg_data.extend_from_slice(&[0x02, 0x11, 0x01]);
    jpeg_data.extend_from_slice(&[0x03, 0x11, 0x01]);

    jpeg_data.extend_from_slice(&[0xFF, 0xC4]);
    jpeg_data.extend_from_slice(&[0x00, 0x1F]);
    jpeg_data.extend_from_slice(&[0x00]);
    jpeg_data.extend_from_slice(&[0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01]);
    jpeg_data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    jpeg_data.extend_from_slice(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]);
    jpeg_data.extend_from_slice(&[0x08, 0x09, 0x0A, 0x0B]);

    jpeg_data.extend_from_slice(&[0xFF, 0xDA]);
    jpeg_data.extend_from_slice(&[0x00, 0x0C]);
    jpeg_data.extend_from_slice(&[0x03]);
    jpeg_data.extend_from_slice(&[0x01, 0x00]);
    jpeg_data.extend_from_slice(&[0x02, 0x11]);
    jpeg_data.extend_from_slice(&[0x03, 0x11]);
    jpeg_data.extend_from_slice(&[0x00, 0x3F, 0x00]);

    jpeg_data.extend_from_slice(&[0xFF, 0x00]);
    jpeg_data.extend_from_slice(&[0xFF, 0xD9]);

    debug!(
        "Created placeholder JPEG for {}x{} frame from {} format ({} bytes)",
        width,
        height,
        source_format,
        jpeg_data.len()
    );

    Ok(jpeg_data)
}

pub fn rotation_to_degrees(rotation: Option<crate::config::Rotation>) -> u16 {
    match rotation {
        Some(crate::config::Rotation::Rotate90) => 90,
        Some(crate::config::Rotation::Rotate180) => 180,
        Some(crate::config::Rotation::Rotate270) => 270,
        None => 0,
    }
}
