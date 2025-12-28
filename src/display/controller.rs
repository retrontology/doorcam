use crate::config::DisplayConfig;
use crate::error::{DisplayError, Result};
use crate::events::{DoorcamEvent, EventBus, EventFilter, EventReceiver};
use crate::frame::{FrameData, FrameFormat};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

#[cfg(target_os = "linux")]
use gstreamer::prelude::*;
#[cfg(target_os = "linux")]
use gstreamer::Pipeline;
#[cfg(target_os = "linux")]
use gstreamer_app::AppSrc;

/// Display controller for HyperPixel 4.0 with GStreamer hardware acceleration
pub struct DisplayController {
    pub(crate) config: DisplayConfig,
    pub(crate) backlight: Arc<RwLock<Option<File>>>,
    pub(crate) is_active: Arc<AtomicBool>,
    pub(crate) activation_timer: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    #[cfg(target_os = "linux")]
    pub(crate) display_pipeline: Arc<RwLock<Option<Pipeline>>>,
    #[cfg(target_os = "linux")]
    pub(crate) appsrc: Arc<RwLock<Option<AppSrc>>>,
}

impl DisplayController {
    /// Create a new display controller
    pub async fn new(config: DisplayConfig) -> Result<Self> {
        info!("Initializing GStreamer display controller for HyperPixel 4.0");
        debug!("Display config: {:?}", config);

        #[cfg(target_os = "linux")]
        {
            // Initialize GStreamer
            gstreamer::init().map_err(|e| DisplayError::Framebuffer {
                details: format!("Failed to initialize GStreamer: {}", e),
            })?;
        }

        let controller = Self {
            config,
            backlight: Arc::new(RwLock::new(None)),
            is_active: Arc::new(AtomicBool::new(false)),
            activation_timer: Arc::new(RwLock::new(None)),
            #[cfg(target_os = "linux")]
            display_pipeline: Arc::new(RwLock::new(None)),
            #[cfg(target_os = "linux")]
            appsrc: Arc::new(RwLock::new(None)),
        };

        controller.initialize_display_pipeline().await?;
        controller.initialize_devices().await?;

        Ok(controller)
    }

    /// Initialize GStreamer display pipeline with hardware acceleration
    #[cfg(target_os = "linux")]
    async fn initialize_display_pipeline(&self) -> Result<()> {
        let (display_width, display_height) = self.config.resolution;

        let (pre_rotation_width, pre_rotation_height) = match &self.config.rotation {
            Some(crate::config::Rotation::Rotate90) | Some(crate::config::Rotation::Rotate270) => {
                (display_height, display_width)
            }
            _ => (display_width, display_height),
        };

        let mut pipeline_desc = "appsrc name=src format=bytes is-live=true caps=image/jpeg ! queue max-size-buffers=1 leaky=downstream ! jpegdec".to_string();

        pipeline_desc.push_str(&format!(
            " ! videoconvert ! \
             videoscale method=nearest-neighbour ! \
             video/x-raw,width={},height={}",
            pre_rotation_width, pre_rotation_height
        ));

        if let Some(rotation) = &self.config.rotation {
            let flip_method = match rotation {
                crate::config::Rotation::Rotate90 => "clockwise",
                crate::config::Rotation::Rotate180 => "rotate-180",
                crate::config::Rotation::Rotate270 => "counterclockwise",
            };

            pipeline_desc.push_str(&format!(" ! videoflip method={}", flip_method));
            let degrees = match rotation {
                crate::config::Rotation::Rotate90 => 90,
                crate::config::Rotation::Rotate180 => 180,
                crate::config::Rotation::Rotate270 => 270,
            };
            info!("Display rotation enabled: {} degrees ({}) - scaling to {}x{} before rotation to achieve final {}x{}", 
                  degrees, flip_method, pre_rotation_width, pre_rotation_height, display_width, display_height);
        }

        pipeline_desc.push_str(&format!(
            " ! videoconvert ! \
             video/x-raw,format=RGB16 ! \
             fbdevsink device={} sync=false max-lateness=-1 async=false",
            self.config.framebuffer_device
        ));

        info!("Creating GStreamer display pipeline: {}", pipeline_desc);

        let pipeline = gstreamer::parse::launch(&pipeline_desc)
            .map_err(|e| DisplayError::Framebuffer {
                details: format!("Failed to create display pipeline: {}", e),
            })?
            .downcast::<Pipeline>()
            .map_err(|_| DisplayError::Framebuffer {
                details: "Failed to downcast to Pipeline".to_string(),
            })?;

        let appsrc = pipeline
            .by_name("src")
            .ok_or_else(|| DisplayError::Framebuffer {
                details: "Failed to get appsrc element".to_string(),
            })?
            .downcast::<AppSrc>()
            .map_err(|_| DisplayError::Framebuffer {
                details: "Failed to downcast to AppSrc".to_string(),
            })?;

        appsrc.set_property("format", gstreamer::Format::Bytes);
        appsrc.set_property("is-live", true);
        appsrc.set_property("max-bytes", 200000u64);
        appsrc.set_property("block", false);
        appsrc.set_property("do-timestamp", false);

        {
            let mut pipeline_lock = self.display_pipeline.write().await;
            *pipeline_lock = Some(pipeline);
        }
        {
            let mut appsrc_lock = self.appsrc.write().await;
            *appsrc_lock = Some(appsrc);
        }

        info!("GStreamer display pipeline initialized successfully");
        Ok(())
    }

    /// Initialize display pipeline when GStreamer feature is disabled
    #[cfg(not(target_os = "linux"))]
    async fn initialize_display_pipeline(&self) -> Result<()> {
        warn!("GStreamer display pipeline is only available on Linux with display feature");
        Ok(())
    }

    /// Initialize backlight device connection
    async fn initialize_devices(&self) -> Result<()> {
        match self.open_backlight().await {
            Ok(bl) => {
                let mut backlight = self.backlight.write().await;
                *backlight = Some(bl);
                info!("Backlight device opened: {}", self.config.backlight_device);
            }
            Err(e) => {
                warn!(
                    "Failed to open backlight device {}: {}",
                    self.config.backlight_device, e
                );
            }
        }

        Ok(())
    }

    /// Open backlight device for writing
    async fn open_backlight(&self) -> Result<File> {
        Ok(OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.config.backlight_device)
            .map_err(|e| DisplayError::BacklightOpen {
                device: self.config.backlight_device.clone(),
                source: e,
            })?)
    }

    /// Start the display controller with event handling
    pub async fn start(&self, event_bus: Arc<EventBus>) -> Result<()> {
        info!("Starting display controller");

        let receiver = event_bus.subscribe();
        let filter = EventFilter::EventTypes(vec![
            "motion_detected",
            "touch_detected",
            "display_activate",
            "display_deactivate",
        ]);
        let mut event_receiver =
            EventReceiver::new(receiver, filter, "display_controller".to_string());

        let controller = self.clone_for_task();
        let event_bus_clone = Arc::clone(&event_bus);

        tokio::spawn(async move {
            loop {
                match event_receiver.recv().await {
                    Ok(event) => {
                        if let Err(e) = controller.handle_event(event, &event_bus_clone).await {
                            error!("Error handling display event: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Error receiving display events: {}", e);
                        sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });

        info!("Display controller started successfully");
        Ok(())
    }

    /// Handle incoming events
    async fn handle_event(&self, event: DoorcamEvent, event_bus: &Arc<EventBus>) -> Result<()> {
        match event {
            DoorcamEvent::MotionDetected { timestamp, .. } => {
                debug!("Motion detected - activating display");
                self.activate_display(timestamp, event_bus).await?;
            }
            DoorcamEvent::TouchDetected { timestamp } => {
                debug!("Touch detected - activating display");
                self.activate_display(timestamp, event_bus).await?;
            }
            DoorcamEvent::DisplayActivate {
                timestamp,
                duration_seconds,
            } => {
                debug!(
                    "Display activation requested for {} seconds",
                    duration_seconds
                );
                self.activate_display_with_duration(timestamp, duration_seconds, event_bus)
                    .await?;
            }
            DoorcamEvent::DisplayDeactivate { .. } => {
                debug!("Display deactivation requested");
                self.deactivate_display(event_bus).await?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Activate the display for the configured duration
    async fn activate_display(
        &self,
        _timestamp: SystemTime,
        event_bus: &Arc<EventBus>,
    ) -> Result<()> {
        self.activate_display_with_duration(
            SystemTime::now(),
            self.config.activation_period_seconds,
            event_bus,
        )
        .await
    }

    /// Activate the display for a specific duration
    async fn activate_display_with_duration(
        &self,
        _timestamp: SystemTime,
        duration_seconds: u32,
        event_bus: &Arc<EventBus>,
    ) -> Result<()> {
        self.is_active.store(true, Ordering::Relaxed);
        self.set_backlight(true).await?;

        {
            let mut timer = self.activation_timer.write().await;
            if let Some(handle) = timer.take() {
                handle.abort();
            }
        }

        let is_active = Arc::clone(&self.is_active);
        let event_bus_clone = Arc::clone(event_bus);
        let duration = Duration::from_secs(duration_seconds as u64);

        let timer_handle = tokio::spawn(async move {
            sleep(duration).await;

            is_active.store(false, Ordering::Relaxed);

            let _ = event_bus_clone
                .publish(DoorcamEvent::DisplayDeactivate {
                    timestamp: SystemTime::now(),
                })
                .await;
        });

        {
            let mut timer = self.activation_timer.write().await;
            *timer = Some(timer_handle);
        }

        info!("Display activated for {} seconds", duration_seconds);
        Ok(())
    }

    /// Deactivate the display immediately
    async fn deactivate_display(&self, _event_bus: &Arc<EventBus>) -> Result<()> {
        self.is_active.store(false, Ordering::Relaxed);
        self.set_backlight(false).await?;

        {
            let mut timer = self.activation_timer.write().await;
            if let Some(handle) = timer.take() {
                handle.abort();
            }
        }

        info!("Display deactivated");
        Ok(())
    }

    /// Control backlight on/off state
    async fn set_backlight(&self, enabled: bool) -> Result<()> {
        let mut backlight = self.backlight.write().await;

        if let Some(ref mut bl_file) = *backlight {
            let power_value = if enabled { "0" } else { "1" };

            bl_file
                .seek(SeekFrom::Start(0))
                .map_err(|e| DisplayError::Backlight {
                    details: format!("Failed to seek backlight: {}", e),
                })?;

            bl_file
                .write_all(power_value.as_bytes())
                .map_err(|e| DisplayError::Backlight {
                    details: format!("Failed to write backlight: {}", e),
                })?;

            bl_file.flush().map_err(|e| DisplayError::Backlight {
                details: format!("Failed to flush backlight: {}", e),
            })?;

            debug!(
                "Backlight set to: {} (power value: {})",
                if enabled { "ON" } else { "OFF" },
                power_value
            );
        } else {
            match self.open_backlight().await {
                Ok(bl) => {
                    *backlight = Some(bl);
                    debug!("Backlight device reconnected");
                    drop(backlight);
                    let mut backlight_retry = self.backlight.write().await;
                    if let Some(ref mut bl_file) = *backlight_retry {
                        let power_value = if enabled { "0" } else { "1" };

                        bl_file
                            .seek(SeekFrom::Start(0))
                            .map_err(|e| DisplayError::Backlight {
                                details: format!("Failed to seek backlight: {}", e),
                            })?;

                        bl_file.write_all(power_value.as_bytes()).map_err(|e| {
                            DisplayError::Backlight {
                                details: format!("Failed to write backlight: {}", e),
                            }
                        })?;

                        bl_file.flush().map_err(|e| DisplayError::Backlight {
                            details: format!("Failed to flush backlight: {}", e),
                        })?;

                        debug!(
                            "Backlight set to: {} (power value: {})",
                            if enabled { "ON" } else { "OFF" },
                            power_value
                        );
                    }
                }
                Err(e) => {
                    warn!("Backlight control unavailable: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Render a frame to the display using GStreamer hardware acceleration
    pub async fn render_frame(&self, frame: &FrameData) -> Result<()> {
        if !self.is_active.load(Ordering::Relaxed) {
            return Ok(());
        }

        #[cfg(target_os = "linux")]
        {
            self.render_frame_gstreamer(frame).await?;
        }

        #[cfg(not(target_os = "linux"))]
        {
            warn!("Display rendering not available without GStreamer on Linux");
        }

        Ok(())
    }

    /// Render frame using GStreamer pipeline
    #[cfg(target_os = "linux")]
    async fn render_frame_gstreamer(&self, frame: &FrameData) -> Result<()> {
        let pipeline_lock = self.display_pipeline.read().await;
        let appsrc_lock = self.appsrc.read().await;

        if let (Some(pipeline), Some(appsrc)) = (pipeline_lock.as_ref(), appsrc_lock.as_ref()) {
            if pipeline.current_state() != gstreamer::State::Playing {
                pipeline.set_state(gstreamer::State::Playing).map_err(|e| {
                    DisplayError::Framebuffer {
                        details: format!("Failed to start display pipeline: {}", e),
                    }
                })?;
                debug!("Display pipeline started");
            }

            let jpeg_data = match frame.format {
                FrameFormat::Mjpeg => frame.data.as_ref().clone(),
                _ => {
                    return Err(DisplayError::Framebuffer {
                        details: format!(
                            "Only MJPEG frames supported for GStreamer display, got {:?}",
                            frame.format
                        ),
                    }
                    .into());
                }
            };

            let mut buffer = gstreamer::Buffer::with_size(jpeg_data.len()).map_err(|e| {
                DisplayError::Framebuffer {
                    details: format!("Failed to create GStreamer buffer: {}", e),
                }
            })?;

            {
                let buffer_ref = buffer.get_mut().unwrap();
                let mut map = buffer_ref
                    .map_writable()
                    .map_err(|e| DisplayError::Framebuffer {
                        details: format!("Failed to map buffer: {}", e),
                    })?;
                map.copy_from_slice(&jpeg_data);
            }

            appsrc
                .push_buffer(buffer)
                .map_err(|e| DisplayError::Framebuffer {
                    details: format!("Failed to push buffer to display pipeline: {:?}", e),
                })?;

            debug!("Frame {} rendered via GStreamer pipeline", frame.id);
            Ok(())
        } else {
            Err(DisplayError::Framebuffer {
                details: "Display pipeline not initialized".to_string(),
            }
            .into())
        }
    }

    /// Check if display is currently active
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Relaxed)
    }

    /// Get display configuration
    pub fn config(&self) -> &DisplayConfig {
        &self.config
    }

    /// Clone for use in async tasks
    pub(crate) fn clone_for_task(&self) -> Self {
        Self {
            config: self.config.clone(),
            backlight: Arc::clone(&self.backlight),
            is_active: Arc::clone(&self.is_active),
            activation_timer: Arc::clone(&self.activation_timer),
            #[cfg(target_os = "linux")]
            display_pipeline: Arc::clone(&self.display_pipeline),
            #[cfg(target_os = "linux")]
            appsrc: Arc::clone(&self.appsrc),
        }
    }
}

impl Clone for DisplayController {
    fn clone(&self) -> Self {
        self.clone_for_task()
    }
}
