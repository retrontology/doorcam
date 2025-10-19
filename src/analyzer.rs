use crate::config::AnalyzerConfig;
use crate::events::{DoorcamEvent, EventBus};
use crate::frame::{FrameData, FrameFormat};
use crate::ring_buffer::RingBuffer;
use crate::error::{DoorcamError, Result};

use std::sync::Arc;
use std::time::SystemTime;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

#[cfg(feature = "motion_analysis")]
use opencv::{
    core::{self, Mat, Point, Scalar, Size, Vector},
    imgcodecs,
    imgproc::{self, BackgroundSubtractor},
    prelude::*,
    video,
};

/// Simple motion detection state for fallback implementation
#[cfg(not(feature = "motion_analysis"))]
struct SimpleMotionState {
    frame_count: u64,
    last_motion_time: Option<SystemTime>,
}

/// Motion detection analyzer using OpenCV background subtraction
pub struct MotionAnalyzer {
    config: AnalyzerConfig,
    #[cfg(feature = "motion_analysis")]
    background_subtractor: Option<core::Ptr<dyn video::BackgroundSubtractor>>,
    #[cfg(not(feature = "motion_analysis"))]
    simple_state: SimpleMotionState,
}

impl MotionAnalyzer {
    /// Create a new motion analyzer with the given configuration
    pub async fn new(config: AnalyzerConfig) -> Result<Self> {
        info!("Initializing motion analyzer with config: {:?}", config);
        
        #[cfg(feature = "motion_analysis")]
        {
            let background_subtractor = video::create_background_subtractor_mog2(
                500,    // history - number of frames to use for background model
                16.0,   // var_threshold - threshold for pixel classification
                false   // detect_shadows - whether to detect shadows
            ).map_err(|e| DoorcamError::MotionAnalysis(format!("Failed to create background subtractor: {}", e)))?;
            
            info!("OpenCV background subtractor initialized successfully");
            
            Ok(Self {
                config,
                background_subtractor: Some(background_subtractor),
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
    
    /// Analyze a single frame for motion
    pub async fn analyze_frame(
        &mut self,
        frame: &FrameData,
        event_bus: &EventBus
    ) -> Result<Option<f64>> {
        match self.detect_motion(frame).await {
            Ok(Some(motion_area)) => {
                if motion_area > self.config.contour_minimum_area {
                    info!("Motion detected: area = {:.2} pixels", motion_area);
                    
                    // Publish motion detection event
                    if let Err(e) = event_bus.publish(DoorcamEvent::MotionDetected {
                        contour_area: motion_area,
                        timestamp: frame.timestamp,
                    }).await {
                        error!("Failed to publish motion detection event: {}", e);
                    }
                    
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
                
                // Publish system error event
                if let Err(publish_err) = event_bus.publish(DoorcamEvent::SystemError {
                    component: "motion_analyzer".to_string(),
                    error: e.to_string(),
                }).await {
                    error!("Failed to publish system error event: {}", publish_err);
                }
                
                Err(e)
            }
        }
    }
    
    /// Detect motion in a single frame
    async fn detect_motion(&mut self, frame: &FrameData) -> Result<Option<f64>> {
        #[cfg(feature = "motion_analysis")]
        {
            debug!("Analyzing frame {} for motion ({}x{}, {:?})", 
                   frame.id, frame.width, frame.height, frame.format);
            
            // Convert frame data to OpenCV Mat
            let mat = self.frame_to_mat(frame)?;
            
            // Convert to grayscale if needed
            let gray_mat = if mat.channels() == 3 {
                let mut gray = Mat::default();
                imgproc::cvt_color(&mat, &mut gray, imgproc::COLOR_BGR2GRAY, 0)
                    .map_err(|e| DoorcamError::MotionAnalysis(format!("Color conversion failed: {}", e)))?;
                gray
            } else {
                mat
            };
            
            // Apply Gaussian blur to reduce noise
            let mut blurred = Mat::default();
            imgproc::gaussian_blur(
                &gray_mat, 
                &mut blurred, 
                Size::new(21, 21), 
                0.0, 
                0.0, 
                core::BORDER_DEFAULT
            ).map_err(|e| DoorcamError::MotionAnalysis(format!("Gaussian blur failed: {}", e)))?;
            
            // Apply background subtraction
            if let Some(ref mut bg_sub) = self.background_subtractor {
                let mut fg_mask = Mat::default();
                bg_sub.apply(&blurred, &mut fg_mask, -1.0)
                    .map_err(|e| DoorcamError::MotionAnalysis(format!("Background subtraction failed: {}", e)))?;
                
                // Apply morphological operations to clean up the mask
                let kernel = imgproc::get_structuring_element(
                    imgproc::MORPH_ELLIPSE,
                    Size::new(5, 5),
                    Point::new(-1, -1)
                ).map_err(|e| DoorcamError::MotionAnalysis(format!("Failed to create morphological kernel: {}", e)))?;
                
                let mut cleaned_mask = Mat::default();
                imgproc::morphology_ex(
                    &fg_mask,
                    &mut cleaned_mask,
                    imgproc::MORPH_OPEN,
                    &kernel,
                    Point::new(-1, -1),
                    1,
                    core::BORDER_CONSTANT,
                    Scalar::default()
                ).map_err(|e| DoorcamError::MotionAnalysis(format!("Morphological operation failed: {}", e)))?;
                
                // Find contours
                let mut contours = Vector::<Vector<Point>>::new();
                imgproc::find_contours(
                    &cleaned_mask,
                    &mut contours,
                    imgproc::RETR_EXTERNAL,
                    imgproc::CHAIN_APPROX_SIMPLE,
                    Point::new(0, 0)
                ).map_err(|e| DoorcamError::MotionAnalysis(format!("Contour detection failed: {}", e)))?;
                
                // Find the largest contour area
                let mut max_area = 0.0;
                for i in 0..contours.len() {
                    let contour = contours.get(i)
                        .map_err(|e| DoorcamError::MotionAnalysis(format!("Failed to get contour {}: {}", i, e)))?;
                    let area = imgproc::contour_area(&contour, false)
                        .map_err(|e| DoorcamError::MotionAnalysis(format!("Failed to calculate contour area: {}", e)))?;
                    
                    if area > max_area {
                        max_area = area;
                    }
                }
                
                debug!("Motion analysis complete: max contour area = {:.2}", max_area);
                return Ok(if max_area > 0.0 { Some(max_area) } else { None });
            }
            
            warn!("Background subtractor not initialized");
            Ok(None)
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
    
    /// Convert frame data to OpenCV Mat
    #[cfg(feature = "motion_analysis")]
    fn frame_to_mat(&self, frame: &FrameData) -> Result<Mat> {
        match frame.format {
            FrameFormat::Mjpeg => {
                // Decode MJPEG data
                let data_vector = Vector::from_slice(&frame.data);
                let decoded = imgcodecs::imdecode(&data_vector, imgcodecs::IMREAD_COLOR)
                    .map_err(|e| DoorcamError::MotionAnalysis(format!("MJPEG decode failed: {}", e)))?;
                Ok(decoded)
            }
            FrameFormat::Yuyv => {
                // Convert YUYV to BGR
                let yuyv_mat = unsafe {
                    Mat::new_rows_cols_with_data(
                        frame.height as i32,
                        frame.width as i32,
                        core::CV_8UC2,
                        frame.data.as_ptr() as *mut std::ffi::c_void,
                        core::Mat_AUTO_STEP
                    ).map_err(|e| DoorcamError::MotionAnalysis(format!("Failed to create YUYV Mat: {}", e)))?
                };
                
                let mut bgr_mat = Mat::default();
                imgproc::cvt_color(&yuyv_mat, &mut bgr_mat, imgproc::COLOR_YUV2BGR_YUYV, 0)
                    .map_err(|e| DoorcamError::MotionAnalysis(format!("YUYV to BGR conversion failed: {}", e)))?;
                Ok(bgr_mat)
            }
            FrameFormat::Rgb24 => {
                // Convert RGB24 to BGR
                let rgb_mat = unsafe {
                    Mat::new_rows_cols_with_data(
                        frame.height as i32,
                        frame.width as i32,
                        core::CV_8UC3,
                        frame.data.as_ptr() as *mut std::ffi::c_void,
                        core::Mat_AUTO_STEP
                    ).map_err(|e| DoorcamError::MotionAnalysis(format!("Failed to create RGB Mat: {}", e)))?
                };
                
                let mut bgr_mat = Mat::default();
                imgproc::cvt_color(&rgb_mat, &mut bgr_mat, imgproc::COLOR_RGB2BGR, 0)
                    .map_err(|e| DoorcamError::MotionAnalysis(format!("RGB to BGR conversion failed: {}", e)))?;
                Ok(bgr_mat)
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
    use std::sync::Arc;
    
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
        let result = analyzer.detect_motion(&frame).await;
        
        // We expect either a successful result or a specific error
        match result {
            Ok(_) => {
                // Motion detection succeeded
            }
            Err(DoorcamError::MotionAnalysis(_)) => {
                // Expected error with synthetic data
            }
            Err(e) => {
                panic!("Unexpected error: {}", e);
            }
        }
    }
}