use super::{ComponentState, DoorcamOrchestrator};
use crate::error::Result;
use crate::streaming::StreamServer;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

impl DoorcamOrchestrator {
    /// Initialize all system components
    pub async fn initialize(&mut self) -> Result<()> {
        info!("Initializing Doorcam system components");

        // Set initial component states
        let mut states = self.component_states.lock().await;
        states.insert("camera".to_string(), ComponentState::Stopped);
        states.insert("analyzer".to_string(), ComponentState::Stopped);
        states.insert("display".to_string(), ComponentState::Stopped);
        states.insert("capture".to_string(), ComponentState::Stopped);
        states.insert("storage".to_string(), ComponentState::Stopped);

        // Only register keyboard component if enabled
        if self.keyboard_enabled {
            states.insert("keyboard".to_string(), ComponentState::Stopped);
        }

        states.insert("streaming".to_string(), ComponentState::Stopped);

        drop(states);

        info!("All components initialized successfully");
        Ok(())
    }

    /// Start all system components
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting Doorcam system");

        // Start camera integration first
        if let Some(camera_integration) = &self.camera_integration {
            self.set_component_state("camera", ComponentState::Starting)
                .await;

            camera_integration.start().await.map_err(|e| {
                error!("Failed to start camera integration: {}", e);
                e
            })?;

            // Wait for frames to start flowing
            camera_integration
                .wait_for_frames(Duration::from_secs(5))
                .await
                .map_err(|e| {
                    error!("Camera failed to produce frames: {}", e);
                    e
                })?;

            self.set_component_state("camera", ComponentState::Running)
                .await;
            info!("Camera integration started successfully");
        }

        // Start streaming server if configured
        if let Some(_stream_server) = &self.stream_server {
            self.set_component_state("streaming", ComponentState::Starting)
                .await;

            // Use the ring buffer from camera integration if available
            let ring_buffer = if let Some(camera_integration) = &self.camera_integration {
                camera_integration.ring_buffer()
            } else {
                Arc::clone(&self.ring_buffer)
            };

            let server = StreamServer::new(
                self.config.stream.clone(),
                ring_buffer,
                Arc::clone(&self.event_bus),
                self.config.camera.fps,
            );

            // Start the server in a background task
            tokio::spawn(async move {
                if let Err(e) = server.start().await {
                    error!("Stream server error: {}", e);
                }
            });

            self.set_component_state("streaming", ComponentState::Running)
                .await;
            info!(
                "Streaming server started on {}:{}",
                self.config.stream.ip, self.config.stream.port
            );
        }

        // Start analyzer integration
        if let Some(analyzer_integration) = &self.analyzer_integration {
            self.set_component_state("analyzer", ComponentState::Starting)
                .await;

            let mut analyzer = analyzer_integration.lock().await;
            analyzer.start().await.map_err(|e| {
                error!("Failed to start analyzer integration: {}", e);
                e
            })?;

            self.set_component_state("analyzer", ComponentState::Running)
                .await;
            info!("Analyzer integration started successfully");
        }

        // Start display integration
        if let Some(display_integration) = &self.display_integration {
            self.set_component_state("display", ComponentState::Starting)
                .await;

            // Use the ring buffer from camera integration if available
            let ring_buffer = if let Some(camera_integration) = &self.camera_integration {
                camera_integration.ring_buffer()
            } else {
                Arc::clone(&self.ring_buffer)
            };

            display_integration.start(ring_buffer).await.map_err(|e| {
                error!("Failed to start display integration: {}", e);
                e
            })?;

            self.set_component_state("display", ComponentState::Running)
                .await;
            info!("Display integration started successfully");
        }

        // Start capture integration
        if let Some(capture_integration) = &self.capture_integration {
            self.set_component_state("capture", ComponentState::Starting)
                .await;

            capture_integration.start().await.map_err(|e| {
                error!("Failed to start capture integration: {}", e);
                e
            })?;

            self.set_component_state("capture", ComponentState::Running)
                .await;
            info!("Capture integration started successfully");
        }

        // Start storage integration
        if let Some(storage_integration) = &self.storage_integration {
            self.set_component_state("storage", ComponentState::Starting)
                .await;

            storage_integration.start().await.map_err(|e| {
                error!("Failed to start storage integration: {}", e);
                e
            })?;

            self.set_component_state("storage", ComponentState::Running)
                .await;
            info!("Storage integration started successfully");
        }

        // Start keyboard input handler for debugging (only if enabled)
        if self.keyboard_enabled {
            if let Some(keyboard_handler) = &self.keyboard_handler {
                self.set_component_state("keyboard", ComponentState::Starting)
                    .await;

                keyboard_handler.start().await.map_err(|e| {
                    error!("Failed to start keyboard handler: {}", e);
                    e
                })?;

                self.set_component_state("keyboard", ComponentState::Running)
                    .await;
                info!("Keyboard input handler started - press SPACE to trigger motion");
            }
        }

        info!("Doorcam system started successfully");
        Ok(())
    }
}
