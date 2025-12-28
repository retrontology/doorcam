use crate::error::{DoorcamError, Result};
use crate::events::{DoorcamEvent, EventBus};
use std::sync::Arc;
use std::time::SystemTime;
use tracing::debug;

/// Mock touch input handler for testing without real hardware
pub struct MockTouchInputHandler {
    event_bus: Arc<EventBus>,
}

impl MockTouchInputHandler {
    /// Create a new mock touch input handler
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }

    /// Start the mock touch handler (generates periodic touch events for testing)
    pub async fn start(&self) -> Result<()> {
        let event_bus = Arc::clone(&self.event_bus);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));

            loop {
                interval.tick().await;

                debug!("Mock touch event generated");
                let _ = event_bus
                    .publish(DoorcamEvent::TouchDetected {
                        timestamp: SystemTime::now(),
                    })
                    .await;
            }
        });

        Ok(())
    }

    /// Trigger a mock touch event immediately
    pub async fn trigger_touch(&self) -> Result<()> {
        self.event_bus
            .publish(DoorcamEvent::TouchDetected {
                timestamp: SystemTime::now(),
            })
            .await
            .map_err(|e| DoorcamError::component("mock_touch".to_string(), e.to_string()))?;

        Ok(())
    }
}
