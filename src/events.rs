use crate::error::EventBusError;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

/// Events that can occur in the doorcam system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DoorcamEvent {
    /// Motion was detected in the camera feed
    MotionDetected {
        contour_area: f64,
        timestamp: SystemTime,
    },
    /// A new frame is ready in the ring buffer
    FrameReady {
        frame_id: u64,
        timestamp: SystemTime,
    },
    /// Touch input was detected on the display
    TouchDetected { timestamp: SystemTime },
    /// Video capture has started for a motion event
    CaptureStarted { event_id: String },
    /// Video capture has completed
    CaptureCompleted { event_id: String, file_count: u32 },
    /// A system error occurred in a component
    SystemError { component: String, error: String },
    /// Display activation requested
    DisplayActivate {
        timestamp: SystemTime,
        duration_seconds: u32,
    },
    /// Display deactivation requested
    DisplayDeactivate { timestamp: SystemTime },
    /// Camera connection status changed
    CameraStatusChanged {
        connected: bool,
        timestamp: SystemTime,
    },
    /// System shutdown requested
    ShutdownRequested {
        timestamp: SystemTime,
        reason: String,
    },
}

impl DoorcamEvent {
    /// Get the timestamp of the event
    pub fn timestamp(&self) -> SystemTime {
        match self {
            DoorcamEvent::MotionDetected { timestamp, .. } => *timestamp,
            DoorcamEvent::FrameReady { timestamp, .. } => *timestamp,
            DoorcamEvent::TouchDetected { timestamp } => *timestamp,
            DoorcamEvent::CaptureStarted { .. } => SystemTime::now(),
            DoorcamEvent::CaptureCompleted { .. } => SystemTime::now(),
            DoorcamEvent::SystemError { .. } => SystemTime::now(),
            DoorcamEvent::DisplayActivate { timestamp, .. } => *timestamp,
            DoorcamEvent::DisplayDeactivate { timestamp } => *timestamp,
            DoorcamEvent::CameraStatusChanged { timestamp, .. } => *timestamp,
            DoorcamEvent::ShutdownRequested { timestamp, .. } => *timestamp,
        }
    }

    /// Get a human-readable description of the event
    pub fn description(&self) -> String {
        match self {
            DoorcamEvent::MotionDetected { contour_area, .. } => {
                format!("Motion detected with area: {:.2}", contour_area)
            }
            DoorcamEvent::FrameReady { frame_id, .. } => {
                format!("Frame {} ready", frame_id)
            }
            DoorcamEvent::TouchDetected { .. } => "Touch detected".to_string(),
            DoorcamEvent::CaptureStarted { event_id } => {
                format!("Capture started: {}", event_id)
            }
            DoorcamEvent::CaptureCompleted {
                event_id,
                file_count,
            } => {
                format!("Capture completed: {} ({} files)", event_id, file_count)
            }
            DoorcamEvent::SystemError { component, error } => {
                format!("Error in {}: {}", component, error)
            }
            DoorcamEvent::DisplayActivate {
                duration_seconds, ..
            } => {
                format!("Display activated for {} seconds", duration_seconds)
            }
            DoorcamEvent::DisplayDeactivate { .. } => "Display deactivated".to_string(),
            DoorcamEvent::CameraStatusChanged { connected, .. } => {
                format!(
                    "Camera {}",
                    if *connected {
                        "connected"
                    } else {
                        "disconnected"
                    }
                )
            }
            DoorcamEvent::ShutdownRequested { reason, .. } => {
                format!("Shutdown requested: {}", reason)
            }
        }
    }

    /// Get the event type as a string for filtering
    pub fn event_type(&self) -> &'static str {
        match self {
            DoorcamEvent::MotionDetected { .. } => "motion_detected",
            DoorcamEvent::FrameReady { .. } => "frame_ready",
            DoorcamEvent::TouchDetected { .. } => "touch_detected",
            DoorcamEvent::CaptureStarted { .. } => "capture_started",
            DoorcamEvent::CaptureCompleted { .. } => "capture_completed",
            DoorcamEvent::SystemError { .. } => "system_error",
            DoorcamEvent::DisplayActivate { .. } => "display_activate",
            DoorcamEvent::DisplayDeactivate { .. } => "display_deactivate",
            DoorcamEvent::CameraStatusChanged { .. } => "camera_status_changed",
            DoorcamEvent::ShutdownRequested { .. } => "shutdown_requested",
        }
    }
}

/// Async event bus for component coordination using broadcast channels
pub struct EventBus {
    sender: broadcast::Sender<DoorcamEvent>,
    debug_logging: bool,
}

impl EventBus {
    /// Create a new event bus with the specified channel capacity
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            debug_logging: false,
        }
    }

    /// Create a new event bus with debug logging enabled
    pub fn with_debug_logging(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            debug_logging: true,
        }
    }

    /// Subscribe to events and get a receiver
    pub fn subscribe(&self) -> broadcast::Receiver<DoorcamEvent> {
        self.sender.subscribe()
    }

    /// Publish an event to all subscribers
    pub async fn publish(&self, event: DoorcamEvent) -> Result<usize, EventBusError> {
        if self.debug_logging {
            debug!("Publishing event: {}", event.description());
        }

        // Log important events at appropriate levels
        match &event {
            DoorcamEvent::MotionDetected { contour_area, .. } => {
                info!("Motion detected with area: {:.2}", contour_area);
            }
            DoorcamEvent::SystemError { component, error } => {
                error!("System error in {}: {}", component, error);
            }
            DoorcamEvent::CameraStatusChanged { connected, .. } => {
                if *connected {
                    info!("Camera connected");
                } else {
                    warn!("Camera disconnected");
                }
            }
            DoorcamEvent::ShutdownRequested { reason, .. } => {
                info!("Shutdown requested: {}", reason);
            }
            _ => {
                if self.debug_logging {
                    debug!("Event: {}", event.description());
                }
            }
        }

        self.sender
            .send(event)
            .map_err(|e| EventBusError::PublishFailed {
                details: e.to_string(),
            })
    }

    /// Get the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Check if there are any active subscribers
    pub fn has_subscribers(&self) -> bool {
        self.sender.receiver_count() > 0
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            debug_logging: self.debug_logging,
        }
    }
}

/// Event filter for selective event handling
#[derive(Debug, Clone)]
pub enum EventFilter {
    /// Accept all events
    All,
    /// Accept only specific event types
    EventTypes(Vec<&'static str>),
    /// Accept events from specific components (for SystemError events)
    Components(Vec<String>),
    /// Custom filter function
    Custom(fn(&DoorcamEvent) -> bool),
}

impl EventFilter {
    /// Check if an event passes this filter
    pub fn matches(&self, event: &DoorcamEvent) -> bool {
        match self {
            EventFilter::All => true,
            EventFilter::EventTypes(types) => types.contains(&event.event_type()),
            EventFilter::Components(components) => {
                if let DoorcamEvent::SystemError { component, .. } = event {
                    components.contains(component)
                } else {
                    false
                }
            }
            EventFilter::Custom(filter_fn) => filter_fn(event),
        }
    }
}

/// Event receiver with filtering and routing capabilities
pub struct EventReceiver {
    receiver: broadcast::Receiver<DoorcamEvent>,
    filter: EventFilter,
    name: String,
}

impl EventReceiver {
    /// Create a new event receiver with a filter
    pub fn new(
        receiver: broadcast::Receiver<DoorcamEvent>,
        filter: EventFilter,
        name: String,
    ) -> Self {
        Self {
            receiver,
            filter,
            name,
        }
    }

    /// Receive the next filtered event
    pub async fn recv(&mut self) -> Result<DoorcamEvent, EventBusError> {
        loop {
            match self.receiver.recv().await {
                Ok(event) => {
                    if self.filter.matches(&event) {
                        debug!(
                            "Receiver '{}' received event: {}",
                            self.name,
                            event.description()
                        );
                        return Ok(event);
                    }
                    // Continue loop to get next event if this one doesn't match filter
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("Receiver '{}' lagged behind by {} events", self.name, n);
                    return Err(EventBusError::PublishFailed {
                        details: format!("Receiver lagged behind by {} events", n),
                    });
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("Event bus closed for receiver '{}'", self.name);
                    return Err(EventBusError::ChannelClosed);
                }
            }
        }
    }

    /// Try to receive an event without blocking
    pub fn try_recv(&mut self) -> Result<Option<DoorcamEvent>, EventBusError> {
        loop {
            match self.receiver.try_recv() {
                Ok(event) => {
                    if self.filter.matches(&event) {
                        debug!(
                            "Receiver '{}' received event: {}",
                            self.name,
                            event.description()
                        );
                        return Ok(Some(event));
                    }
                    // Continue loop to check next event
                }
                Err(broadcast::error::TryRecvError::Empty) => {
                    return Ok(None);
                }
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    warn!("Receiver '{}' lagged behind by {} events", self.name, n);
                    return Err(EventBusError::PublishFailed {
                        details: format!("Receiver lagged behind by {} events", n),
                    });
                }
                Err(broadcast::error::TryRecvError::Closed) => {
                    debug!("Event bus closed for receiver '{}'", self.name);
                    return Err(EventBusError::ChannelClosed);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_event_bus_basic_operations() {
        let event_bus = EventBus::new(10);
        let mut receiver = event_bus.subscribe();

        let event = DoorcamEvent::MotionDetected {
            contour_area: 1500.0,
            timestamp: SystemTime::now(),
        };

        // Publish event
        let subscriber_count = event_bus.publish(event.clone()).await.unwrap();
        assert_eq!(subscriber_count, 1);

        // Receive event
        let received_event = receiver.recv().await.unwrap();
        match received_event {
            DoorcamEvent::MotionDetected { contour_area, .. } => {
                assert_eq!(contour_area, 1500.0);
            }
            _ => panic!("Unexpected event type"),
        }
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let event_bus = EventBus::new(10);
        let mut receiver1 = event_bus.subscribe();
        let mut receiver2 = event_bus.subscribe();

        assert_eq!(event_bus.subscriber_count(), 2);

        let event = DoorcamEvent::TouchDetected {
            timestamp: SystemTime::now(),
        };

        event_bus.publish(event).await.unwrap();

        // Both receivers should get the event
        let _ = timeout(Duration::from_millis(100), receiver1.recv())
            .await
            .unwrap()
            .unwrap();
        let _ = timeout(Duration::from_millis(100), receiver2.recv())
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn test_event_filter() {
        let filter = EventFilter::EventTypes(vec!["motion_detected", "touch_detected"]);

        let motion_event = DoorcamEvent::MotionDetected {
            contour_area: 1000.0,
            timestamp: SystemTime::now(),
        };

        let frame_event = DoorcamEvent::FrameReady {
            frame_id: 1,
            timestamp: SystemTime::now(),
        };

        assert!(filter.matches(&motion_event));
        assert!(!filter.matches(&frame_event));
    }

    #[tokio::test]
    async fn test_filtered_receiver() {
        let event_bus = EventBus::new(10);
        let receiver = event_bus.subscribe();
        let filter = EventFilter::EventTypes(vec!["motion_detected"]);
        let mut filtered_receiver = EventReceiver::new(receiver, filter, "test".to_string());

        // Publish events of different types
        event_bus
            .publish(DoorcamEvent::FrameReady {
                frame_id: 1,
                timestamp: SystemTime::now(),
            })
            .await
            .unwrap();

        event_bus
            .publish(DoorcamEvent::MotionDetected {
                contour_area: 2000.0,
                timestamp: SystemTime::now(),
            })
            .await
            .unwrap();

        // Should only receive the motion event
        let received = timeout(Duration::from_millis(100), filtered_receiver.recv())
            .await
            .unwrap()
            .unwrap();
        match received {
            DoorcamEvent::MotionDetected { contour_area, .. } => {
                assert_eq!(contour_area, 2000.0);
            }
            _ => panic!("Unexpected event type"),
        }
    }

    #[test]
    fn test_event_properties() {
        let event = DoorcamEvent::MotionDetected {
            contour_area: 1500.0,
            timestamp: SystemTime::now(),
        };

        assert_eq!(event.event_type(), "motion_detected");
        assert!(event.description().contains("1500.00"));
    }
}

/// Event router for directing events to specific handlers
pub struct EventRouter {
    routes: Vec<EventRoute>,
}

/// A route definition for event handling
pub struct EventRoute {
    pub filter: EventFilter,
    pub handler_name: String,
    pub handler: Box<dyn Fn(DoorcamEvent) -> tokio::task::JoinHandle<()> + Send + Sync>,
}

impl Default for EventRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl EventRouter {
    /// Create a new event router
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    /// Add a route for handling specific events
    pub fn add_route<F, Fut>(&mut self, filter: EventFilter, handler_name: String, handler: F)
    where
        F: Fn(DoorcamEvent) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let boxed_handler = Box::new(move |event: DoorcamEvent| tokio::spawn(handler(event)));

        self.routes.push(EventRoute {
            filter,
            handler_name,
            handler: boxed_handler,
        });
    }

    /// Route an event to all matching handlers
    pub fn route_event(&self, event: DoorcamEvent) -> Vec<tokio::task::JoinHandle<()>> {
        let mut handles = Vec::new();

        for route in &self.routes {
            if route.filter.matches(&event) {
                debug!("Routing event to handler: {}", route.handler_name);
                let handle = (route.handler)(event.clone());
                handles.push(handle);
            }
        }

        handles
    }

    /// Get the number of routes
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }
}

/// Event handler trait for components that need to handle events
#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    /// Handle an incoming event
    async fn handle_event(&mut self, event: DoorcamEvent) -> Result<(), EventBusError>;

    /// Get the name of this handler for logging
    fn handler_name(&self) -> &str;

    /// Get the event filter for this handler
    fn event_filter(&self) -> EventFilter;
}

/// Event processing pipeline for complex event handling workflows
pub struct EventPipeline {
    stages: Vec<Box<dyn EventProcessor + Send + Sync>>,
    name: String,
}

/// Trait for event processing stages
#[async_trait::async_trait]
pub trait EventProcessor: Send + Sync {
    /// Process an event and optionally transform it
    async fn process(&self, event: DoorcamEvent) -> Result<Option<DoorcamEvent>, EventBusError>;

    /// Get the name of this processor
    fn processor_name(&self) -> &str;
}

impl EventPipeline {
    /// Create a new event processing pipeline
    pub fn new(name: String) -> Self {
        Self {
            stages: Vec::new(),
            name,
        }
    }

    /// Add a processing stage to the pipeline
    pub fn add_stage<P: EventProcessor + Send + Sync + 'static>(&mut self, processor: P) {
        self.stages.push(Box::new(processor));
    }

    /// Process an event through all stages
    pub async fn process_event(
        &self,
        mut event: DoorcamEvent,
    ) -> Result<Option<DoorcamEvent>, EventBusError> {
        debug!("Processing event through pipeline: {}", self.name);

        for (i, stage) in self.stages.iter().enumerate() {
            match stage.process(event).await? {
                Some(processed_event) => {
                    debug!("Stage {} ({}) processed event", i, stage.processor_name());
                    event = processed_event;
                }
                None => {
                    debug!(
                        "Stage {} ({}) filtered out event",
                        i,
                        stage.processor_name()
                    );
                    return Ok(None);
                }
            }
        }

        Ok(Some(event))
    }
}

/// Event metrics collector for monitoring and debugging
#[derive(Debug, Default)]
pub struct EventMetrics {
    pub total_events: u64,
    pub events_by_type: std::collections::HashMap<&'static str, u64>,
    pub errors: u64,
    pub last_event_time: Option<SystemTime>,
}

impl EventMetrics {
    /// Record an event
    pub fn record_event(&mut self, event: &DoorcamEvent) {
        self.total_events += 1;
        *self.events_by_type.entry(event.event_type()).or_insert(0) += 1;
        self.last_event_time = Some(event.timestamp());
    }

    /// Record an error
    pub fn record_error(&mut self) {
        self.errors += 1;
    }

    /// Get events per second over the last period
    pub fn events_per_second(&self, period: std::time::Duration) -> f64 {
        if let Some(last_time) = self.last_event_time {
            if let Ok(elapsed) = last_time.elapsed() {
                if elapsed < period {
                    return self.total_events as f64 / elapsed.as_secs_f64();
                }
            }
        }
        0.0
    }

    /// Reset all metrics
    pub fn reset(&mut self) {
        self.total_events = 0;
        self.events_by_type.clear();
        self.errors = 0;
        self.last_event_time = None;
    }
}

/// Event debugging utilities
pub struct EventDebugger {
    event_history: std::collections::VecDeque<(SystemTime, DoorcamEvent)>,
    max_history: usize,
    metrics: EventMetrics,
}

impl EventDebugger {
    /// Create a new event debugger
    pub fn new(max_history: usize) -> Self {
        Self {
            event_history: std::collections::VecDeque::with_capacity(max_history),
            max_history,
            metrics: EventMetrics::default(),
        }
    }

    /// Record an event for debugging
    pub fn record_event(&mut self, event: DoorcamEvent) {
        let timestamp = SystemTime::now();

        // Update metrics
        self.metrics.record_event(&event);

        // Add to history
        self.event_history.push_back((timestamp, event));

        // Maintain max history size
        while self.event_history.len() > self.max_history {
            self.event_history.pop_front();
        }
    }

    /// Get recent events of a specific type
    pub fn get_recent_events(&self, event_type: &str, count: usize) -> Vec<&DoorcamEvent> {
        self.event_history
            .iter()
            .rev()
            .filter(|(_, event)| event.event_type() == event_type)
            .take(count)
            .map(|(_, event)| event)
            .collect()
    }

    /// Get all events in the last duration
    pub fn get_events_since(&self, duration: std::time::Duration) -> Vec<&DoorcamEvent> {
        let cutoff = SystemTime::now() - duration;

        self.event_history
            .iter()
            .filter(|(timestamp, _)| *timestamp >= cutoff)
            .map(|(_, event)| event)
            .collect()
    }

    /// Get current metrics
    pub fn metrics(&self) -> &EventMetrics {
        &self.metrics
    }

    /// Print debug summary
    pub fn print_summary(&self) {
        info!("Event Debug Summary:");
        info!("  Total events: {}", self.metrics.total_events);
        info!("  Errors: {}", self.metrics.errors);
        info!("  History size: {}", self.event_history.len());

        for (event_type, count) in &self.metrics.events_by_type {
            info!("  {}: {}", event_type, count);
        }
    }
}

/// Convenience functions for common event handling patterns
pub mod patterns {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Create a simple event handler that logs all events
    pub fn create_logging_handler(name: String) -> impl EventHandler {
        LoggingHandler { name }
    }

    /// Create an event handler that filters and forwards events to another bus
    pub fn create_forwarding_handler(
        target_bus: Arc<EventBus>,
        filter: EventFilter,
        name: String,
    ) -> impl EventHandler {
        ForwardingHandler {
            target_bus,
            filter,
            name,
        }
    }

    /// Create an event handler that collects metrics
    pub fn create_metrics_handler(
        metrics: Arc<Mutex<EventMetrics>>,
        name: String,
    ) -> impl EventHandler {
        MetricsHandler { metrics, name }
    }

    struct LoggingHandler {
        name: String,
    }

    #[async_trait::async_trait]
    impl EventHandler for LoggingHandler {
        async fn handle_event(&mut self, event: DoorcamEvent) -> Result<(), EventBusError> {
            info!("[{}] Event: {}", self.name, event.description());
            Ok(())
        }

        fn handler_name(&self) -> &str {
            &self.name
        }

        fn event_filter(&self) -> EventFilter {
            EventFilter::All
        }
    }

    struct ForwardingHandler {
        target_bus: Arc<EventBus>,
        filter: EventFilter,
        name: String,
    }

    #[async_trait::async_trait]
    impl EventHandler for ForwardingHandler {
        async fn handle_event(&mut self, event: DoorcamEvent) -> Result<(), EventBusError> {
            if self.filter.matches(&event) {
                self.target_bus.publish(event).await?;
            }
            Ok(())
        }

        fn handler_name(&self) -> &str {
            &self.name
        }

        fn event_filter(&self) -> EventFilter {
            EventFilter::All
        }
    }

    struct MetricsHandler {
        metrics: Arc<Mutex<EventMetrics>>,
        name: String,
    }

    #[async_trait::async_trait]
    impl EventHandler for MetricsHandler {
        async fn handle_event(&mut self, event: DoorcamEvent) -> Result<(), EventBusError> {
            let mut metrics = self.metrics.lock().await;
            metrics.record_event(&event);
            Ok(())
        }

        fn handler_name(&self) -> &str {
            &self.name
        }

        fn event_filter(&self) -> EventFilter {
            EventFilter::All
        }
    }
}

#[cfg(test)]
mod pattern_tests {
    use super::patterns::*;
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_event_router() {
        let mut router = EventRouter::new();
        let received_events = Arc::new(Mutex::new(Vec::new()));

        let events_clone = Arc::clone(&received_events);
        router.add_route(
            EventFilter::EventTypes(vec!["motion_detected"]),
            "motion_handler".to_string(),
            move |event| {
                let events = Arc::clone(&events_clone);
                async move {
                    events.lock().await.push(event);
                }
            },
        );

        let motion_event = DoorcamEvent::MotionDetected {
            contour_area: 1000.0,
            timestamp: SystemTime::now(),
        };

        let frame_event = DoorcamEvent::FrameReady {
            frame_id: 1,
            timestamp: SystemTime::now(),
        };

        // Route events
        let handles1 = router.route_event(motion_event);
        let handles2 = router.route_event(frame_event);

        // Wait for handlers to complete
        for handle in handles1 {
            handle.await.unwrap();
        }
        for handle in handles2 {
            handle.await.unwrap();
        }

        // Check that only motion event was handled
        let events = received_events.lock().await;
        assert_eq!(events.len(), 1);
        match &events[0] {
            DoorcamEvent::MotionDetected { .. } => {}
            _ => panic!("Expected motion event"),
        }
    }

    #[tokio::test]
    async fn test_metrics_handler() {
        let metrics = Arc::new(Mutex::new(EventMetrics::default()));
        let mut handler = create_metrics_handler(Arc::clone(&metrics), "test".to_string());

        let event = DoorcamEvent::TouchDetected {
            timestamp: SystemTime::now(),
        };

        handler.handle_event(event).await.unwrap();

        let metrics_guard = metrics.lock().await;
        assert_eq!(metrics_guard.total_events, 1);
        assert_eq!(metrics_guard.events_by_type.get("touch_detected"), Some(&1));
    }

    #[tokio::test]
    async fn test_forwarding_handler() {
        let _source_bus = Arc::new(EventBus::new(10));
        let target_bus = Arc::new(EventBus::new(10));
        let mut target_receiver = target_bus.subscribe();

        let mut handler = create_forwarding_handler(
            Arc::clone(&target_bus),
            EventFilter::EventTypes(vec!["motion_detected"]),
            "forwarder".to_string(),
        );

        let motion_event = DoorcamEvent::MotionDetected {
            contour_area: 1500.0,
            timestamp: SystemTime::now(),
        };

        let touch_event = DoorcamEvent::TouchDetected {
            timestamp: SystemTime::now(),
        };

        // Handle events
        handler.handle_event(motion_event).await.unwrap();
        handler.handle_event(touch_event).await.unwrap();

        // Should only receive the motion event on target bus
        let received = timeout(Duration::from_millis(100), target_receiver.recv())
            .await
            .unwrap()
            .unwrap();
        match received {
            DoorcamEvent::MotionDetected { .. } => {}
            _ => panic!("Expected motion event"),
        }

        // Should not receive touch event
        assert!(timeout(Duration::from_millis(50), target_receiver.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_event_debugger() {
        let mut debugger = EventDebugger::new(5);

        let events = vec![
            DoorcamEvent::MotionDetected {
                contour_area: 1000.0,
                timestamp: SystemTime::now(),
            },
            DoorcamEvent::TouchDetected {
                timestamp: SystemTime::now(),
            },
            DoorcamEvent::FrameReady {
                frame_id: 1,
                timestamp: SystemTime::now(),
            },
        ];

        for event in events {
            debugger.record_event(event);
        }

        assert_eq!(debugger.metrics().total_events, 3);
        assert_eq!(debugger.get_recent_events("motion_detected", 10).len(), 1);
        assert_eq!(debugger.get_recent_events("touch_detected", 10).len(), 1);
    }

    #[tokio::test]
    async fn test_event_system_stress() {
        use std::sync::Arc;
        use tokio::time::Duration;

        let event_bus = Arc::new(EventBus::new(1000));
        let mut handles = Vec::new();

        // Spawn multiple publishers
        for publisher_id in 0..5 {
            let event_bus_clone = Arc::clone(&event_bus);
            let handle = tokio::spawn(async move {
                for i in 0..100 {
                    let event = DoorcamEvent::MotionDetected {
                        contour_area: (publisher_id * 100 + i) as f64,
                        timestamp: SystemTime::now(),
                    };
                    let _ = event_bus_clone.publish(event).await;

                    // Small delay to simulate realistic event rates
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            });
            handles.push(handle);
        }

        // Spawn multiple subscribers
        let received_events = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        for _ in 0..3 {
            let event_bus_clone = Arc::clone(&event_bus);
            let received_clone = Arc::clone(&received_events);
            let handle = tokio::spawn(async move {
                let mut receiver = event_bus_clone.subscribe();
                let mut count = 0;

                while count < 50 {
                    if let Ok(event) =
                        tokio::time::timeout(Duration::from_millis(100), receiver.recv()).await
                    {
                        if event.is_ok() {
                            received_clone.lock().await.push(event.unwrap());
                            count += 1;
                        }
                    } else {
                        break; // Timeout, stop receiving
                    }
                }
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            let _ = handle.await;
        }

        // Check that events were received
        let received = received_events.lock().await;
        assert!(!received.is_empty());
    }

    #[tokio::test]
    async fn test_event_error_handling() {
        let event_bus = EventBus::new(10); // Small buffer to test overflow

        // Create a subscriber first so events can be published
        let _receiver = event_bus.subscribe();

        // Test that the event bus handles publishing gracefully
        let mut successful_publishes = 0;

        for i in 0..15 {
            let event = DoorcamEvent::SystemError {
                component: format!("component_{}", i),
                error: "Test error".to_string(),
            };

            let result = event_bus.publish(event).await;
            if result.is_ok() {
                successful_publishes += 1;
            }
        }

        // Should have some successful publishes
        assert!(successful_publishes > 0);

        // Test that the event bus remains functional
        assert!(event_bus.has_subscribers());

        // Test that we can publish additional events
        let test_event = DoorcamEvent::SystemError {
            component: "test".to_string(),
            error: "Final test".to_string(),
        };

        // This should work since we have a subscriber
        let _result = event_bus.publish(test_event).await;
        // Result may vary depending on buffer state, but the operation should complete
    }
}
