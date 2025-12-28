use super::{ComponentState, DoorcamOrchestrator};
use crate::error::{DoorcamError, Result};
use std::time::Duration;
use tokio::time::timeout;
use tracing::{error, info};

impl DoorcamOrchestrator {
    /// Perform graceful shutdown of all components
    pub async fn shutdown(&mut self) -> Result<i32> {
        info!("Beginning graceful shutdown");

        // Cancel all background tasks
        self.cancellation_token.cancel();

        let mut exit_code = 0;

        // Stop components in reverse dependency order
        if self.keyboard_enabled {
            if let Err(e) = self.stop_component("keyboard").await {
                error!("Error stopping keyboard: {}", e);
                exit_code = 1;
            }
        }

        if let Err(e) = self.stop_component("streaming").await {
            error!("Error stopping streaming: {}", e);
            exit_code = 1;
        }

        if let Err(e) = self.stop_component("capture").await {
            error!("Error stopping capture: {}", e);
            exit_code = 1;
        }

        if let Err(e) = self.stop_component("display").await {
            error!("Error stopping display: {}", e);
            exit_code = 1;
        }

        if let Err(e) = self.stop_component("analyzer").await {
            error!("Error stopping analyzer: {}", e);
            exit_code = 1;
        }

        if let Err(e) = self.stop_component("camera").await {
            error!("Error stopping camera: {}", e);
            exit_code = 1;
        }

        if let Err(e) = self.stop_component("storage").await {
            error!("Error stopping storage: {}", e);
            exit_code = 1;
        }

        info!("Graceful shutdown completed with exit code: {}", exit_code);
        Ok(exit_code)
    }

    /// Stop a specific component
    async fn stop_component(&mut self, component: &str) -> Result<()> {
        info!("Stopping {} component", component);
        self.set_component_state(component, ComponentState::Stopping)
            .await;

        match component {
            "camera" => {
                if let Some(camera_integration) = &self.camera_integration {
                    match timeout(Duration::from_secs(10), camera_integration.stop()).await {
                        Ok(Ok(())) => {
                            self.set_component_state(component, ComponentState::Stopped)
                                .await;
                            info!("{} component stopped", component);
                            Ok(())
                        }
                        Ok(Err(e)) => {
                            self.set_component_state(component, ComponentState::Failed)
                                .await;
                            error!("Error stopping {} component: {}", component, e);
                            Err(e)
                        }
                        Err(_) => {
                            self.set_component_state(component, ComponentState::Failed)
                                .await;
                            let err = DoorcamError::System {
                                message: format!("{} component stop timeout", component),
                            };
                            error!("{} component stop timeout", component);
                            Err(err)
                        }
                    }
                } else {
                    self.set_component_state(component, ComponentState::Stopped)
                        .await;
                    Ok(())
                }
            }
            "analyzer" => {
                if let Some(analyzer_integration) = &self.analyzer_integration {
                    let mut analyzer = analyzer_integration.lock().await;
                    match timeout(Duration::from_secs(10), analyzer.stop()).await {
                        Ok(Ok(())) => {
                            self.set_component_state(component, ComponentState::Stopped)
                                .await;
                            info!("{} component stopped", component);
                            Ok(())
                        }
                        Ok(Err(e)) => {
                            self.set_component_state(component, ComponentState::Failed)
                                .await;
                            error!("Error stopping {} component: {}", component, e);
                            Err(e)
                        }
                        Err(_) => {
                            self.set_component_state(component, ComponentState::Failed)
                                .await;
                            let err = DoorcamError::System {
                                message: format!("{} component stop timeout", component),
                            };
                            error!("{} component stop timeout", component);
                            Err(err)
                        }
                    }
                } else {
                    self.set_component_state(component, ComponentState::Stopped)
                        .await;
                    Ok(())
                }
            }
            "capture" => {
                if let Some(capture_integration) = &self.capture_integration {
                    match timeout(Duration::from_secs(5), capture_integration.stop()).await {
                        Ok(Ok(())) => {
                            self.set_component_state(component, ComponentState::Stopped)
                                .await;
                            info!("{} component stopped", component);
                            Ok(())
                        }
                        Ok(Err(e)) => {
                            self.set_component_state(component, ComponentState::Failed)
                                .await;
                            error!("Error stopping {} component: {}", component, e);
                            Err(e)
                        }
                        Err(_) => {
                            self.set_component_state(component, ComponentState::Failed)
                                .await;
                            let err = DoorcamError::System {
                                message: format!("{} component stop timeout", component),
                            };
                            error!("{} component stop timeout", component);
                            Err(err)
                        }
                    }
                } else {
                    self.set_component_state(component, ComponentState::Stopped)
                        .await;
                    Ok(())
                }
            }
            "display" => {
                if let Some(display_integration) = &self.display_integration {
                    match timeout(Duration::from_secs(5), display_integration.stop()).await {
                        Ok(Ok(())) => {
                            self.set_component_state(component, ComponentState::Stopped)
                                .await;
                            info!("{} component stopped", component);
                            Ok(())
                        }
                        Ok(Err(e)) => {
                            self.set_component_state(component, ComponentState::Failed)
                                .await;
                            error!("Error stopping {} component: {}", component, e);
                            Err(e)
                        }
                        Err(_) => {
                            self.set_component_state(component, ComponentState::Failed)
                                .await;
                            let err = DoorcamError::System {
                                message: format!("{} component stop timeout", component),
                            };
                            error!("{} component stop timeout", component);
                            Err(err)
                        }
                    }
                } else {
                    self.set_component_state(component, ComponentState::Stopped)
                        .await;
                    Ok(())
                }
            }
            "storage" => {
                if let Some(storage_integration) = &self.storage_integration {
                    match timeout(Duration::from_secs(5), storage_integration.stop()).await {
                        Ok(Ok(())) => {
                            self.set_component_state(component, ComponentState::Stopped)
                                .await;
                            info!("{} component stopped", component);
                            Ok(())
                        }
                        Ok(Err(e)) => {
                            self.set_component_state(component, ComponentState::Failed)
                                .await;
                            error!("Error stopping {} component: {}", component, e);
                            Err(e)
                        }
                        Err(_) => {
                            self.set_component_state(component, ComponentState::Failed)
                                .await;
                            let err = DoorcamError::System {
                                message: format!("{} component stop timeout", component),
                            };
                            error!("{} component stop timeout", component);
                            Err(err)
                        }
                    }
                } else {
                    self.set_component_state(component, ComponentState::Stopped)
                        .await;
                    Ok(())
                }
            }
            "keyboard" => {
                if let Some(keyboard_handler) = &self.keyboard_handler {
                    match timeout(Duration::from_secs(2), keyboard_handler.stop()).await {
                        Ok(Ok(())) => {
                            self.set_component_state(component, ComponentState::Stopped)
                                .await;
                            info!("{} component stopped", component);
                            Ok(())
                        }
                        Ok(Err(e)) => {
                            self.set_component_state(component, ComponentState::Failed)
                                .await;
                            error!("Error stopping {} component: {}", component, e);
                            Err(e)
                        }
                        Err(_) => {
                            self.set_component_state(component, ComponentState::Failed)
                                .await;
                            let err = DoorcamError::System {
                                message: format!("{} component stop timeout", component),
                            };
                            error!("{} component stop timeout", component);
                            Err(err)
                        }
                    }
                } else {
                    self.set_component_state(component, ComponentState::Stopped)
                        .await;
                    Ok(())
                }
            }
            _ => {
                // For other components, just simulate a graceful stop with timeout
                match timeout(Duration::from_secs(5), async {
                    // Simulate component shutdown work
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok(())
                })
                .await
                {
                    Ok(Ok(())) => {
                        self.set_component_state(component, ComponentState::Stopped)
                            .await;
                        info!("{} component stopped", component);
                        Ok(())
                    }
                    Ok(Err(e)) => {
                        self.set_component_state(component, ComponentState::Failed)
                            .await;
                        error!("Error stopping {} component: {}", component, e);
                        Err(e)
                    }
                    Err(_) => {
                        self.set_component_state(component, ComponentState::Failed)
                            .await;
                        let err = DoorcamError::System {
                            message: format!("{} component stop timeout", component),
                        };
                        error!("{} component stop timeout", component);
                        Err(err)
                    }
                }
            }
        }
    }
}
