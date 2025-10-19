use crate::error::{DoorcamError, CameraError, TouchError};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Recovery action to take after an error
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryAction {
    /// Retry the operation immediately
    Retry,
    /// Retry after a delay
    RetryAfterDelay(Duration),
    /// Continue operation without the failed component
    Continue,
    /// Shutdown the system gracefully
    Shutdown,
    /// No recovery needed
    None,
}

/// Recovery strategy configuration
#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Base delay for exponential backoff
    pub base_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Whether to use exponential backoff
    pub exponential_backoff: bool,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            exponential_backoff: true,
        }
    }
}

/// Component recovery manager
pub struct RecoveryManager {
    config: RecoveryConfig,
    retry_counts: std::collections::HashMap<String, u32>,
    last_retry_times: std::collections::HashMap<String, Instant>,
}

impl RecoveryManager {
    /// Create a new recovery manager with default configuration
    pub fn new() -> Self {
        Self::with_config(RecoveryConfig::default())
    }
    
    /// Create a new recovery manager with custom configuration
    pub fn with_config(config: RecoveryConfig) -> Self {
        Self {
            config,
            retry_counts: std::collections::HashMap::new(),
            last_retry_times: std::collections::HashMap::new(),
        }
    }
    
    /// Determine recovery action for an error
    pub fn handle_error(&mut self, component: &str, error: &DoorcamError) -> RecoveryAction {
        let retry_count = self.retry_counts.get(component).copied().unwrap_or(0);
        
        // Check if error is recoverable
        if !error.is_recoverable() {
            warn!("Non-recoverable error in {}: {}", component, error);
            return RecoveryAction::Shutdown;
        }
        
        // Check retry limit
        if retry_count >= self.config.max_retries {
            error!(
                "Maximum retries ({}) exceeded for component {}: {}",
                self.config.max_retries, component, error
            );
            return match component {
                "camera" => RecoveryAction::Continue, // Continue without camera
                "display" => RecoveryAction::Continue, // Continue without display
                "touch" => RecoveryAction::Continue, // Continue without touch
                "stream" => RecoveryAction::Continue, // Continue without streaming
                _ => RecoveryAction::Shutdown,
            };
        }
        
        // Increment retry count
        self.retry_counts.insert(component.to_string(), retry_count + 1);
        
        // Calculate delay
        let delay = self.calculate_delay(retry_count);
        
        info!(
            "Scheduling recovery for {} (attempt {}/{}): {}",
            component, retry_count + 1, self.config.max_retries, error
        );
        
        RecoveryAction::RetryAfterDelay(delay)
    }
    
    /// Reset retry count for a component after successful recovery
    pub fn reset_retry_count(&mut self, component: &str) {
        if self.retry_counts.remove(component).is_some() {
            info!("Component {} recovered successfully, reset retry count", component);
        }
        self.last_retry_times.remove(component);
    }
    
    /// Calculate delay for retry with exponential backoff
    fn calculate_delay(&self, retry_count: u32) -> Duration {
        if !self.config.exponential_backoff {
            return self.config.base_delay;
        }
        
        let delay_ms = self.config.base_delay.as_millis() as u64 * 2_u64.pow(retry_count);
        let delay = Duration::from_millis(delay_ms);
        
        if delay > self.config.max_delay {
            self.config.max_delay
        } else {
            delay
        }
    }
    
    /// Get current retry count for a component
    pub fn get_retry_count(&self, component: &str) -> u32 {
        self.retry_counts.get(component).copied().unwrap_or(0)
    }
    
    /// Check if component has exceeded retry limit
    pub fn has_exceeded_retry_limit(&self, component: &str) -> bool {
        self.get_retry_count(component) >= self.config.max_retries
    }
}

/// Camera recovery strategies
pub struct CameraRecovery {
    recovery_manager: RecoveryManager,
}

impl CameraRecovery {
    pub fn new() -> Self {
        Self {
            recovery_manager: RecoveryManager::new(),
        }
    }
    
    /// Handle camera error and determine recovery action
    pub fn handle_camera_error(&mut self, error: &CameraError) -> RecoveryAction {
        let doorcam_error = DoorcamError::Camera(error.clone());
        let action = self.recovery_manager.handle_error("camera", &doorcam_error);
        
        match error {
            CameraError::DeviceOpen { device } => {
                warn!("Camera device {} failed to open, will retry", device);
                action
            }
            CameraError::DeviceOpenWithSource { device, details } => {
                warn!("Camera device {} failed to open ({}), will retry", device, details);
                action
            }
            CameraError::Disconnected => {
                warn!("Camera disconnected, will attempt reconnection");
                action
            }
            CameraError::FrameTimeout { timeout } => {
                warn!("Camera frame timeout after {:?}, will retry", timeout);
                action
            }
            CameraError::NotAvailable => {
                error!("Camera not available, continuing without camera");
                RecoveryAction::Continue
            }
            _ => {
                warn!("Camera error: {}, will retry", error);
                action
            }
        }
    }
    
    /// Reset camera recovery state after successful reconnection
    pub fn reset(&mut self) {
        self.recovery_manager.reset_retry_count("camera");
    }
    
    /// Execute camera recovery with retry logic
    pub async fn recover_camera<F, Fut>(&mut self, mut reconnect_fn: F) -> Result<(), DoorcamError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<(), CameraError>>,
    {
        loop {
            match reconnect_fn().await {
                Ok(()) => {
                    self.reset();
                    info!("Camera recovery successful");
                    return Ok(());
                }
                Err(error) => {
                    let action = self.handle_camera_error(&error);
                    
                    match action {
                        RecoveryAction::RetryAfterDelay(delay) => {
                            debug!("Waiting {:?} before camera retry", delay);
                            sleep(delay).await;
                            continue;
                        }
                        RecoveryAction::Retry => {
                            continue;
                        }
                        RecoveryAction::Continue => {
                            warn!("Camera recovery failed, continuing without camera");
                            return Ok(());
                        }
                        RecoveryAction::Shutdown => {
                            error!("Camera recovery failed, shutting down");
                            return Err(DoorcamError::recovery_failed("camera", 
                                self.recovery_manager.get_retry_count("camera")));
                        }
                        RecoveryAction::None => {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}

/// Touch input recovery strategies
pub struct TouchRecovery {
    recovery_manager: RecoveryManager,
}

impl TouchRecovery {
    pub fn new() -> Self {
        Self {
            recovery_manager: RecoveryManager::new(),
        }
    }
    
    /// Handle touch error and determine recovery action
    pub fn handle_touch_error(&mut self, error: &TouchError) -> RecoveryAction {
        let doorcam_error = DoorcamError::Touch(error.clone());
        let action = self.recovery_manager.handle_error("touch", &doorcam_error);
        
        match error {
            TouchError::DeviceOpen { device, .. } => {
                warn!("Touch device {} failed to open, will retry", device);
                action
            }
            TouchError::DeviceRead { .. } => {
                warn!("Touch device read error, will retry");
                action
            }
            TouchError::NotAvailable => {
                warn!("Touch input not available, continuing without touch");
                RecoveryAction::Continue
            }
            _ => {
                warn!("Touch error: {}, will retry", error);
                action
            }
        }
    }
    
    /// Reset touch recovery state
    pub fn reset(&mut self) {
        self.recovery_manager.reset_retry_count("touch");
    }
    
    /// Execute touch recovery with retry logic
    pub async fn recover_touch<F, Fut>(&mut self, mut reconnect_fn: F) -> Result<(), DoorcamError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<(), TouchError>>,
    {
        loop {
            match reconnect_fn().await {
                Ok(()) => {
                    self.reset();
                    info!("Touch input recovery successful");
                    return Ok(());
                }
                Err(error) => {
                    let action = self.handle_touch_error(&error);
                    
                    match action {
                        RecoveryAction::RetryAfterDelay(delay) => {
                            debug!("Waiting {:?} before touch retry", delay);
                            sleep(delay).await;
                            continue;
                        }
                        RecoveryAction::Retry => {
                            continue;
                        }
                        RecoveryAction::Continue => {
                            warn!("Touch recovery failed, continuing without touch input");
                            return Ok(());
                        }
                        RecoveryAction::Shutdown => {
                            error!("Touch recovery failed, shutting down");
                            return Err(DoorcamError::recovery_failed("touch", 
                                self.recovery_manager.get_retry_count("touch")));
                        }
                        RecoveryAction::None => {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}

/// System health monitor
pub struct HealthMonitor {
    component_status: std::collections::HashMap<String, ComponentHealth>,
    last_health_check: Instant,
    health_check_interval: Duration,
}

/// Component health status
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ComponentHealth {
    Healthy,
    Degraded,
    Failed,
    Unknown,
}

impl HealthMonitor {
    /// Create a new health monitor
    pub fn new() -> Self {
        Self {
            component_status: std::collections::HashMap::new(),
            last_health_check: Instant::now(),
            health_check_interval: Duration::from_secs(30),
        }
    }
    
    /// Update component health status
    pub fn update_component_health(&mut self, component: &str, health: ComponentHealth) {
        let previous_health = self.component_status.get(component);
        
        if previous_health != Some(&health) {
            match health {
                ComponentHealth::Healthy => {
                    if previous_health.is_some() {
                        info!("Component {} recovered to healthy state", component);
                    }
                }
                ComponentHealth::Degraded => {
                    warn!("Component {} is in degraded state", component);
                }
                ComponentHealth::Failed => {
                    error!("Component {} has failed", component);
                }
                ComponentHealth::Unknown => {
                    warn!("Component {} health is unknown", component);
                }
            }
        }
        
        self.component_status.insert(component.to_string(), health);
    }
    
    /// Get component health status
    pub fn get_component_health(&self, component: &str) -> ComponentHealth {
        self.component_status.get(component)
            .cloned()
            .unwrap_or(ComponentHealth::Unknown)
    }
    
    /// Get overall system health
    pub fn get_system_health(&self) -> ComponentHealth {
        if self.component_status.is_empty() {
            return ComponentHealth::Unknown;
        }
        
        let mut has_failed = false;
        let mut has_degraded = false;
        
        for health in self.component_status.values() {
            match health {
                ComponentHealth::Failed => has_failed = true,
                ComponentHealth::Degraded => has_degraded = true,
                ComponentHealth::Unknown => has_degraded = true,
                ComponentHealth::Healthy => {}
            }
        }
        
        if has_failed {
            ComponentHealth::Failed
        } else if has_degraded {
            ComponentHealth::Degraded
        } else {
            ComponentHealth::Healthy
        }
    }
    
    /// Check if health check is due
    pub fn should_check_health(&self) -> bool {
        self.last_health_check.elapsed() >= self.health_check_interval
    }
    
    /// Mark health check as completed
    pub fn mark_health_check_completed(&mut self) {
        self.last_health_check = Instant::now();
    }
    
    /// Get all component statuses
    pub fn get_all_component_status(&self) -> &std::collections::HashMap<String, ComponentHealth> {
        &self.component_status
    }
}

/// Graceful degradation manager
pub struct GracefulDegradation {
    essential_components: std::collections::HashSet<String>,
    optional_components: std::collections::HashSet<String>,
}

impl GracefulDegradation {
    /// Create a new graceful degradation manager
    pub fn new() -> Self {
        let mut essential = std::collections::HashSet::new();
        essential.insert("ring_buffer".to_string());
        essential.insert("event_bus".to_string());
        
        let mut optional = std::collections::HashSet::new();
        optional.insert("camera".to_string());
        optional.insert("display".to_string());
        optional.insert("touch".to_string());
        optional.insert("stream".to_string());
        optional.insert("analyzer".to_string());
        optional.insert("capture".to_string());
        
        Self {
            essential_components: essential,
            optional_components: optional,
        }
    }
    
    /// Check if component failure should cause system shutdown
    pub fn should_shutdown_on_failure(&self, component: &str) -> bool {
        self.essential_components.contains(component)
    }
    
    /// Check if component is optional
    pub fn is_optional_component(&self, component: &str) -> bool {
        self.optional_components.contains(component)
    }
    
    /// Add essential component
    pub fn add_essential_component(&mut self, component: String) {
        self.essential_components.insert(component);
    }
    
    /// Add optional component
    pub fn add_optional_component(&mut self, component: String) {
        self.optional_components.insert(component);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_recovery_manager_retry_logic() {
        let mut manager = RecoveryManager::new();
        let error = DoorcamError::Camera(CameraError::Disconnected);
        
        // First error should trigger retry
        let action = manager.handle_error("camera", &error);
        assert!(matches!(action, RecoveryAction::RetryAfterDelay(_)));
        assert_eq!(manager.get_retry_count("camera"), 1);
        
        // After max retries, should continue
        for _ in 1..manager.config.max_retries {
            manager.handle_error("camera", &error);
        }
        
        let final_action = manager.handle_error("camera", &error);
        assert_eq!(final_action, RecoveryAction::Continue);
    }
    
    #[test]
    fn test_health_monitor() {
        let mut monitor = HealthMonitor::new();
        
        // Initial state should be unknown
        assert_eq!(monitor.get_component_health("camera"), ComponentHealth::Unknown);
        
        // Update health
        monitor.update_component_health("camera", ComponentHealth::Healthy);
        assert_eq!(monitor.get_component_health("camera"), ComponentHealth::Healthy);
        
        // System health with one healthy component
        assert_eq!(monitor.get_system_health(), ComponentHealth::Healthy);
        
        // Add failed component
        monitor.update_component_health("display", ComponentHealth::Failed);
        assert_eq!(monitor.get_system_health(), ComponentHealth::Failed);
    }
    
    #[test]
    fn test_graceful_degradation() {
        let degradation = GracefulDegradation::new();
        
        // Essential components should cause shutdown
        assert!(degradation.should_shutdown_on_failure("ring_buffer"));
        assert!(degradation.should_shutdown_on_failure("event_bus"));
        
        // Optional components should not cause shutdown
        assert!(!degradation.should_shutdown_on_failure("camera"));
        assert!(!degradation.should_shutdown_on_failure("display"));
        
        // Check optional component classification
        assert!(degradation.is_optional_component("camera"));
        assert!(!degradation.is_optional_component("ring_buffer"));
    }
}