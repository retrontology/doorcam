use super::controller::DisplayController;
use super::stats::DisplayStats;
use crate::config::DisplayConfig;
use crate::error::{DoorcamError, Result};
use crate::events::{DoorcamEvent, EventBus, EventFilter, EventReceiver};
use crate::ring_buffer::RingBuffer;
use crate::touch::{MockTouchInputHandler, TouchInputHandler};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::time::sleep;
use tracing::{error, info};

/// Integration component that manages display controller and touch input together
pub struct DisplayIntegration {
    pub(crate) display_controller: DisplayController,
    pub(crate) config: DisplayConfig,
    pub(crate) event_bus: Arc<EventBus>,
    use_mock_touch: bool,
    is_running: Arc<tokio::sync::RwLock<bool>>,
    render_task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    touch_task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl DisplayIntegration {
    pub async fn new(config: DisplayConfig, event_bus: Arc<EventBus>) -> Result<Self> {
        info!("Initializing display integration");

        let display_controller = DisplayController::new(config.clone()).await?;

        Ok(Self {
            display_controller,
            config,
            event_bus,
            use_mock_touch: false,
            is_running: Arc::new(tokio::sync::RwLock::new(false)),
            render_task: Arc::new(tokio::sync::Mutex::new(None)),
            touch_task: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    pub fn with_mock_touch(mut self) -> Self {
        self.use_mock_touch = true;
        self
    }

    pub async fn start(&self, ring_buffer: Arc<RingBuffer>) -> Result<()> {
        let mut is_running = self.is_running.write().await;
        if *is_running {
            info!("Display integration is already running");
            return Ok(());
        }

        info!("Starting display integration");

        self.display_controller
            .start(Arc::clone(&self.event_bus))
            .await?;

        let touch_task = if self.use_mock_touch {
            info!("Using mock touch input handler");
            let mock_handler = MockTouchInputHandler::new(Arc::clone(&self.event_bus));
            Some(tokio::spawn(async move {
                if let Err(e) = mock_handler.start().await {
                    error!("Mock touch handler error: {}", e);
                }
            }))
        } else {
            info!("Using real touch input handler");
            let touch_handler = TouchInputHandler::new(&self.config, Arc::clone(&self.event_bus));
            Some(tokio::spawn(async move {
                if let Err(e) = touch_handler.start().await {
                    error!("Touch handler error: {}", e);
                }
            }))
        };

        if let Some(task) = touch_task {
            *self.touch_task.lock().await = Some(task);
        }

        self.start_frame_rendering(ring_buffer).await?;

        *is_running = true;
        info!("Display integration started successfully");
        Ok(())
    }

    async fn start_frame_rendering(&self, ring_buffer: Arc<RingBuffer>) -> Result<()> {
        let display_controller = self.display_controller.clone();
        let event_bus = Arc::clone(&self.event_bus);
        let is_running = Arc::clone(&self.is_running);

        let task = tokio::spawn(async move {
            let mut render_interval = tokio::time::interval(Duration::from_millis(16));
            let mut last_frame_id = 0u64;

            while *is_running.read().await {
                render_interval.tick().await;

                if !*is_running.read().await {
                    break;
                }

                if display_controller.is_active() {
                    if let Some(frame) = ring_buffer.get_latest_frame().await {
                        if frame.id > last_frame_id {
                            last_frame_id = frame.id;

                            if let Err(e) = display_controller.render_frame(&frame).await {
                                error!("Failed to render frame to display: {}", e);
                                let _ = event_bus
                                    .publish(DoorcamEvent::SystemError {
                                        component: "display_rendering".to_string(),
                                        error: e.to_string(),
                                    })
                                    .await;
                            }
                        }
                    }
                }
            }

            info!("Display rendering loop stopped");
        });

        *self.render_task.lock().await = Some(task);

        Ok(())
    }

    pub fn display_controller(&self) -> &DisplayController {
        &self.display_controller
    }

    pub fn is_display_active(&self) -> bool {
        self.display_controller.is_active()
    }

    pub async fn activate_display(&self) -> Result<()> {
        self.event_bus
            .publish(DoorcamEvent::DisplayActivate {
                timestamp: SystemTime::now(),
                duration_seconds: self.config.activation_period_seconds,
            })
            .await
            .map(|_| ())
            .map_err(|e| {
                DoorcamError::component(
                    "display_integration",
                    &format!("Failed to publish display activate event: {}", e),
                )
            })
    }

    pub async fn deactivate_display(&self) -> Result<()> {
        self.event_bus
            .publish(DoorcamEvent::DisplayDeactivate {
                timestamp: SystemTime::now(),
            })
            .await
            .map(|_| ())
            .map_err(|e| {
                DoorcamError::component(
                    "display_integration",
                    &format!("Failed to publish display deactivate event: {}", e),
                )
            })
    }

    pub async fn stop(&self) -> Result<()> {
        let mut is_running = self.is_running.write().await;
        if !*is_running {
            return Ok(());
        }

        info!("Stopping display integration");
        *is_running = false;

        if let Some(task) = self.render_task.lock().await.take() {
            let _ = task.await;
        }

        if let Some(task) = self.touch_task.lock().await.take() {
            let _ = task.await;
        }

        info!("Display integration stopped");
        Ok(())
    }

    pub async fn with_mock_touch_runtime(
        mut self,
        use_mock_touch: bool,
        ring_buffer: Arc<RingBuffer>,
    ) -> Result<Self> {
        self.use_mock_touch = use_mock_touch;
        self.start(ring_buffer).await?;
        Ok(self)
    }

    pub fn get_display_controller(&self) -> &DisplayController {
        &self.display_controller
    }

    pub fn touch_task_handle(
        &self,
    ) -> Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>> {
        Arc::clone(&self.touch_task)
    }
}

/// Builder for DisplayIntegration
pub struct DisplayIntegrationBuilder {
    config: Option<DisplayConfig>,
    event_bus: Option<Arc<EventBus>>,
    use_mock_touch: bool,
}

impl DisplayIntegrationBuilder {
    pub fn new() -> Self {
        Self {
            config: None,
            event_bus: None,
            use_mock_touch: false,
        }
    }

    pub fn config(mut self, config: DisplayConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    pub fn use_mock_touch(mut self, use_mock_touch: bool) -> Self {
        self.use_mock_touch = use_mock_touch;
        self
    }

    pub async fn build(self) -> Result<DisplayIntegration> {
        let config = self.config.ok_or_else(|| {
            DoorcamError::component("display_integration_builder", "Display config is required")
        })?;

        let event_bus = self.event_bus.ok_or_else(|| {
            DoorcamError::component("display_integration_builder", "Event bus is required")
        })?;

        let mut integration = DisplayIntegration::new(config, event_bus).await?;
        if self.use_mock_touch {
            integration = integration.with_mock_touch();
        }

        Ok(integration)
    }
}

/// Display integration with statistics tracking
pub struct DisplayIntegrationWithStats {
    integration: DisplayIntegration,
    stats: Arc<tokio::sync::RwLock<DisplayStats>>,
}

impl DisplayIntegrationWithStats {
    pub async fn new(config: DisplayConfig, event_bus: Arc<EventBus>) -> Result<Self> {
        let integration = DisplayIntegration::new(config, event_bus).await?;
        let stats = Arc::new(tokio::sync::RwLock::new(DisplayStats::default()));

        Ok(Self { integration, stats })
    }

    pub async fn start(&self, ring_buffer: Arc<RingBuffer>) -> Result<()> {
        self.integration.start(ring_buffer).await?;
        self.start_stats_collection().await?;
        Ok(())
    }

    async fn start_stats_collection(&self) -> Result<()> {
        let event_bus = Arc::clone(&self.integration.event_bus);
        let stats = Arc::clone(&self.stats);

        let receiver = event_bus.subscribe();
        let filter =
            EventFilter::EventTypes(vec!["touch_detected", "display_activate", "system_error"]);
        let mut event_receiver = EventReceiver::new(receiver, filter, "display_stats".to_string());

        tokio::spawn(async move {
            loop {
                match event_receiver.recv().await {
                    Ok(event) => {
                        let mut stats_guard = stats.write().await;
                        match event {
                            DoorcamEvent::TouchDetected { .. } => {
                                stats_guard.record_touch_event();
                            }
                            DoorcamEvent::DisplayActivate { .. } => {
                                stats_guard.record_activation();
                            }
                            DoorcamEvent::SystemError { component, .. } => {
                                if component.contains("display") || component.contains("render") {
                                    stats_guard.record_render_error();
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        error!("Error receiving events for display stats: {}", e);
                        sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });

        Ok(())
    }

    pub async fn get_stats(&self) -> DisplayStats {
        self.stats.read().await.clone()
    }

    pub async fn reset_stats(&self) {
        self.stats.write().await.reset();
    }

    pub fn integration(&self) -> &DisplayIntegration {
        &self.integration
    }
}
