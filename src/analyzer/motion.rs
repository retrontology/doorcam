use crate::config::AnalyzerConfig;
use crate::error::{AnalyzerError, Result};
use crate::events::{DoorcamEvent, EventBus};
use crate::frame::{FrameData, FrameFormat};

use image::{GrayImage, ImageBuffer, Luma, RgbImage};
use imageproc::{
    contrast::threshold,
    distance_transform::Norm,
    filter::gaussian_blur_f32,
    morphology::{dilate, erode},
    region_labelling::{connected_components, Connectivity},
};
use tracing::{debug, error, info, warn};

/// Motion detection analyzer with GStreamer preprocessing and imageproc analysis
pub struct MotionAnalyzer {
    config: AnalyzerConfig,
    background_model: Option<GrayImage>,
    pub(crate) frame_count: u64,
    #[cfg(target_os = "linux")]
    preprocessing_pipeline: Option<gstreamer::Pipeline>,
    #[cfg(target_os = "linux")]
    pub(crate) appsrc: Option<gstreamer_app::AppSrc>,
    #[cfg(target_os = "linux")]
    pub(crate) appsink: Option<gstreamer_app::AppSink>,
    #[cfg(target_os = "linux")]
    preprocessing_dims: Option<(u32, u32)>,
}

impl MotionAnalyzer {
    /// Create a new motion analyzer with the given configuration
    pub async fn new(config: AnalyzerConfig) -> Result<Self> {
        info!(
            "Initializing GStreamer-enhanced motion analyzer with config: {:?}",
            config
        );

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
            #[cfg(target_os = "linux")]
            preprocessing_pipeline: None,
            #[cfg(target_os = "linux")]
            appsrc: None,
            #[cfg(target_os = "linux")]
            appsink: None,
            #[cfg(target_os = "linux")]
            preprocessing_dims: None,
        };

        info!("Motion analyzer initialized successfully");
        Ok(analyzer)
    }

    /// Initialize GStreamer preprocessing pipeline for motion analysis with target dimensions
    #[cfg(target_os = "linux")]
    fn initialize_preprocessing_pipeline(
        &mut self,
        target_width: u32,
        target_height: u32,
    ) -> Result<()> {
        use gstreamer::prelude::*;
        use gstreamer_app::{AppSink, AppSrc};

        info!(
            "Initializing software GStreamer pipeline for motion analysis at {}x{} (scale 1/{})",
            target_width, target_height, self.config.jpeg_decode_scale
        );

        // Software pipeline - decode JPEG and scale down for motion analysis
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

        debug!(
            "Creating software motion analysis pipeline: {}",
            sw_pipeline_desc
        );

        let pipeline = gstreamer::parse::launch(&sw_pipeline_desc).map_err(|e| {
            AnalyzerError::FrameProcessing {
                details: format!("Failed to create software pipeline: {}", e),
            }
        })?;

        let pipeline = pipeline.downcast::<gstreamer::Pipeline>().map_err(|_| {
            AnalyzerError::FrameProcessing {
                details: "Failed to downcast to Pipeline".to_string(),
            }
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
    #[cfg(target_os = "linux")]
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
    pub(crate) fn detect_motion_sync(&mut self, frame: &FrameData) -> Result<Option<f64>> {
        debug!(
            "Analyzing frame {} for motion ({}x{}, {:?})",
            frame.id, frame.width, frame.height, frame.format
        );

        let gray_image = self.frame_to_gray_image(frame)?;

        // Apply Gaussian blur to reduce noise
        let blurred = gaussian_blur_f32(&gray_image, 2.0);

        // Initialize background model if this is the first frame
        if self.background_model.is_none() {
            info!("Initializing background model with first frame");
            self.background_model = Some(blurred.clone());
            self.frame_count = 1;
            return Ok(None);
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

    /// Convert frame data to grayscale image using GStreamer preprocessing when available
    fn frame_to_gray_image(&mut self, frame: &FrameData) -> Result<GrayImage> {
        #[cfg(target_os = "linux")]
        {
            if frame.format == FrameFormat::Mjpeg {
                if let Ok(gray_image) = self.frame_to_gray_gstreamer(frame) {
                    return Ok(gray_image);
                } else {
                    debug!("GStreamer preprocessing failed, falling back to direct processing");
                }
            }
        }

        match frame.format {
            FrameFormat::Mjpeg => {
                let dynamic_image = image::load_from_memory(&frame.data).map_err(|e| {
                    AnalyzerError::FrameProcessing {
                        details: format!("MJPEG decode failed: {}", e),
                    }
                })?;
                Ok(dynamic_image.to_luma8())
            }
            FrameFormat::Yuyv => self.yuyv_to_gray(frame),
            FrameFormat::Rgb24 => self.rgb24_to_gray(frame),
        }
    }

    /// Convert MJPEG frame to grayscale using GStreamer preprocessing
    #[cfg(target_os = "linux")]
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
            if let Err(e) = appsrc.push_buffer(buffer) {
                self.cleanup();
                return Err(AnalyzerError::FrameProcessing {
                    details: format!("Failed to push buffer to preprocessing pipeline: {:?}", e),
                }
                .into());
            }

            // Pull processed sample with a timeout to avoid blocking indefinitely
            let timeout = gstreamer::ClockTime::from_mseconds(200);
            if let Some(sample) = appsink.try_pull_sample(timeout) {
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
                self.cleanup();
                return Err(AnalyzerError::FrameProcessing {
                    details: "No sample available from GStreamer pipeline (timeout)".to_string(),
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
    fn yuyv_to_gray(&self, frame: &FrameData) -> Result<GrayImage> {
        let width = frame.width;
        let height = frame.height;
        let mut gray_image = GrayImage::new(width, height);

        // YUYV format: Y0 U Y1 V (4 bytes for 2 pixels)
        for y in 0..height {
            for x in 0..(width / 2) {
                let base_idx = ((y * width / 2 + x) * 4) as usize;
                if base_idx + 3 < frame.data.len() {
                    let y0 = frame.data[base_idx];
                    let y1 = frame.data[base_idx + 2];

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
    fn rgb24_to_gray(&self, frame: &FrameData) -> Result<GrayImage> {
        let width = frame.width;
        let height = frame.height;

        let rgb_image =
            RgbImage::from_raw(width, height, frame.data.to_vec()).ok_or_else(|| {
                AnalyzerError::FrameProcessing {
                    details: "Failed to create RGB image from raw data".to_string(),
                }
            })?;

        let mut gray_image = GrayImage::new(width, height);
        for (x, y, rgb) in rgb_image.enumerate_pixels() {
            let gray_value =
                (0.299 * rgb[0] as f32 + 0.587 * rgb[1] as f32 + 0.114 * rgb[2] as f32) as u8;
            gray_image.put_pixel(x, y, Luma([gray_value]));
        }

        Ok(gray_image)
    }

    /// Calculate frame difference between background and current frame
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
    fn calculate_largest_component_area(
        &self,
        components: &ImageBuffer<Luma<u32>, Vec<u32>>,
    ) -> f64 {
        let mut component_counts = std::collections::HashMap::new();

        for pixel in components.pixels() {
            let component_id = pixel[0];
            if component_id > 0 {
                *component_counts.entry(component_id).or_insert(0) += 1;
            }
        }

        component_counts.values().max().copied().unwrap_or(0) as f64
    }

    /// Update background model using simple running average
    fn update_background_model(&mut self, current_frame: &GrayImage) {
        if let Some(ref mut background) = self.background_model {
            let learning_rate = 0.05;

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

    pub(crate) fn background_initialized(&self) -> bool {
        self.background_model.is_some()
    }

    /// Update configuration (requires restart of analysis loop)
    pub fn update_config(&mut self, config: AnalyzerConfig) {
        info!("Updating motion analyzer configuration: {:?}", config);
        self.config = config;
    }

    /// Cleanup GStreamer resources (no-op on non-Linux)
    #[cfg(target_os = "linux")]
    pub fn cleanup(&mut self) {
        use gstreamer::prelude::*;

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

    #[cfg(not(target_os = "linux"))]
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

    #[tokio::test]
    async fn test_motion_detection_with_synthetic_frame() {
        let config = AnalyzerConfig {
            fps: 5,
            delta_threshold: 25,
            contour_minimum_area: 100.0,
            jpeg_decode_scale: 4,
        };

        let mut analyzer = MotionAnalyzer::new(config).await.unwrap();

        let width = 64;
        let height = 48;
        let rgb_data = vec![128u8; (width * height * 3) as usize];

        let frame = FrameData::new(
            1,
            std::time::SystemTime::now(),
            rgb_data,
            width,
            height,
            FrameFormat::Rgb24,
        );

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::task::spawn_blocking(move || analyzer.detect_motion_sync(&frame)),
        )
        .await;

        match result {
            Ok(Ok(motion_result)) => match motion_result {
                Ok(_) => {}
                Err(crate::error::DoorcamError::Analyzer(_)) => {}
                Err(e) => {
                    panic!("Unexpected error: {}", e);
                }
            },
            Ok(Err(_)) => {
                panic!("Task panicked during motion detection");
            }
            Err(_) => {
                panic!("Motion detection timed out - this indicates a hanging operation");
            }
        }
    }
}
