use super::types::ComponentState;
use crate::analyzer::MotionAnalyzerOrchestrator;
use crate::camera::{calculate_ring_buffer_capacity, CameraRingBufferIntegration};
use crate::capture::VideoCaptureIntegration;
use crate::config::DoorcamConfig;
use crate::display::DisplayIntegration;
use crate::error::Result;
use crate::events::EventBus;
use crate::keyboard_input::KeyboardInputHandler;
use crate::ring_buffer::RingBuffer;
use crate::storage::EventStorageIntegration;
use crate::streaming::StreamServer;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};
use tokio_util::sync::CancellationToken;

/// Main application coordinator that manages all system components
pub struct DoorcamOrchestrator {
    pub(super) config: DoorcamConfig,
    pub(super) event_bus: Arc<EventBus>,
    pub(super) ring_buffer: Arc<RingBuffer>,

    // Components
    pub(super) camera_integration: Option<CameraRingBufferIntegration>,
    pub(super) analyzer_orchestrator: Option<Arc<Mutex<MotionAnalyzerOrchestrator>>>,
    pub(super) display_integration: Option<DisplayIntegration>,
    pub(super) capture_integration: Option<VideoCaptureIntegration>,
    pub(super) storage_integration: Option<EventStorageIntegration>,
    pub(super) keyboard_handler: Option<KeyboardInputHandler>,
    pub(super) keyboard_enabled: bool,
    pub(super) stream_server: Option<StreamServer>,

    // Lifecycle management
    pub(super) component_states: Arc<Mutex<HashMap<String, ComponentState>>>,
    pub(super) shutdown_sender: Option<oneshot::Sender<super::types::ShutdownReason>>,
    pub(super) shutdown_receiver: Option<oneshot::Receiver<super::types::ShutdownReason>>,
    pub(super) cancellation_token: CancellationToken,
}

impl DoorcamOrchestrator {
    /// Create a new orchestrator with the given configuration
    pub async fn new(config: DoorcamConfig) -> Result<Self> {
        let event_bus = Arc::new(EventBus::new());
        let ring_buffer_capacity =
            calculate_ring_buffer_capacity(config.camera.fps, config.event.preroll_seconds);
        let ring_buffer = Arc::new(RingBuffer::new(
            ring_buffer_capacity,
            Duration::from_secs(config.event.preroll_seconds as u64),
        ));
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();

        // Initialize camera integration
        let camera_integration = Some(CameraRingBufferIntegration::new(config.clone()).await?);

        // Initialize analyzer integration with camera's ring buffer
        let camera_ring_buffer = if let Some(ref camera_integration) = camera_integration {
            camera_integration.ring_buffer()
        } else {
            Arc::clone(&ring_buffer)
        };

        let analyzer_orchestrator = Some(Arc::new(Mutex::new(
            MotionAnalyzerOrchestrator::new(
                config.analyzer.clone(),
                camera_ring_buffer,
                Arc::clone(&event_bus),
            )
            .await?,
        )));

        // Initialize display integration
        let display_integration =
            Some(DisplayIntegration::new(config.display.clone(), Arc::clone(&event_bus)).await?);

        // Initialize capture integration with camera's ring buffer
        let capture_ring_buffer = if let Some(camera_integration) = &camera_integration {
            camera_integration.ring_buffer()
        } else {
            Arc::clone(&ring_buffer)
        };

        let capture_integration = Some(VideoCaptureIntegration::new(
            config.capture.clone(),
            config.event.clone(),
            config.camera.fps,
            Arc::clone(&event_bus),
            capture_ring_buffer,
        ));

        // Initialize storage integration
        let storage_integration = Some(
            EventStorageIntegration::builder()
                .with_capture_config(config.capture.clone())
                .with_system_config(config.system.clone())
                .with_event_bus(Arc::clone(&event_bus))
                .build()?,
        );

        let stream_server = Some(StreamServer::new(
            config.stream.clone(),
            Arc::clone(&ring_buffer),
            Arc::clone(&event_bus),
            config.camera.fps,
        ));

        // Initialize keyboard input handler for debugging (disabled by default)
        let keyboard_handler = Some(KeyboardInputHandler::new(Arc::clone(&event_bus)));

        Ok(Self {
            config,
            event_bus,
            ring_buffer,
            camera_integration,
            analyzer_orchestrator,
            display_integration,
            capture_integration,
            storage_integration,
            keyboard_handler,
            keyboard_enabled: false, // Disabled by default, enable via set_keyboard_enabled()
            stream_server,
            component_states: Arc::new(Mutex::new(HashMap::new())),
            shutdown_sender: Some(shutdown_sender),
            shutdown_receiver: Some(shutdown_receiver),
            cancellation_token: CancellationToken::new(),
        })
    }

    /// Enable or disable the keyboard input handler
    pub fn set_keyboard_enabled(&mut self, enabled: bool) {
        self.keyboard_enabled = enabled;
    }
}
