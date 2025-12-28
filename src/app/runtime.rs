use super::{DoorcamOrchestrator, ShutdownReason};
use crate::error::{DoorcamError, Result};
use std::sync::Arc;
use tokio::signal;
use tokio::sync::{oneshot, Mutex};
use tracing::info;

impl DoorcamOrchestrator {
    /// Run the main application loop with signal handling
    pub async fn run(&mut self) -> Result<i32> {
        info!("Doorcam system is running");

        // Set up signal handling for graceful shutdown
        let shutdown_sender = self
            .shutdown_sender
            .take()
            .ok_or_else(|| DoorcamError::System {
                message: "Shutdown sender already taken".to_string(),
            })?;

        let shutdown_receiver =
            self.shutdown_receiver
                .take()
                .ok_or_else(|| DoorcamError::System {
                    message: "Shutdown receiver already taken".to_string(),
                })?;

        // Spawn signal handlers
        self.setup_signal_handlers(shutdown_sender).await;

        // Wait for shutdown signal
        let shutdown_reason = shutdown_receiver.await.map_err(|_| DoorcamError::System {
            message: "Shutdown channel closed unexpectedly".to_string(),
        })?;

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
                if let Some(()) = signal::unix::signal(signal::unix::SignalKind::terminate())
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
}
