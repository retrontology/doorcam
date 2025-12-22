use std::sync::Arc;
use std::time::SystemTime;
use serde::{Deserialize, Serialize};
#[cfg(feature = "motion_analysis")]
use image::codecs::jpeg::JpegEncoder;

/// Frame format enumeration supporting different video formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameFormat {
    /// Motion JPEG format - compressed JPEG frames
    Mjpeg,
    /// YUV 4:2:2 format - uncompressed YUV data
    Yuyv,
    /// RGB24 format - uncompressed RGB data
    Rgb24,
}

impl FrameFormat {
    /// Get bytes per pixel for the format
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            FrameFormat::Mjpeg => 0, // Variable size, compressed
            FrameFormat::Yuyv => 2,  // 2 bytes per pixel
            FrameFormat::Rgb24 => 3, // 3 bytes per pixel
        }
    }
    
    /// Check if format is compressed
    pub fn is_compressed(&self) -> bool {
        matches!(self, FrameFormat::Mjpeg)
    }
}

/// Rotation options for frame processing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rotation {
    /// Rotate 90 degrees clockwise
    Rotate90,
    /// Rotate 180 degrees
    Rotate180,
    /// Rotate 270 degrees clockwise (90 degrees counter-clockwise)
    Rotate270,
}

impl Rotation {
    /// Get rotation angle in degrees
    pub fn degrees(&self) -> u16 {
        match self {
            Rotation::Rotate90 => 90,
            Rotation::Rotate180 => 180,
            Rotation::Rotate270 => 270,
        }
    }
}

/// Frame data structure containing raw frame data and metadata
#[derive(Debug, Clone)]
pub struct FrameData {
    /// Unique frame identifier
    pub id: u64,
    /// Timestamp when frame was captured
    pub timestamp: SystemTime,
    /// Raw frame data (shared ownership for efficiency)
    pub data: Arc<Vec<u8>>,
    /// Frame width in pixels
    pub width: u32,
    /// Frame height in pixels
    pub height: u32,
    /// Frame format
    pub format: FrameFormat,
}

impl FrameData {
    /// Create a new frame data instance
    pub fn new(
        id: u64,
        timestamp: SystemTime,
        data: Vec<u8>,
        width: u32,
        height: u32,
        format: FrameFormat,
    ) -> Self {
        Self {
            id,
            timestamp,
            data: Arc::new(data),
            width,
            height,
            format,
        }
    }
    
    /// Get the expected frame size for uncompressed formats
    pub fn expected_size(&self) -> Option<usize> {
        if self.format.is_compressed() {
            None
        } else {
            Some(self.width as usize * self.height as usize * self.format.bytes_per_pixel())
        }
    }
    
    /// Validate frame data size against expected size
    pub fn validate_size(&self) -> bool {
        match self.expected_size() {
            Some(expected) => self.data.len() == expected,
            None => true, // Compressed formats have variable size
        }
    }
    
    /// Get frame age in milliseconds
    pub fn age_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(self.timestamp)
            .unwrap_or_default()
            .as_millis() as u64
    }
    
    /// Check if frame is older than specified duration
    pub fn is_older_than(&self, duration: std::time::Duration) -> bool {
        SystemTime::now()
            .duration_since(self.timestamp)
            .map(|age| age > duration)
            .unwrap_or(false)
    }
}

/// Processed frame with additional transformations applied
#[derive(Debug, Clone)]
pub struct ProcessedFrame {
    /// Original frame data
    pub original: FrameData,
    /// Rotated frame data (if rotation was applied)
    pub rotated: Option<Arc<Vec<u8>>>,
    /// JPEG encoded frame data (if encoding was applied)
    pub jpeg_encoded: Option<Arc<Vec<u8>>>,
    /// Display-ready frame data (if display conversion was applied)
    pub display_ready: Option<Arc<Vec<u8>>>,
}

impl ProcessedFrame {
    /// Create a processed frame from original frame data
    pub fn new(frame: FrameData) -> Self {
        Self {
            original: frame,
            rotated: None,
            jpeg_encoded: None,
            display_ready: None,
        }
    }

    /// Create a processed frame from original frame data with optional rotation
    pub async fn from_frame(frame: FrameData, rotation: Option<Rotation>) -> Result<Self, crate::error::DoorcamError> {
        let mut processed = Self {
            original: frame,
            rotated: None,
            jpeg_encoded: None,
            display_ready: None,
        };

        // Apply rotation if specified
        if let Some(rot) = rotation {
            processed.rotated = Some(FrameProcessor::apply_rotation(&processed.original, rot).await?);
        }

        Ok(processed)
    }
    
    /// Get JPEG encoded data, encoding if necessary
    pub async fn get_jpeg(&self) -> Result<Arc<Vec<u8>>, crate::error::DoorcamError> {
        // Return cached JPEG if available
        if let Some(ref jpeg) = self.jpeg_encoded {
            return Ok(Arc::clone(jpeg));
        }

        // Use rotated data if available, otherwise original
        let source_frame = if self.rotated.is_some() {
            // Create a temporary frame with rotated data for encoding
            FrameData {
                id: self.original.id,
                timestamp: self.original.timestamp,
                data: Arc::clone(self.rotated.as_ref().unwrap()),
                width: self.original.width,
                height: self.original.height,
                format: self.original.format,
            }
        } else {
            self.original.clone()
        };

        // Encode to JPEG
        FrameProcessor::encode_jpeg(&source_frame).await
    }
    
    /// Get the most appropriate frame data for the requested purpose
    pub fn get_data_for_purpose(&self, purpose: FramePurpose) -> &Arc<Vec<u8>> {
        match purpose {
            FramePurpose::Display => {
                self.display_ready
                    .as_ref()
                    .or(self.rotated.as_ref())
                    .unwrap_or(&self.original.data)
            }
            FramePurpose::Streaming => {
                self.jpeg_encoded
                    .as_ref()
                    .or(self.rotated.as_ref())
                    .unwrap_or(&self.original.data)
            }
            FramePurpose::Storage => {
                self.jpeg_encoded
                    .as_ref()
                    .or(self.rotated.as_ref())
                    .unwrap_or(&self.original.data)
            }
            FramePurpose::Analysis => {
                self.rotated.as_ref().unwrap_or(&self.original.data)
            }
        }
    }
}

/// Purpose for which frame data is being requested
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramePurpose {
    /// Frame for display on screen
    Display,
    /// Frame for streaming over network
    Streaming,
    /// Frame for storage/recording
    Storage,
    /// Frame for motion analysis
    Analysis,
}

/// Frame processing utilities
pub struct FrameProcessor;

impl FrameProcessor {
    /// Apply rotation to frame data (placeholder for future OpenCV integration)
    pub async fn apply_rotation(
        frame: &FrameData,
        rotation: Rotation,
    ) -> Result<Arc<Vec<u8>>, crate::error::DoorcamError> {
        #[cfg(feature = "motion_analysis")]
        {
            // For MJPEG, decode, rotate, and re-encode. For other formats, no-op for now.
            match frame.format {
                FrameFormat::Mjpeg => {
                    let img = image::load_from_memory(&frame.data).map_err(|e| {
                        crate::error::ProcessingError::Rotation {
                            details: format!("JPEG decode failed: {}", e),
                        }
                    })?;

                    let rotated = match rotation {
                        Rotation::Rotate90 => img.rotate90(),
                        Rotation::Rotate180 => img.rotate180(),
                        Rotation::Rotate270 => img.rotate270(),
                    };

                    let mut buf = Vec::new();
                    let mut encoder = JpegEncoder::new_with_quality(&mut buf, 90);
                    encoder.encode_image(&rotated).map_err(|e| {
                        crate::error::ProcessingError::JpegEncoding {
                            details: e.to_string(),
                        }
                    })?;

                    Ok(Arc::new(buf))
                }
                _ => {
                    tracing::debug!(
                        "Rotation {:?} requested for non-MJPEG frame {} - returning original",
                        rotation,
                        frame.id
                    );
                    Ok(Arc::clone(&frame.data))
                }
            }
        }

        #[cfg(not(feature = "motion_analysis"))]
        {
            tracing::warn!(
                "Rotation {:?} requested for frame {} but motion_analysis feature is disabled; returning original bytes",
                rotation,
                frame.id
            );
            Ok(Arc::clone(&frame.data))
        }
    }
    
    /// Convert frame to JPEG format (placeholder for future OpenCV integration)
    pub async fn encode_jpeg(
        frame: &FrameData,
    ) -> Result<Arc<Vec<u8>>, crate::error::DoorcamError> {
        match frame.format {
            FrameFormat::Mjpeg => {
                // Already JPEG encoded
                Ok(Arc::clone(&frame.data))
            }
            _ => {
                // TODO: Implement actual JPEG encoding using OpenCV in later tasks
                tracing::debug!(
                    "JPEG encoding requested for frame {} ({:?} format) - placeholder implementation",
                    frame.id,
                    frame.format
                );
                
                Ok(Arc::clone(&frame.data))
            }
        }
    }
    
    /// Convert frame for display (placeholder for future implementation)
    pub async fn prepare_for_display(
        frame: &FrameData,
        target_format: FrameFormat,
    ) -> Result<Arc<Vec<u8>>, crate::error::DoorcamError> {
        // TODO: Implement actual format conversion for display in later tasks
        tracing::debug!(
            "Display conversion requested for frame {} ({:?} -> {:?}) - placeholder implementation",
            frame.id,
            frame.format,
            target_format
        );
        
        Ok(Arc::clone(&frame.data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    
    #[test]
    fn test_frame_format_properties() {
        assert_eq!(FrameFormat::Mjpeg.bytes_per_pixel(), 0);
        assert_eq!(FrameFormat::Yuyv.bytes_per_pixel(), 2);
        assert_eq!(FrameFormat::Rgb24.bytes_per_pixel(), 3);
        
        assert!(FrameFormat::Mjpeg.is_compressed());
        assert!(!FrameFormat::Yuyv.is_compressed());
        assert!(!FrameFormat::Rgb24.is_compressed());
    }
    
    #[test]
    fn test_rotation_degrees() {
        assert_eq!(Rotation::Rotate90.degrees(), 90);
        assert_eq!(Rotation::Rotate180.degrees(), 180);
        assert_eq!(Rotation::Rotate270.degrees(), 270);
    }
    
    #[test]
    fn test_frame_data_creation() {
        let data = vec![0u8; 1920 * 1080 * 2]; // YUYV data
        let frame = FrameData::new(
            1,
            SystemTime::now(),
            data,
            1920,
            1080,
            FrameFormat::Yuyv,
        );
        
        assert_eq!(frame.id, 1);
        assert_eq!(frame.width, 1920);
        assert_eq!(frame.height, 1080);
        assert_eq!(frame.format, FrameFormat::Yuyv);
        assert!(frame.validate_size());
    }
    
    #[test]
    fn test_frame_size_validation() {
        // Valid YUYV frame
        let valid_data = vec![0u8; 640 * 480 * 2];
        let valid_frame = FrameData::new(
            1,
            SystemTime::now(),
            valid_data,
            640,
            480,
            FrameFormat::Yuyv,
        );
        assert!(valid_frame.validate_size());
        
        // Invalid YUYV frame (wrong size)
        let invalid_data = vec![0u8; 100];
        let invalid_frame = FrameData::new(
            2,
            SystemTime::now(),
            invalid_data,
            640,
            480,
            FrameFormat::Yuyv,
        );
        assert!(!invalid_frame.validate_size());
        
        // MJPEG frame (compressed, always valid)
        let mjpeg_data = vec![0u8; 5000];
        let mjpeg_frame = FrameData::new(
            3,
            SystemTime::now(),
            mjpeg_data,
            640,
            480,
            FrameFormat::Mjpeg,
        );
        assert!(mjpeg_frame.validate_size());
    }
    
    #[tokio::test]
    async fn test_frame_age() {
        let past_time = SystemTime::now() - Duration::from_millis(100);
        let frame = FrameData::new(
            1,
            past_time,
            vec![0u8; 100],
            640,
            480,
            FrameFormat::Mjpeg,
        );
        
        // Frame should be older than 50ms
        assert!(frame.is_older_than(Duration::from_millis(50)));
        // Frame should not be older than 200ms
        assert!(!frame.is_older_than(Duration::from_millis(200)));
    }
    
    #[test]
    fn test_processed_frame() {
        let frame = FrameData::new(
            1,
            SystemTime::now(),
            vec![0u8; 100],
            640,
            480,
            FrameFormat::Mjpeg,
        );
        
        let processed = ProcessedFrame::new(frame);
        
        // Should return original data when no processing is done
        assert_eq!(
            processed.get_data_for_purpose(FramePurpose::Display).len(),
            100
        );
        assert_eq!(
            processed.get_data_for_purpose(FramePurpose::Streaming).len(),
            100
        );
    }
}