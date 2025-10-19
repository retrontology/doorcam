use crate::config::DoorcamConfig;
use crate::error::{DoorcamError, Result};
use crate::events::EventBus;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{oneshot, Mutex};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// Component lifecycle states
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

/// System shutdown reason
#[derive(Debug, Clone)]
pub enum ShutdownReason {
    Signal(String),
    Error(String),
    UserRequest,
    HealthCheck,
}

/// Main application coordinator that manages all system components
pub struct DoorcamOrchestrator {
    config: DoorcamConfig,
    event_bus: Arc<EventBus>,
    
    // Lifecycle management
    component_states: Arc<Mutex<HashMap<String, ComponentState>>>,
    shutdown_sender: Option<oneshot::Sender<ShutdownReason>>,
    shutdown_receiver: Option<oneshot::Receiver<ShutdownReason>>,
}

impl DoorcamOrchestrator {
    /// Create a new orchestrator with the given configuration
    pub fn new(config: DoorcamConfig) -> Result<Self> {
        let event_bus = Arc::new(EventBus::new(config.system.event_bus_capacity));
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        
        Ok(Self {
            config,
            event_bus,
            component_states: Arc::new(Mutex::new(HashMap::new())),
            shutdown_sender: Some(shutdown_sender),
            shutdown_receiver: Some(shutdown_receiver),
        })
    }
    
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
        
        #[cfg(feature = "streaming")]
        states.insert("streaming".to_string(), ComponentState::Stopped);
        
        drop(states);
        
        info!("All components initialized successfully");
        Ok(())
    }
    
    /// Start all system components
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting Doorcam system");
        
        // For now, just mark components as running
        // TODO: Initialize actual components when integration builders are available
        self.set_component_state("camera", ComponentState::Running).await;
        self.set_component_state("analyzer", ComponentState::Running).await;
        self.set_component_state("display", ComponentState::Running).await;
        self.set_component_state("capture", ComponentState::Running).await;
        self.set_component_state("storage", ComponentState::Running).await;
        
        #[cfg(feature = "streaming")]
        self.set_component_state("streaming", ComponentState::Running).await;
        
        info!("Doorcam system started successfully");
        Ok(())
    }
    
    /// Run the main application loop with signal handling
    pub async fn run(&mut self) -> Result<i32> {
        info!("Doorcam system is running");
        
        // Set up signal handling for graceful shutdown
        let shutdown_sender = self.shutdown_sender.take()
            .ok_or_else(|| DoorcamError::System { message: "Shutdown sender already taken".to_string() })?;
        
        let shutdown_receiver = self.shutdown_receiver.take()
            .ok_or_else(|| DoorcamError::System { message: "Shutdown receiver already taken".to_string() })?;
        
        // Spawn signal handlers
        self.setup_signal_handlers(shutdown_sender).await;
        
        // Wait for shutdown signal
        let shutdown_reason = shutdown_receiver.await
            .map_err(|_| DoorcamError::System { message: "Shutdown channel closed unexpectedly".to_string() })?;
        
        info!("Shutdown initiated: {:?}", shutdown_reason);
        
        // Perform graceful shutdown
        let exit_code = self.shutdown().await?;
        
        info!("Doorcam system shutdown complete");
        Ok(exit_code)
    }
    
    /// Set up signal handlers for graceful shutdown
    async fn setup_signal_handlers(&self, shutdown_sender: oneshot::Sender<ShutdownReason>) {
        let shutdown_sender = Arc::new(Mutex::new(Some(shutdown_sender)));
        
        // Handle SIGTERM (systemd stop) - Unix only
        #[cfg(unix)]
        {
            let shutdown_sender_sigterm = Arc::clone(&shutdown_sender);
            tokio::spawn(async move {
                if let Ok(()) = signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("Failed to register SIGTERM handler")
                    .recv()
                    .await
                {
                    info!("Received SIGTERM signal");
                    if let Some(sender) = shutdown_sender_sigterm.lock().await.take() {
                        let _ = sender.send(ShutdownReason::Signal("SIGTERM".to_string()));
                    }
                }
            });
        }
        
        // Handle SIGINT (Ctrl+C) - Cross-platform
        let shutdown_sender_sigint = Arc::clone(&shutdown_sender);
        tokio::spawn(async move {
            if let Ok(()) = tokio::signal::ctrl_c().await {
                info!("Received SIGINT signal (Ctrl+C)");
                if let Some(sender) = shutdown_sender_sigint.lock().await.take() {
                    let _ = sender.send(ShutdownReason::Signal("SIGINT".to_string()));
                }
            }
        });
    }
    
    /// Perform graceful shutdown of all components
    async fn shutdown(&mut self) -> Result<i32> {
        info!("Beginning graceful shutdown");
        
        let mut exit_code = 0;
        
        // Stop components in reverse dependency order
        #[cfg(feature = "streaming")]
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
        self.set_component_state(component, ComponentState::Stopping).await;
        
        // TODO: Implement actual component stopping when integrations are available
        // For now, just simulate a graceful stop with timeout
        match timeout(Duration::from_secs(5), async {
            // Simulate component shutdown work
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(())
        }).await {
            Ok(Ok(())) => {
                self.set_component_state(component, ComponentState::Stopped).await;
                info!("{} component stopped", component);
                Ok(())
            }
            Ok(Err(e)) => {
                self.set_component_state(component, ComponentState::Failed).await;
                error!("Error stopping {} component: {}", component, e);
                Err(e)
            }
            Err(_) => {
                self.set_component_state(component, ComponentState::Failed).await;
                let err = DoorcamError::System { message: format!("{} component stop timeout", component) };
                error!("{} component stop timeout", component);
                Err(err)
            }
        }
    }
    
    /// Update component state
    async fn set_component_state(&self, component: &str, state: ComponentState) {
        let mut states = self.component_states.lock().await;
        states.insert(component.to_string(), state.clone());
        debug!("Component '{}' state changed to: {:?}", component, state);
    }
    
    /// Get component state
    pub async fn get_component_state(&self, component: &str) -> Option<ComponentState> {
        let states = self.component_states.lock().await;
        states.get(component).cloned()
    }
    
    /// Get all component states
    pub async fn get_all_component_states(&self) -> HashMap<String, ComponentState> {
        let states = self.component_states.lock().await;
        states.clone()
    }
}