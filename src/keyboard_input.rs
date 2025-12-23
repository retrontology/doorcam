use crate::error::Result;
use crate::events::{DoorcamEvent, EventBus};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::runtime::Handle;
use tokio::task;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Keyboard input handler for debugging motion events
pub struct KeyboardInputHandler {
    event_bus: Arc<EventBus>,
    cancellation_token: CancellationToken,
}

impl KeyboardInputHandler {
    /// Create a new keyboard input handler
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            event_bus,
            cancellation_token: CancellationToken::new(),
        }
    }

    /// Start listening for keyboard input
    pub async fn start(&self) -> Result<()> {
        info!("Starting keyboard input handler - press SPACE to trigger motion event");

        let event_bus = Arc::clone(&self.event_bus);
        let cancellation_token = self.cancellation_token.clone();
        let runtime_handle = Handle::current();

        // Spawn a blocking task to handle keyboard input
        task::spawn_blocking(move || {
            // Enable raw mode to capture individual key presses
            if let Err(e) = enable_raw_mode() {
                error!("Failed to enable raw mode for keyboard input: {}", e);
                return;
            }

            info!("Raw mode enabled - keyboard handler active");

            loop {
                // Check if we should stop
                if cancellation_token.is_cancelled() {
                    debug!("Keyboard input handler stopping");
                    break;
                }

                // Poll for keyboard events with a timeout
                match event::poll(Duration::from_millis(100)) {
                    Ok(true) => {
                        if let Ok(Event::Key(key_event)) = event::read() {
                            // Only handle key press events (not release)
                            if key_event.kind == KeyEventKind::Press {
                                match key_event.code {
                                    KeyCode::Char(' ') => {
                                        info!("Space bar pressed - triggering motion event");

                                        // Publish a motion detected event
                                        let motion_event = DoorcamEvent::MotionDetected {
                                            contour_area: 5000.0, // Simulated large motion area
                                            timestamp: SystemTime::now(),
                                        };

                                        let event_bus_clone = Arc::clone(&event_bus);
                                        runtime_handle.spawn(async move {
                                            if let Err(e) =
                                                event_bus_clone.publish(motion_event).await
                                            {
                                                warn!("Failed to publish motion event: {}", e);
                                            }
                                        });
                                    }
                                    KeyCode::Char('q') | KeyCode::Esc => {
                                        info!("Quit key pressed - requesting shutdown");

                                        let shutdown_event = DoorcamEvent::ShutdownRequested {
                                            timestamp: SystemTime::now(),
                                            reason: "User requested via keyboard".to_string(),
                                        };

                                        let event_bus_clone = Arc::clone(&event_bus);
                                        runtime_handle.spawn(async move {
                                            if let Err(e) =
                                                event_bus_clone.publish(shutdown_event).await
                                            {
                                                warn!("Failed to publish shutdown event: {}", e);
                                            }
                                        });
                                        break;
                                    }
                                    _ => {
                                        // Ignore other keys
                                        debug!("Key pressed: {:?}", key_event.code);
                                    }
                                }
                            }
                        }
                    }
                    Ok(false) => {
                        // No event available, continue polling
                    }
                    Err(e) => {
                        warn!("Error polling for keyboard events: {}", e);
                    }
                }
            }

            // Disable raw mode when exiting
            if let Err(e) = disable_raw_mode() {
                error!("Failed to disable raw mode: {}", e);
            } else {
                debug!("Raw mode disabled");
            }

            debug!("Keyboard input handler task exited");
        });

        Ok(())
    }

    /// Stop the keyboard input handler
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping keyboard input handler");
        self.cancellation_token.cancel();

        // Give the task a moment to clean up and disable raw mode
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Ensure raw mode is disabled even if the task didn't clean up properly
        let _ = disable_raw_mode();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_keyboard_handler_creation() {
        let event_bus = Arc::new(EventBus::new(100));
        let handler = KeyboardInputHandler::new(event_bus);

        // Just verify we can create the handler
        assert!(!handler.cancellation_token.is_cancelled());
    }

    #[tokio::test]
    async fn test_keyboard_handler_stop() {
        let event_bus = Arc::new(EventBus::new(100));
        let handler = KeyboardInputHandler::new(event_bus);

        handler.stop().await.unwrap();
        assert!(handler.cancellation_token.is_cancelled());
    }
}
