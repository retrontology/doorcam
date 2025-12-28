use crate::error::Result;
use crate::events::{DoorcamEvent, EventBus};
use crate::recovery::{ComponentHealth, GracefulDegradation, HealthMonitor};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// System health manager that monitors all components
pub struct SystemHealthManager {
    health_monitor: Arc<tokio::sync::Mutex<HealthMonitor>>,
    degradation: Arc<tokio::sync::Mutex<GracefulDegradation>>,
    event_bus: Arc<EventBus>,
    health_check_interval: Duration,
    _last_health_report: Instant,
    health_report_interval: Duration,
}

impl SystemHealthManager {
    /// Create a new system health manager
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            health_monitor: Arc::new(tokio::sync::Mutex::new(HealthMonitor::new())),
            degradation: Arc::new(tokio::sync::Mutex::new(GracefulDegradation::new())),
            event_bus,
            health_check_interval: Duration::from_secs(30),
            _last_health_report: Instant::now(),
            health_report_interval: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Start the health monitoring system
    pub async fn start(&self) -> Result<()> {
        info!("Starting system health monitoring");

        let health_monitor = Arc::clone(&self.health_monitor);
        let degradation = Arc::clone(&self.degradation);
        let event_bus = Arc::clone(&self.event_bus);
        let check_interval = self.health_check_interval;
        let report_interval = self.health_report_interval;

        tokio::spawn(async move {
            let mut interval = interval(check_interval);
            let mut last_report = Instant::now();

            loop {
                interval.tick().await;

                // Perform health checks
                if let Err(e) =
                    Self::perform_health_checks(&health_monitor, &degradation, &event_bus).await
                {
                    error!("Health check failed: {}", e);
                }

                // Periodic health reports
                if last_report.elapsed() >= report_interval {
                    if let Err(e) = Self::generate_health_report(&health_monitor).await {
                        error!("Failed to generate health report: {}", e);
                    }
                    last_report = Instant::now();
                }
            }
        });

        Ok(())
    }

    /// Update component health status
    pub async fn update_component_health(&self, component: &str, health: ComponentHealth) {
        let mut monitor = self.health_monitor.lock().await;
        monitor.update_component_health(component, health);

        // Check if we need to take action based on component failure
        if health == ComponentHealth::Failed {
            let degradation = self.degradation.lock().await;
            if degradation.should_shutdown_on_failure(component) {
                error!(
                    "Essential component {} failed, system shutdown required",
                    component
                );

                // Publish system error event
                let _ = self
                    .event_bus
                    .publish(DoorcamEvent::SystemError {
                        component: component.to_string(),
                        error: format!("Essential component {} failed", component),
                    })
                    .await;
            } else if degradation.is_optional_component(component) {
                warn!(
                    "Optional component {} failed, continuing with degraded functionality",
                    component
                );
            }
        }
    }

    /// Get current system health status
    pub async fn get_system_health(&self) -> ComponentHealth {
        let monitor = self.health_monitor.lock().await;
        monitor.get_system_health()
    }

    /// Get health status for a specific component
    pub async fn get_component_health(&self, component: &str) -> ComponentHealth {
        let monitor = self.health_monitor.lock().await;
        monitor.get_component_health(component)
    }

    /// Check if system should shutdown due to component failures
    pub async fn should_shutdown(&self) -> bool {
        let monitor = self.health_monitor.lock().await;
        let degradation = self.degradation.lock().await;

        for (component, health) in monitor.get_all_component_status() {
            if *health == ComponentHealth::Failed
                && degradation.should_shutdown_on_failure(component)
            {
                return true;
            }
        }

        false
    }

    /// Perform health checks on all components
    async fn perform_health_checks(
        health_monitor: &Arc<tokio::sync::Mutex<HealthMonitor>>,
        _degradation: &Arc<tokio::sync::Mutex<GracefulDegradation>>,
        event_bus: &Arc<EventBus>,
    ) -> Result<()> {
        let mut monitor = health_monitor.lock().await;

        if !monitor.should_check_health() {
            return Ok(());
        }

        debug!("Performing system health checks");

        // Check event bus health
        let event_bus_health = Self::check_event_bus_health(event_bus).await;
        monitor.update_component_health("event_bus", event_bus_health);

        // Check ring buffer health (would need access to ring buffer)
        // For now, assume healthy if no errors reported
        monitor.update_component_health("ring_buffer", ComponentHealth::Healthy);

        // Other component health checks would be performed here
        // This would typically involve calling health check methods on each component

        monitor.mark_health_check_completed();

        Ok(())
    }

    /// Check event bus health
    async fn check_event_bus_health(event_bus: &Arc<EventBus>) -> ComponentHealth {
        // Try to publish a test event
        match event_bus
            .publish(DoorcamEvent::SystemError {
                component: "health_check".to_string(),
                error: "test_event".to_string(),
            })
            .await
        {
            Ok(_) => ComponentHealth::Healthy,
            Err(_) => ComponentHealth::Failed,
        }
    }

    /// Generate periodic health report
    async fn generate_health_report(
        health_monitor: &Arc<tokio::sync::Mutex<HealthMonitor>>,
    ) -> Result<()> {
        let monitor = health_monitor.lock().await;
        let system_health = monitor.get_system_health();
        let component_statuses = monitor.get_all_component_status();

        info!("=== System Health Report ===");
        info!("Overall system health: {:?}", system_health);

        for (component, health) in component_statuses {
            match health {
                ComponentHealth::Healthy => {
                    debug!("Component {}: Healthy", component);
                }
                ComponentHealth::Degraded => {
                    warn!("Component {}: Degraded", component);
                }
                ComponentHealth::Failed => {
                    error!("Component {}: Failed", component);
                }
                ComponentHealth::Unknown => {
                    warn!("Component {}: Unknown status", component);
                }
            }
        }

        info!("=== End Health Report ===");

        Ok(())
    }
}

/// Component health checker trait
pub trait HealthChecker {
    /// Perform a health check on the component
    fn check_health(&self) -> impl std::future::Future<Output = ComponentHealth> + Send;

    /// Get the component name for health monitoring
    fn component_name(&self) -> &'static str;
}

/// Health check result with details
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub component: String,
    pub health: ComponentHealth,
    pub details: Option<String>,
    pub timestamp: std::time::SystemTime,
}

impl HealthCheckResult {
    /// Create a new health check result
    pub fn new(component: String, health: ComponentHealth) -> Self {
        Self {
            component,
            health,
            details: None,
            timestamp: std::time::SystemTime::now(),
        }
    }

    /// Create a health check result with details
    pub fn with_details(component: String, health: ComponentHealth, details: String) -> Self {
        Self {
            component,
            health,
            details: Some(details),
            timestamp: std::time::SystemTime::now(),
        }
    }
}

/// System metrics collector
pub struct SystemMetrics {
    component_error_counts: std::collections::HashMap<String, u64>,
    component_recovery_counts: std::collections::HashMap<String, u64>,
    last_reset: Instant,
}

impl Default for SystemMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemMetrics {
    /// Create a new system metrics collector
    pub fn new() -> Self {
        Self {
            component_error_counts: std::collections::HashMap::new(),
            component_recovery_counts: std::collections::HashMap::new(),
            last_reset: Instant::now(),
        }
    }

    /// Record an error for a component
    pub fn record_error(&mut self, component: &str) {
        *self
            .component_error_counts
            .entry(component.to_string())
            .or_insert(0) += 1;
    }

    /// Record a successful recovery for a component
    pub fn record_recovery(&mut self, component: &str) {
        *self
            .component_recovery_counts
            .entry(component.to_string())
            .or_insert(0) += 1;
    }

    /// Get error count for a component
    pub fn get_error_count(&self, component: &str) -> u64 {
        self.component_error_counts
            .get(component)
            .copied()
            .unwrap_or(0)
    }

    /// Get recovery count for a component
    pub fn get_recovery_count(&self, component: &str) -> u64 {
        self.component_recovery_counts
            .get(component)
            .copied()
            .unwrap_or(0)
    }

    /// Reset all metrics
    pub fn reset(&mut self) {
        self.component_error_counts.clear();
        self.component_recovery_counts.clear();
        self.last_reset = Instant::now();
    }

    /// Get time since last reset
    pub fn time_since_reset(&self) -> Duration {
        self.last_reset.elapsed()
    }

    /// Generate metrics report
    pub fn generate_report(&self) -> String {
        let mut report = String::new();
        report.push_str("=== System Metrics Report ===\n");
        report.push_str(&format!(
            "Time since reset: {:?}\n",
            self.time_since_reset()
        ));

        report.push_str("\nError Counts:\n");
        for (component, count) in &self.component_error_counts {
            report.push_str(&format!("  {}: {}\n", component, count));
        }

        report.push_str("\nRecovery Counts:\n");
        for (component, count) in &self.component_recovery_counts {
            report.push_str(&format!("  {}: {}\n", component, count));
        }

        report.push_str("=== End Metrics Report ===");
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventBus;

    #[tokio::test]
    async fn test_system_health_manager() {
        let event_bus = Arc::new(EventBus::new());
        let health_manager = SystemHealthManager::new(event_bus);

        // Initial system health should be unknown
        let initial_health = health_manager.get_system_health().await;
        assert_eq!(initial_health, ComponentHealth::Unknown);

        // Update component health
        health_manager
            .update_component_health("camera", ComponentHealth::Healthy)
            .await;
        health_manager
            .update_component_health("display", ComponentHealth::Degraded)
            .await;

        // System health should be degraded due to display
        let system_health = health_manager.get_system_health().await;
        assert_eq!(system_health, ComponentHealth::Degraded);

        // Check individual component health
        let camera_health = health_manager.get_component_health("camera").await;
        assert_eq!(camera_health, ComponentHealth::Healthy);

        let display_health = health_manager.get_component_health("display").await;
        assert_eq!(display_health, ComponentHealth::Degraded);
    }

    #[test]
    fn test_system_metrics() {
        let mut metrics = SystemMetrics::new();

        // Record some errors and recoveries
        metrics.record_error("camera");
        metrics.record_error("camera");
        metrics.record_recovery("camera");

        assert_eq!(metrics.get_error_count("camera"), 2);
        assert_eq!(metrics.get_recovery_count("camera"), 1);
        assert_eq!(metrics.get_error_count("display"), 0);

        // Test reset
        metrics.reset();
        assert_eq!(metrics.get_error_count("camera"), 0);
        assert_eq!(metrics.get_recovery_count("camera"), 0);
    }
}
