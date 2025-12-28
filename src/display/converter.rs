use crate::error::{DisplayError, Result};
use tracing::debug;

/// Display format conversion utilities
pub struct DisplayConverter;

impl DisplayConverter {
    /// Create placeholder RGB565 data for testing
    pub fn create_placeholder_rgb565(width: u32, height: u32) -> Result<Vec<u8>> {
        let pixel_count = (width * height) as usize;
        let mut data = Vec::with_capacity(pixel_count * 2);

        for y in 0..height {
            for x in 0..width {
                let r = ((x * 31) / width) as u16;
                let g = ((y * 63) / height) as u16;
                let b = (((x + y) * 31) / (width + height)) as u16;

                let rgb565 = (r << 11) | (g << 5) | b;
                data.push((rgb565 & 0xFF) as u8);
                data.push((rgb565 >> 8) as u8);
            }
        }

        Ok(data)
    }

    /// Convert RGB24 to RGB565 format with optional scaling
    pub fn rgb24_to_rgb565(rgb24_data: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
        let expected_size = (width * height * 3) as usize;
        if rgb24_data.len() != expected_size {
            return Err(DisplayError::FormatConversion {
                details: format!(
                    "Invalid RGB24 data size: expected {}, got {}",
                    expected_size,
                    rgb24_data.len()
                ),
            }
            .into());
        }

        let mut rgb565_data = Vec::with_capacity((width * height * 2) as usize);

        for chunk in rgb24_data.chunks_exact(3) {
            let r = chunk[0] >> 3;
            let g = chunk[1] >> 2;
            let b = chunk[2] >> 3;

            let rgb565 = ((r as u16) << 11) | ((g as u16) << 5) | (b as u16);

            rgb565_data.push((rgb565 & 0xFF) as u8);
            rgb565_data.push((rgb565 >> 8) as u8);
        }

        Ok(rgb565_data)
    }

    /// Scale RGB565 data to target resolution using simple nearest neighbor
    pub fn scale_rgb565(
        data: &[u8],
        src_width: u32,
        src_height: u32,
        dst_width: u32,
        dst_height: u32,
    ) -> Result<Vec<u8>> {
        if data.len() != (src_width * src_height * 2) as usize {
            return Err(DisplayError::FormatConversion {
                details: format!(
                    "Invalid RGB565 data size: expected {}, got {}",
                    src_width * src_height * 2,
                    data.len()
                ),
            }
            .into());
        }

        let mut scaled_data = Vec::with_capacity((dst_width * dst_height * 2) as usize);

        let x_ratio = src_width as f32 / dst_width as f32;
        let y_ratio = src_height as f32 / dst_height as f32;

        for dst_y in 0..dst_height {
            for dst_x in 0..dst_width {
                let src_x = ((dst_x as f32) * x_ratio) as u32;
                let src_y = ((dst_y as f32) * y_ratio) as u32;

                let src_x = src_x.min(src_width - 1);
                let src_y = src_y.min(src_height - 1);

                let src_index = ((src_y * src_width + src_x) * 2) as usize;

                scaled_data.push(data[src_index]);
                scaled_data.push(data[src_index + 1]);
            }
        }

        Ok(scaled_data)
    }

    /// Crop RGB565 data to fit within target dimensions (center crop)
    pub fn crop_rgb565(
        data: &[u8],
        src_width: u32,
        src_height: u32,
        dst_width: u32,
        dst_height: u32,
    ) -> Result<Vec<u8>> {
        if data.len() != (src_width * src_height * 2) as usize {
            return Err(DisplayError::FormatConversion {
                details: format!(
                    "Invalid RGB565 data size: expected {}, got {}",
                    src_width * src_height * 2,
                    data.len()
                ),
            }
            .into());
        }

        let crop_width = dst_width.min(src_width);
        let crop_height = dst_height.min(src_height);
        let offset_x = (src_width - crop_width) / 2;
        let offset_y = (src_height - crop_height) / 2;

        let mut cropped_data = Vec::with_capacity((crop_width * crop_height * 2) as usize);

        for y in 0..crop_height {
            let src_y = offset_y + y;
            let src_row_start = (src_y * src_width + offset_x) as usize * 2;
            let src_row_end = src_row_start + (crop_width as usize * 2);

            cropped_data.extend_from_slice(&data[src_row_start..src_row_end]);
        }

        Ok(cropped_data)
    }

    /// Apply rotation to display data (placeholder for future implementation)
    pub fn apply_rotation(
        data: &[u8],
        _width: u32,
        _height: u32,
        rotation: crate::config::Rotation,
    ) -> Result<Vec<u8>> {
        debug!(
            "Display rotation {:?} requested - placeholder implementation",
            rotation
        );
        Ok(data.to_vec())
    }
}
