use crate::config::AnalyzerConfig;
#[cfg(feature = "motion_analysis")]
use crate::error::AnalyzerError;
use crate::error::Result;
use crate::events::{DoorcamEvent, EventBus};
use crate::frame::FrameData;
#[cfg(feature = "motion_analysis")]
use crate::frame::FrameFormat;

use tracing::{debug, error, info, warn};

#[cfg(all(feature = "motion_analysis", target_os = "linux"))]
use gstreamer::prelude::*;
#[cfg(all(feature = "motion_analysis", target_os = "linux"))]
use gstreamer::Pipeline;
#[cfg(all(feature = "motion_analysis", target_os = "linux"))]
use gstreamer_app::{AppSink, AppSrc};

#[cfg(feature = "motion_analysis")]
use image::{GrayImage, ImageBuffer, Luma, RgbImage};
#[cfg(feature = "motion_analysis")]
use imageproc::{
    contrast::threshold,
    distance_transform::Norm,
    filter::gaussian_blur_f32,
    morphology::{dilate, erode},
    region_labelling::{connected_components, Connectivity},
};

/// Simple motion detection state for fallback implementation
#[cfg(not(feature = "motion_analysis"))]
struct SimpleMotionState {
    frame_count: u64,
    last_motion_time: Option<std::time::SystemTime>,
}

/// Motion detection analyzer with GStreamer preprocessing and imageproc analysis
pub struct MotionAnalyzer {
    config: AnalyzerConfig,
    #[cfg(feature = "motion_analysis")]
    background_model: Option<GrayImage>,
    #[cfg(feature = "motion_analysis")]
    frame_count: u64,
    #[cfg(not(feature = "motion_analysis"))]
    simple_state: SimpleMotionState,
    #[cfg(all(feature = "motion_analysis", target_os = "linux"))]
    preprocessing_pipeline: Option<Pipeline>,
    #[cfg(all(feature = "motion_analysis", target_os = "linux"))]
    appsrc: Option<AppSrc>,
    #[cfg(all(feature = "motion_analysis", target_os = "linux"))]
    appsink: Option<AppSink>,
    #[cfg(all(feature = "motion_analysis", target_os = "linux"))]
    preprocessing_dims: Option<(u32, u32)>,
}

impl MotionAnalyzer {
    /// Create a new motion analyzer with the given configuration
    pub async fn new(config: AnalyzerConfig) -> Result<Self> {
        info!(
            "Initializing GStreamer-enhanced motion analyzer with config: {:?}",
            config
        );

        #[cfg(feature = "motion_analysis")]
        {
            #[cfg(target_os = "linux")]
            {
                // Initialize GStreamer for preprocessing
                if let Err(e) = gstreamer::init() {
                    warn!("Failed to initialize GStreamer for motion analysis: {}", e);
                }
            }

            let analyzer = Self {
                config,
                background_model: None,
                frame_count: 0,
                #[cfg(all(feature = "motion_analysis", target_os = "linux"))]
                preprocessing_pipeline: None,
                #[cfg(all(feature = "motion_analysis", target_os = "linux"))]
                appsrc: None,
                #[cfg(all(feature = "motion_analysis", target_os = "linux"))]
                appsink: None,
                #[cfg(all(feature = "motion_analysis", target_os = "linux"))]
                preprocessing_dims: None,
            };

            info!("Motion analyzer initialized successfully");
            Ok(analyzer)
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

    /// Initialize GStreamer preprocessing pipeline for motion analysis with target dimensions
    #[cfg(all(feature = "motion_analysis", target_os = "linux"))]
    fn initialize_preprocessing_pipeline(
        &mut self,
        target_width: u32,
        target_height: u32,
    ) -> Result<()> {
        info!(
            "Initializing GStreamer pipeline for motion analysis at {}x{} (scale 1/{})",
            target_width, target_height, self.config.jpeg_decode_scale
        );

        // Define hardware pipeline with direct downscaling if supported
        let hw_pipeline_desc = format!(
            "appsrc name=src format=time is-live=true caps=image/jpeg ! \
             v4l2jpegdec ! \
             v4l2convert ! \
             video/x-raw,format=GRAY8,width={},height={} ! \
             appsink name=sink sync=false max-buffers=1 drop=true",
            target_width, target_height
        );

        // Software pipeline - decode JPEG and scale down for motion analysis
        // Note: jpegdec doesn't support idct-method property, so we decode at full resolution
        // and then scale down using videoscale for better performance
        let sw_pipeline_desc = format!(
            "appsrc name=src format=time is-live=true caps=image/jpeg ! \
             jpegdec ! \
             videoconvert ! \
             video/x-raw,format=GRAY8 ! \
             videoscale method=0 ! \
             video/x-raw,format=GRAY8,width={},height={} ! \
             appsink name=sink sync=false max-buffers=1 drop=true",
            target_width, target_height
        );

        // Attempt to use hardware acceleration and fall back to software if unavailable
        let pipeline = match gstreamer::parse::launch(&hw_pipeline_desc) {
            Ok(pipeline) => {
                info!("Hardware-accelerated GStreamer pipeline created successfully");
                pipeline
            }
            Err(e) => {
                warn!(
                    "Hardware acceleration not available ({}), falling back to software pipeline",
                    e
                );
                debug!(
                    "Creating software motion analysis pipeline: {}",
                    sw_pipeline_desc
                );

                gstreamer::parse::launch(&sw_pipeline_desc).map_err(|e| {
                    AnalyzerError::FrameProcessing {
                        details: format!("Failed to create software pipeline: {}", e),
                    }
                })?
            }
        };

        let pipeline =
            pipeline
                .downcast::<Pipeline>()
                .map_err(|_| AnalyzerError::FrameProcessing {
                    details: "Failed to downcast to Pipeline".to_string(),
                })?;

        let appsrc = pipeline
            .by_name("src")
            .ok_or_else(|| AnalyzerError::FrameProcessing {
                details: "Failed to get appsrc element".to_string(),
            })?
            .downcast::<AppSrc>()
            .map_err(|_| AnalyzerError::FrameProcessing {
                details: "Failed to downcast to AppSrc".to_string(),
            })?;

        let appsink = pipeline
            .by_name("sink")
            .ok_or_else(|| AnalyzerError::FrameProcessing {
                details: "Failed to get appsink element".to_string(),
            })?
            .downcast::<AppSink>()
            .map_err(|_| AnalyzerError::FrameProcessing {
                details: "Failed to downcast to AppSink".to_string(),
            })?;

        // Configure elements
        appsrc.set_property("format", gstreamer::Format::Time);
        appsrc.set_property("is-live", true);

        // Start pipeline
        pipeline.set_state(gstreamer::State::Playing).map_err(|e| {
            AnalyzerError::FrameProcessing {
                details: format!("Failed to start preprocessing pipeline: {}", e),
            }
        })?;

        self.preprocessing_pipeline = Some(pipeline);
        self.appsrc = Some(appsrc);
        self.appsink = Some(appsink);
        self.preprocessing_dims = Some((target_width, target_height));

        info!("GStreamer preprocessing pipeline initialized for motion analysis");
        Ok(())
    }

    /// Ensure preprocessing pipeline is available and sized correctly
    #[cfg(all(feature = "motion_analysis", target_os = "linux"))]
    fn ensure_preprocessing_pipeline(
        &mut self,
        target_width: u32,
        target_height: u32,
    ) -> Result<()> {
        if let Some((w, h)) = self.preprocessing_dims {
            if w == target_width
                && h == target_height
                && self.preprocessing_pipeline.is_some()
                && self.appsrc.is_some()
                && self.appsink.is_some()
            {
                return Ok(());
            }
        }

        // Rebuild to match the requested dimensions
        self.cleanup();
        self.initialize_preprocessing_pipeline(target_width, target_height)
    }

    /// Analyze a single frame for motion (async wrapper)
    pub async fn analyze_frame(
        &mut self,
        frame: &FrameData,
        event_bus: &EventBus,
    ) -> Result<Option<f64>> {
        let motion_result = self.analyze_frame_sync(frame)?;

        if let Some(motion_area) = motion_result {
            // Publish motion detection event
            if let Err(e) = event_bus
                .publish(DoorcamEvent::MotionDetected {
                    contour_area: motion_area,
                    timestamp: frame.timestamp,
                })
                .await
            {
                error!("Failed to publish motion detection event: {}", e);
            }
        }

        Ok(motion_result)
    }

    /// Analyze a single frame for motion (synchronous)
    pub fn analyze_frame_sync(&mut self, frame: &FrameData) -> Result<Option<f64>> {
        match self.detect_motion_sync(frame) {
            Ok(Some(motion_area)) => {
                if motion_area > self.config.contour_minimum_area {
                    info!("Motion detected: area = {:.2} pixels", motion_area);
                    Ok(Some(motion_area))
                } else {
                    debug!(
                        "Motion area {:.2} below threshold {:.2}",
                        motion_area, self.config.contour_minimum_area
                    );
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
            debug!(
                "Analyzing frame {} for motion ({}x{}, {:?})",
                frame.id, frame.width, frame.height, frame.format
            );

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
            let cleaned_mask = dilate(
                &erode(&binary_mask, Norm::LInf, kernel_size),
                Norm::LInf,
                kernel_size,
            );

            // Find connected components (contours)
            let components = connected_components(&cleaned_mask, Connectivity::Eight, Luma([0u8]));

            // Calculate the largest component area
            let max_area = self.calculate_largest_component_area(&components);

            // Update background model (simple running average)
            self.update_background_model(&blurred);
            self.frame_count += 1;

            debug!(
                "Motion analysis complete: max component area = {:.2}",
                max_area
            );
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
                self.simple_state.last_motion_time = Some(std::time::SystemTime::now());
                Ok(Some(simulated_area))
            } else {
                debug!("No simulated motion detected");
                Ok(None)
            }
        }
    }

    /// Convert frame data to grayscale image using GStreamer preprocessing when available
    #[cfg(feature = "motion_analysis")]
    fn frame_to_gray_image(&mut self, frame: &FrameData) -> Result<GrayImage> {
        #[cfg(target_os = "linux")]
        {
            // Try GStreamer preprocessing first for MJPEG frames
            if frame.format == FrameFormat::Mjpeg {
                if let Ok(gray_image) = self.frame_to_gray_gstreamer(frame) {
                    return Ok(gray_image);
                } else {
                    debug!("GStreamer preprocessing failed, falling back to direct processing");
                }
            }
        }

        // Fallback to direct processing
        match frame.format {
            FrameFormat::Mjpeg => {
                // Decode MJPEG data
                let dynamic_image = image::load_from_memory(&frame.data).map_err(|e| {
                    AnalyzerError::FrameProcessing {
                        details: format!("MJPEG decode failed: {}", e),
                    }
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

    /// Convert MJPEG frame to grayscale using GStreamer preprocessing
    #[cfg(all(feature = "motion_analysis", target_os = "linux"))]
    fn frame_to_gray_gstreamer(&mut self, frame: &FrameData) -> Result<GrayImage> {
        let scale = std::cmp::max(1, self.config.jpeg_decode_scale);
        let mut target_width = std::cmp::max(1, frame.width / scale);
        let mut target_height = std::cmp::max(1, frame.height / scale);

        // Align to even dimensions for better pipeline compatibility
        if target_width % 2 != 0 && target_width > 1 {
            target_width -= 1;
        }
        if target_height % 2 != 0 && target_height > 1 {
            target_height -= 1;
        }

        self.ensure_preprocessing_pipeline(target_width, target_height)?;

        if let (Some(appsrc), Some(appsink)) = (&self.appsrc, &self.appsink) {
            // Create GStreamer buffer from JPEG data
            let mut buffer = gstreamer::Buffer::with_size(frame.data.len()).map_err(|e| {
                AnalyzerError::FrameProcessing {
                    details: format!("Failed to create GStreamer buffer: {}", e),
                }
            })?;

            {
                let buffer_ref = buffer.get_mut().unwrap();
                let mut map =
                    buffer_ref
                        .map_writable()
                        .map_err(|e| AnalyzerError::FrameProcessing {
                            details: format!("Failed to map buffer: {}", e),
                        })?;
                map.copy_from_slice(frame.data.as_ref());
            }

            // Set timestamp for proper pipeline timing
            buffer
                .get_mut()
                .unwrap()
                .set_pts(gstreamer::ClockTime::from_nseconds(
                    frame
                        .timestamp
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as u64,
                ));

            // Push buffer to pipeline
            appsrc
                .push_buffer(buffer)
                .map_err(|e| AnalyzerError::FrameProcessing {
                    details: format!("Failed to push buffer to preprocessing pipeline: {:?}", e),
                })?;

            // Pull processed sample
            if let Ok(sample) = appsink.pull_sample() {
                let buffer = sample
                    .buffer()
                    .ok_or_else(|| AnalyzerError::FrameProcessing {
                        details: "No buffer in processed sample".to_string(),
                    })?;

                let map = buffer
                    .map_readable()
                    .map_err(|e| AnalyzerError::FrameProcessing {
                        details: format!("Failed to map processed buffer: {}", e),
                    })?;

                // Create grayscale image from processed data at the scaled resolution
                let gray_image =
                    GrayImage::from_raw(target_width, target_height, map.as_slice().to_vec())
                        .ok_or_else(|| AnalyzerError::FrameProcessing {
                            details: "Failed to create grayscale image from processed data"
                                .to_string(),
                        })?;

                debug!(
                    "Successfully preprocessed frame {} using GStreamer 1/{} resolution decode ({}x{})", 
                    frame.id, self.config.jpeg_decode_scale, target_width, target_height
                );
                return Ok(gray_image);
            } else {
                return Err(AnalyzerError::FrameProcessing {
                    details: "No sample available from GStreamer pipeline".to_string(),
                }
                .into());
            }
        }

        Err(AnalyzerError::FrameProcessing {
            details: "GStreamer preprocessing not available".to_string(),
        }
        .into())
    }

    /// Convert YUYV frame to grayscale
    #[cfg(feature = "motion_analysis")]
    fn yuyv_to_gray(&self, frame: &FrameData) -> Result<GrayImage> {
        let width = frame.width;
        let height = frame.height;
        let mut gray_image = GrayImage::new(width, height);

        // YUYV format: Y0 U Y1 V (4 bytes for 2 pixels)
        for y in 0..height {
            for x in 0..(width / 2) {
                let base_idx = ((y * width / 2 + x) * 4) as usize;
                if base_idx + 3 < frame.data.len() {
                    let y0 = frame.data[base_idx]; // First pixel Y
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
        let width = frame.width;
        let height = frame.height;

        // Create RGB image from raw data
        let rgb_image =
            RgbImage::from_raw(width, height, frame.data.to_vec()).ok_or_else(|| {
                AnalyzerError::FrameProcessing {
                    details: "Failed to create RGB image from raw data".to_string(),
                }
            })?;

        // Convert to grayscale using standard luminance formula
        let mut gray_image = GrayImage::new(width, height);
        for (x, y, rgb) in rgb_image.enumerate_pixels() {
            let gray_value =
                (0.299 * rgb[0] as f32 + 0.587 * rgb[1] as f32 + 0.114 * rgb[2] as f32) as u8;
            gray_image.put_pixel(x, y, Luma([gray_value]));
        }

        Ok(gray_image)
    }

    /// Calculate frame difference between background and current frame
    #[cfg(feature = "motion_analysis")]
    fn calculate_frame_difference(
        &self,
        background: &GrayImage,
        current: &GrayImage,
    ) -> Result<GrayImage> {
        let (width, height) = background.dimensions();
        let mut diff_image = GrayImage::new(width, height);

        for (x, y, bg_pixel) in background.enumerate_pixels() {
            if let Some(curr_pixel) = current.get_pixel_checked(x, y) {
                let diff = (bg_pixel[0] as i16 - curr_pixel[0] as i16).unsigned_abs() as u8;
                diff_image.put_pixel(x, y, Luma([diff]));
            }
        }

        Ok(diff_image)
    }

    /// Calculate the area of the largest connected component
    #[cfg(feature = "motion_analysis")]
    fn calculate_largest_component_area(
        &self,
        components: &ImageBuffer<Luma<u32>, Vec<u32>>,
    ) -> f64 {
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

    /// Cleanup GStreamer resources
    #[cfg(all(feature = "motion_analysis", target_os = "linux"))]
    pub fn cleanup(&mut self) {
        if let Some(ref pipeline) = self.preprocessing_pipeline {
            debug!("Stopping GStreamer preprocessing pipeline");
            if let Err(e) = pipeline.set_state(gstreamer::State::Null) {
                warn!("Failed to stop GStreamer pipeline cleanly: {}", e);
            }
        }
        self.preprocessing_pipeline = None;
        self.appsrc = None;
        self.appsink = None;
        self.preprocessing_dims = None;
        debug!("GStreamer preprocessing pipeline cleaned up");
    }

    #[cfg(not(all(feature = "motion_analysis", target_os = "linux")))]
    pub fn cleanup(&mut self) {
        // No-op for non-GStreamer builds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::FrameData;

    #[tokio::test]
    async fn test_motion_analyzer_creation() {
        let config = AnalyzerConfig {
            fps: 5,
            delta_threshold: 25,
            contour_minimum_area: 1000.0,
            jpeg_decode_scale: 4,
        };

        let analyzer = MotionAnalyzer::new(config).await;
        assert!(analyzer.is_ok());

        let analyzer = analyzer.unwrap();
        assert_eq!(analyzer.config().fps, 5);
        assert_eq!(analyzer.config().contour_minimum_area, 1000.0);
    }

    #[tokio::test]
    async fn test_config_update() {
        let initial_config = AnalyzerConfig {
            fps: 5,
            delta_threshold: 25,
            contour_minimum_area: 1000.0,
            jpeg_decode_scale: 4,
        };

        let mut analyzer = MotionAnalyzer::new(initial_config).await.unwrap();

        let new_config = AnalyzerConfig {
            fps: 10,
            delta_threshold: 30,
            contour_minimum_area: 2000.0,
            jpeg_decode_scale: 2,
        };

        analyzer.update_config(new_config);
        assert_eq!(analyzer.config().fps, 10);
        assert_eq!(analyzer.config().contour_minimum_area, 2000.0);
    }

    #[cfg(feature = "motion_analysis")]
    #[tokio::test]
    async fn test_motion_detection_with_synthetic_frame() {
        let config = AnalyzerConfig {
            fps: 5,
            delta_threshold: 25,
            contour_minimum_area: 100.0,
            jpeg_decode_scale: 4,
        };

        let mut analyzer = MotionAnalyzer::new(config).await.unwrap();

        // Create a simple RGB24 frame instead of MJPEG to avoid decoding issues
        let width = 64;
        let height = 48;
        let rgb_data = vec![128u8; (width * height * 3) as usize]; // Gray image

        let frame = FrameData::new(
            1,
            std::time::SystemTime::now(),
            rgb_data,
            width,
            height,
            FrameFormat::Rgb24,
        );

        // Use timeout to prevent hanging
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::task::spawn_blocking(move || analyzer.detect_motion_sync(&frame)),
        )
        .await;

        match result {
            Ok(Ok(motion_result)) => {
                // Motion detection completed successfully
                match motion_result {
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
            Ok(Err(_)) => {
                panic!("Task panicked during motion detection");
            }
            Err(_) => {
                panic!("Motion detection timed out - this indicates a hanging operation");
            }
        }
    }
}
