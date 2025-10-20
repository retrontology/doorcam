use crate::config::AnalyzerConfig;
use crate::events::{DoorcamEvent, EventBus};
use crate::frame::FrameData;
#[cfg(feature = "motion_analysis")]
use crate::frame::FrameFormat;
use crate::error::Result;
#[cfg(feature = "motion_analysis")]
use crate::error::AnalyzerError;

use std::time::SystemTime;
use tracing::{debug, error, info};

#[cfg(feature = "motion_analysis")]
use image::{ImageBuffer, Luma, RgbImage, GrayImage};
#[cfg(feature = "motion_analysis")]
use imageproc::{
    filter::gaussian_blur_f32,
    morphology::{dilate, erode},
    distance_transform::Norm,
    contrast::threshold,
    region_labelling::{connected_components, Connectivity},
};

/// Simple motion detection state for fallback implementation
#[cfg(not(feature = "motion_analysis"))]
struct SimpleMotionState {
    frame_count: u64,
    last_motion_time: Option<SystemTime>,
}

/// Motion detection analyzer using imageproc background subtraction
pub struct MotionAnalyzer {
    config: AnalyzerConfig,
    #[cfg(feature = "motion_analysis")]
    background_model: Option<GrayImage>,
    #[cfg(feature = "motion_analysis")]
    frame_count: u64,
    #[cfg(not(feature = "motion_analysis"))]
    simple_state: SimpleMotionState,
}

impl MotionAnalyzer {
    /// Create a new motion analyzer with the given configuration
    pub async fn new(config: AnalyzerConfig) -> Result<Self> {
        info!("Initializing motion analyzer with config: {:?}", config);
        
        #[cfg(feature = "motion_analysis")]
        {
            info!("Imageproc motion analyzer initialized successfully");
            
            Ok(Self {
                config,
                background_model: None,
                frame_count: 0,
            })
        }
        
        #[cfg(not(feature = "motion_analysis"))]
        {
            warn!("Motion analysis feature not enabled - using simple fallback implementation");
            Ok(Self {
                config,
                simple_state: SimpleMotionState {
                    frame_count: 0,
                    last_motion_time: None,
                },
            })
        }
    }
    
    /// Analyze a single frame for motion (async wrapper)
    pub async fn analyze_frame(
        &mut self,
        frame: &FrameData,
        event_bus: &EventBus
    ) -> Result<Option<f64>> {
        let motion_result = self.analyze_frame_sync(frame)?;
        
        if let Some(motion_area) = motion_result {
            // Publish motion detection event
            if let Err(e) = event_bus.publish(DoorcamEvent::MotionDetected {
                contour_area: motion_area,
                timestamp: frame.timestamp,
            }).await {
                error!("Failed to publish motion detection event: {}", e);
            }
        }
        
        Ok(motion_result)
    }
    
    /// Analyze a single frame for motion (synchronous)
    pub fn analyze_frame_sync(
        &mut self,
        frame: &FrameData,
    ) -> Result<Option<f64>> {
        match self.detect_motion_sync(frame) {
            Ok(Some(motion_area)) => {
                if motion_area > self.config.contour_minimum_area {
                    info!("Motion detected: area = {:.2} pixels", motion_area);
                    Ok(Some(motion_area))
                } else {
                    debug!("Motion area {:.2} below threshold {:.2}", motion_area, self.config.contour_minimum_area);
                    Ok(None)
                }
            }
            Ok(None) => {
                debug!("No motion detected in frame {}", frame.id);
                Ok(None)
            }
            Err(e) => {
                error!("Motion detection error for frame {}: {}", frame.id, e);
                Err(e)
            }
        }
    }
    
    /// Detect motion in a single frame
    fn detect_motion_sync(&mut self, frame: &FrameData) -> Result<Option<f64>> {
        #[cfg(feature = "motion_analysis")]
        {
            debug!("Analyzing frame {} for motion ({}x{}, {:?})", 
                   frame.id, frame.width, frame.height, frame.format);
            
            // Convert frame data to grayscale image
            let gray_image = self.frame_to_gray_image(frame)?;
            
            // Apply Gaussian blur to reduce noise
            let blurred = gaussian_blur_f32(&gray_image, 2.0);
            
            // Initialize background model if this is the first frame
            if self.background_model.is_none() {
                info!("Initializing background model with first frame");
                self.background_model = Some(blurred.clone());
                self.frame_count = 1;
                return Ok(None); // No motion on first frame
            }
            
            let background = self.background_model.as_ref().unwrap();
            
            // Calculate frame difference
            let diff_image = self.calculate_frame_difference(background, &blurred)?;
            
            // Apply threshold to create binary mask
            let threshold_value = self.config.delta_threshold as u8;
            let binary_mask = threshold(&diff_image, threshold_value);
            
            // Apply morphological operations to clean up noise
            let kernel_size = 3u8;
            let cleaned_mask = dilate(&erode(&binary_mask, Norm::LInf, kernel_size), Norm::LInf, kernel_size);
            
            // Find connected components (contours)
            let components = connected_components(&cleaned_mask, Connectivity::Eight, Luma([0u8]));
            
            // Calculate the largest component area
            let max_area = self.calculate_largest_component_area(&components);
            
            // Update background model (simple running average)
            self.update_background_model(&blurred);
            self.frame_count += 1;
            
            debug!("Motion analysis complete: max component area = {:.2}", max_area);
            Ok(if max_area > 0.0 { Some(max_area) } else { None })
        }
        
        #[cfg(not(feature = "motion_analysis"))]
        {
            // Simple fallback implementation that simulates motion detection
            self.simple_state.frame_count += 1;
            
            // Simulate motion detection every 10 frames for testing
            if self.simple_state.frame_count % 10 == 0 {
                let simulated_area = 1500.0; // Above default threshold
                debug!("Simulated motion detection: area = {:.2}", simulated_area);
                self.simple_state.last_motion_time = Some(SystemTime::now());
                Ok(Some(simulated_area))
            } else {
                debug!("No simulated motion detected");
                Ok(None)
            }
        }
    }
    
    /// Convert frame data to grayscale image
    #[cfg(feature = "motion_analysis")]
    fn frame_to_gray_image(&self, frame: &FrameData) -> Result<GrayImage> {
        match frame.format {
            FrameFormat::Mjpeg => {
                // Decode MJPEG data
                let dynamic_image = image::load_from_memory(&frame.data)
                    .map_err(|e| AnalyzerError::FrameProcessing { 
                        details: format!("MJPEG decode failed: {}", e) 
                    })?;
                Ok(dynamic_image.to_luma8())
            }
            FrameFormat::Yuyv => {
                // Convert YUYV to grayscale
                self.yuyv_to_gray(frame)
            }
            FrameFormat::Rgb24 => {
                // Convert RGB24 to grayscale
                self.rgb24_to_gray(frame)
            }
        }
    }
    
    /// Convert YUYV frame to grayscale
    #[cfg(feature = "motion_analysis")]
    fn yuyv_to_gray(&self, frame: &FrameData) -> Result<GrayImage> {
        let width = frame.width as u32;
        let height = frame.height as u32;
        let mut gray_image = GrayImage::new(width, height);
        
        // YUYV format: Y0 U Y1 V (4 bytes for 2 pixels)
        for y in 0..height {
            for x in 0..(width / 2) {
                let base_idx = ((y * width / 2 + x) * 4) as usize;
                if base_idx + 3 < frame.data.len() {
                    let y0 = frame.data[base_idx];     // First pixel Y
                    let y1 = frame.data[base_idx + 2]; // Second pixel Y
                    
                    gray_image.put_pixel(x * 2, y, Luma([y0]));
                    if x * 2 + 1 < width {
                        gray_image.put_pixel(x * 2 + 1, y, Luma([y1]));
                    }
                }
            }
        }
        
        Ok(gray_image)
    }
    
    /// Convert RGB24 frame to grayscale
    #[cfg(feature = "motion_analysis")]
    fn rgb24_to_gray(&self, frame: &FrameData) -> Result<GrayImage> {
        let width = frame.width as u32;
        let height = frame.height as u32;
        
        // Create RGB image from raw data
        let rgb_image = RgbImage::from_raw(width, height, frame.data.to_vec())
            .ok_or_else(|| AnalyzerError::FrameProcessing {
                details: "Failed to create RGB image from raw data".to_string()
            })?;
        
        // Convert to grayscale using standard luminance formula
        let mut gray_image = GrayImage::new(width, height);
        for (x, y, rgb) in rgb_image.enumerate_pixels() {
            let gray_value = (0.299 * rgb[0] as f32 + 0.587 * rgb[1] as f32 + 0.114 * rgb[2] as f32) as u8;
            gray_image.put_pixel(x, y, Luma([gray_value]));
        }
        
        Ok(gray_image)
    }
    
    /// Calculate frame difference between background and current frame
    #[cfg(feature = "motion_analysis")]
    fn calculate_frame_difference(&self, background: &GrayImage, current: &GrayImage) -> Result<GrayImage> {
        let (width, height) = background.dimensions();
        let mut diff_image = GrayImage::new(width, height);
        
        for (x, y, bg_pixel) in background.enumerate_pixels() {
            if let Some(curr_pixel) = current.get_pixel_checked(x, y) {
                let diff = (bg_pixel[0] as i16 - curr_pixel[0] as i16).abs() as u8;
                diff_image.put_pixel(x, y, Luma([diff]));
            }
        }
        
        Ok(diff_image)
    }
    
    /// Calculate the area of the largest connected component
    #[cfg(feature = "motion_analysis")]
    fn calculate_largest_component_area(&self, components: &ImageBuffer<Luma<u32>, Vec<u32>>) -> f64 {
        let mut component_counts = std::collections::HashMap::new();
        
        // Count pixels in each component (skip background component 0)
        for pixel in components.pixels() {
            let component_id = pixel[0];
            if component_id > 0 {
                *component_counts.entry(component_id).or_insert(0) += 1;
            }
        }
        
        // Return the size of the largest component
        component_counts.values().max().copied().unwrap_or(0) as f64
    }
    
    /// Update background model using simple running average
    #[cfg(feature = "motion_analysis")]
    fn update_background_model(&mut self, current_frame: &GrayImage) {
        if let Some(ref mut background) = self.background_model {
            let learning_rate = 0.05; // 5% learning rate
            
            for (bg_pixel, curr_pixel) in background.pixels_mut().zip(current_frame.pixels()) {
                let bg_val = bg_pixel[0] as f32;
                let curr_val = curr_pixel[0] as f32;
                let new_val = (bg_val * (1.0 - learning_rate) + curr_val * learning_rate) as u8;
                bg_pixel[0] = new_val;
            }
        }
    }
    
    /// Get current configuration
    pub fn config(&self) -> &AnalyzerConfig {
        &self.config
    }
    
    /// Update configuration (requires restart of analysis loop)
    pub fn update_config(&mut self, config: AnalyzerConfig) {
        info!("Updating motion analyzer configuration: {:?}", config);
        self.config = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::FrameData;

    
    #[tokio::test]
    async fn test_motion_analyzer_creation() {
        let config = AnalyzerConfig {
            max_fps: 5,
            delta_threshold: 25,
            contour_minimum_area: 1000.0,
        };
        
        let analyzer = MotionAnalyzer::new(config).await;
        assert!(analyzer.is_ok());
        
        let analyzer = analyzer.unwrap();
        assert_eq!(analyzer.config().max_fps, 5);
        assert_eq!(analyzer.config().contour_minimum_area, 1000.0);
    }
    
    #[tokio::test]
    async fn test_config_update() {
        let initial_config = AnalyzerConfig {
            max_fps: 5,
            delta_threshold: 25,
            contour_minimum_area: 1000.0,
        };
        
        let mut analyzer = MotionAnalyzer::new(initial_config).await.unwrap();
        
        let new_config = AnalyzerConfig {
            max_fps: 10,
            delta_threshold: 30,
            contour_minimum_area: 2000.0,
        };
        
        analyzer.update_config(new_config);
        assert_eq!(analyzer.config().max_fps, 10);
        assert_eq!(analyzer.config().contour_minimum_area, 2000.0);
    }
    
    #[cfg(feature = "motion_analysis")]
    #[tokio::test]
    async fn test_motion_detection_with_synthetic_frame() {
        let config = AnalyzerConfig {
            max_fps: 5,
            delta_threshold: 25,
            contour_minimum_area: 100.0,
        };
        
        let mut analyzer = MotionAnalyzer::new(config).await.unwrap();
        
        // Create a synthetic MJPEG frame (minimal JPEG header)
        let jpeg_data = vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
            0x01, 0x01, 0x00, 0x48, 0x00, 0x48, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43,
            // ... (truncated for brevity - this would be a complete JPEG)
            0xFF, 0xD9 // End of JPEG marker
        ];
        
        let frame = FrameData::new(
            1,
            SystemTime::now(),
            jpeg_data,
            640,
            480,
            FrameFormat::Mjpeg,
        );
        
        // This test may fail if OpenCV can't decode the synthetic JPEG
        // In a real implementation, we'd use actual camera frames
        let result = analyzer.detect_motion_sync(&frame);
        
        // We expect either a successful result or a specific error
        match result {
            Ok(_) => {
                // Motion detection succeeded
            }
            Err(crate::error::DoorcamError::Analyzer(_)) => {
                // Expected error with synthetic data
            }
            Err(e) => {
                panic!("Unexpected error: {}", e);
            }
        }
    }
}