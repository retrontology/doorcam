use crate::analyzer::motion::MotionAnalyzer;
use crate::config::AnalyzerConfig;
use crate::error::{DoorcamError, Result};
use crate::events::{DoorcamEvent, EventBus, EventFilter};
use crate::ring_buffer::RingBuffer;

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Metrics about motion analysis state
#[derive(Debug, Clone, Default)]
pub struct MotionAnalysisMetrics {
    pub background_initialized: bool,
    pub frames_processed: u64,
}

/// Orchestrator that connects the motion analyzer with the ring buffer and event system
pub struct MotionAnalyzerOrchestrator {
    analyzer: Arc<RwLock<MotionAnalyzer>>,
    ring_buffer: Arc<RingBuffer>,
    event_bus: Arc<EventBus>,
    analysis_task: Option<JoinHandle<()>>,
    event_handler_task: Option<JoinHandle<()>>,
    is_running: Arc<tokio::sync::RwLock<bool>>,
    last_analysis_time: Arc<tokio::sync::RwLock<std::time::Instant>>,
}

impl MotionAnalyzerOrchestrator {
    /// Create a new motion analyzer orchestrator
    pub async fn new(
        config: AnalyzerConfig,
        ring_buffer: Arc<RingBuffer>,
        event_bus: Arc<EventBus>,
    ) -> Result<Self> {
        info!("Creating motion analyzer orchestrator");

        let analyzer = MotionAnalyzer::new(config).await?;

        Ok(Self {
            analyzer: Arc::new(RwLock::new(analyzer)),
            ring_buffer,
            event_bus,
            analysis_task: None,
            event_handler_task: None,
            is_running: Arc::new(tokio::sync::RwLock::new(false)),
            last_analysis_time: Arc::new(tokio::sync::RwLock::new(std::time::Instant::now())),
        })
    }

    /// Start the motion analysis orchestrator
    pub async fn start(&mut self) -> Result<()> {
        let mut is_running = self.is_running.write().await;
        if *is_running {
            warn!("Motion analyzer orchestrator is already running");
            return Ok(());
        }

        info!("Starting motion analyzer orchestrator");

        let analyzer = Arc::clone(&self.analyzer);
        let ring_buffer = Arc::clone(&self.ring_buffer);
        let event_bus = Arc::clone(&self.event_bus);
        let is_running_clone = Arc::clone(&self.is_running);
        let last_analysis_time = Arc::clone(&self.last_analysis_time);

        let analysis_task = tokio::spawn(async move {
            info!("Motion analysis task started");

            let mut last_frame_id = 0u64;

            loop {
                {
                    let running = is_running_clone.read().await;
                    if !*running {
                        info!("Motion analysis task stopping");
                        break;
                    }
                }

                let config = {
                    let analyzer_guard = analyzer.read().await;
                    analyzer_guard.config().clone()
                };

                let frame_interval = Duration::from_millis(1000 / config.fps as u64);

                let should_analyze = {
                    let last_time = last_analysis_time.read().await;
                    last_time.elapsed() >= frame_interval
                };

                if should_analyze {
                    if let Some(frame) = ring_buffer.get_latest_frame().await {
                        if frame.id < last_frame_id {
                            warn!(
                                "Detected frame ID reset ({} -> {}), resetting analysis cursor",
                                last_frame_id, frame.id
                            );
                            last_frame_id = 0;
                        }

                        if frame.id > last_frame_id {
                            last_frame_id = frame.id;

                            {
                                let mut last_time = last_analysis_time.write().await;
                                *last_time = std::time::Instant::now();
                            }

                            debug!("Analyzing frame {} (fps limit: {})", frame.id, config.fps);

                            let analysis_result = tokio::time::timeout(
                                tokio::time::Duration::from_millis(2000),
                                async {
                                    let mut analyzer_guard = analyzer.write().await;
                                    analyzer_guard.analyze_frame(&frame, &event_bus).await
                                },
                            )
                            .await;

                            match analysis_result {
                                Ok(Ok(Some(motion_area))) => {
                                    info!("Motion detected with area: {:.2}", motion_area);
                                }
                                Ok(Ok(None)) => {
                                    debug!("No motion detected in frame {}", frame.id);
                                }
                                Ok(Err(e)) => {
                                    error!("Motion analysis error: {}", e);

                                    if let Err(publish_err) = event_bus
                                        .publish(DoorcamEvent::SystemError {
                                            component: "motion_analyzer_orchestrator".to_string(),
                                            error: e.to_string(),
                                        })
                                        .await
                                    {
                                        error!(
                                            "Failed to publish motion analysis error event: {}",
                                            publish_err
                                        );
                                    }
                                }
                                Err(_) => {
                                    warn!("Motion analysis timeout, skipping frame {}", frame.id);
                                }
                            }
                        } else {
                            debug!("Frame {} already analyzed, skipping", frame.id);
                        }
                    } else {
                        debug!("No frames available for motion analysis");
                    }
                }

                tokio::time::sleep(Duration::from_millis(5)).await;
            }

            info!("Motion analysis task ended");
        });

        let event_bus = Arc::clone(&self.event_bus);
        let event_handler_task = tokio::spawn(async move {
            info!("Motion analyzer event handler started");

            let mut event_receiver = event_bus.subscribe();
            let motion_filter = EventFilter::EventTypes(vec!["motion_detected"]);

            loop {
                match event_receiver.recv().await {
                    Ok(event) => {
                        if motion_filter.matches(&event) {
                            match event {
                                DoorcamEvent::MotionDetected {
                                    contour_area,
                                    timestamp,
                                } => {
                                    debug!(
                                        "Motion detected event received (area: {:.2}, ts: {:?})",
                                        contour_area, timestamp
                                    );
                                }
                                DoorcamEvent::SystemError { component, error } => {
                                    error!(
                                        "System error from {} received by analyzer orchestrator: {}",
                                        component, error
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Motion analyzer event handler error: {}", e);
                        break;
                    }
                }
            }

            info!("Motion analyzer event handler stopped");
        });

        self.analysis_task = Some(analysis_task);
        self.event_handler_task = Some(event_handler_task);
        *is_running = true;

        Ok(())
    }

    /// Stop the motion analysis orchestrator
    pub async fn stop(&mut self) -> Result<()> {
        let mut is_running = self.is_running.write().await;
        if !*is_running {
            warn!("Motion analyzer orchestrator is not running");
            return Ok(());
        }

        info!("Stopping motion analyzer orchestrator");
        *is_running = false;

        if let Some(task) = self.analysis_task.take() {
            if let Err(e) = task.await {
                error!("Error stopping motion analysis task: {}", e);
            }
        }

        if let Some(task) = self.event_handler_task.take() {
            if let Err(e) = task.await {
                error!("Error stopping motion analyzer event handler: {}", e);
            }
        }

        info!("Motion analyzer orchestrator stopped");
        Ok(())
    }

    /// Get the underlying analyzer (for testing or direct access)
    pub fn analyzer(&self) -> Arc<RwLock<MotionAnalyzer>> {
        Arc::clone(&self.analyzer)
    }

    /// Get metrics about the motion analysis
    pub async fn get_metrics(&self) -> MotionAnalysisMetrics {
        let analyzer = self.analyzer.read().await;

        MotionAnalysisMetrics {
            background_initialized: analyzer.background_initialized(),
            frames_processed: analyzer.frame_count,
        }
    }
}

/// Builder for MotionAnalyzerOrchestrator
pub struct MotionAnalyzerOrchestratorBuilder {
    config: Option<AnalyzerConfig>,
    ring_buffer: Option<Arc<RingBuffer>>,
    event_bus: Option<Arc<EventBus>>,
}

impl MotionAnalyzerOrchestratorBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            config: None,
            ring_buffer: None,
            event_bus: None,
        }
    }

    /// Set the analyzer configuration
    pub fn config(mut self, config: AnalyzerConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set the ring buffer
    pub fn ring_buffer(mut self, ring_buffer: Arc<RingBuffer>) -> Self {
        self.ring_buffer = Some(ring_buffer);
        self
    }

    /// Set the event bus
    pub fn event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Build the orchestrator
    pub async fn build(self) -> Result<MotionAnalyzerOrchestrator> {
        let config = self.config.ok_or_else(|| {
            DoorcamError::component("motion_analyzer_orchestrator_builder", "Config is required")
        })?;

        let ring_buffer = self.ring_buffer.ok_or_else(|| {
            DoorcamError::component(
                "motion_analyzer_orchestrator_builder",
                "Ring buffer is required",
            )
        })?;

        let event_bus = self.event_bus.ok_or_else(|| {
            DoorcamError::component(
                "motion_analyzer_orchestrator_builder",
                "Event bus is required",
            )
        })?;

        MotionAnalyzerOrchestrator::new(config, ring_buffer, event_bus).await
    }
}
