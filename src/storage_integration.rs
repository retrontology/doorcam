use crate::{
    config::{CaptureConfig, SystemConfig},
    error::{DoorcamError, Result},
    events::EventBus,
    storage::{CleanupResult, EventStorage, StorageStats},
};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Integration wrapper for EventStorage with additional management capabilities
pub struct EventStorageIntegration {
    storage: Arc<EventStorage>,
    running: Arc<RwLock<bool>>,
    stats: Arc<RwLock<StorageIntegrationStats>>,
}

/// Statistics for the storage integration
#[derive(Debug, Clone, Default)]
pub struct StorageIntegrationStats {
    pub start_time: Option<SystemTime>,
    pub total_cleanups: u64,
    pub total_events_deleted: u64,
    pub total_bytes_freed: u64,
    pub last_cleanup_result: Option<CleanupResult>,
    pub cleanup_errors: u64,
}

/// Builder for EventStorageIntegration
pub struct EventStorageIntegrationBuilder {
    capture_config: Option<CaptureConfig>,
    system_config: Option<SystemConfig>,
    event_bus: Option<Arc<EventBus>>,
}

impl EventStorageIntegrationBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            capture_config: None,
            system_config: None,
            event_bus: None,
        }
    }

    /// Set the capture configuration
    pub fn with_capture_config(mut self, config: CaptureConfig) -> Self {
        self.capture_config = Some(config);
        self
    }

    /// Set the system configuration
    pub fn with_system_config(mut self, config: SystemConfig) -> Self {
        self.system_config = Some(config);
        self
    }

    /// Set the event bus
    pub fn with_event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Build the EventStorageIntegration
    pub fn build(self) -> Result<EventStorageIntegration> {
        let capture_config = self.capture_config.ok_or_else(|| {
            DoorcamError::component("storage_integration", "Capture config is required")
        })?;

        let system_config = self.system_config.ok_or_else(|| {
            DoorcamError::component("storage_integration", "System config is required")
        })?;

        let event_bus = self.event_bus.ok_or_else(|| {
            DoorcamError::component("storage_integration", "Event bus is required")
        })?;

        let storage = Arc::new(EventStorage::new(capture_config, system_config, event_bus));

        Ok(EventStorageIntegration {
            storage,
            running: Arc::new(RwLock::new(false)),
            stats: Arc::new(RwLock::new(StorageIntegrationStats::default())),
        })
    }
}

impl Default for EventStorageIntegrationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl EventStorageIntegration {
    /// Create a new builder
    pub fn builder() -> EventStorageIntegrationBuilder {
        EventStorageIntegrationBuilder::new()
    }

    /// Start the storage integration
    pub async fn start(&self) -> Result<()> {
        {
            let mut running = self.running.write().await;
            if *running {
                return Err(DoorcamError::component(
                    "storage_integration",
                    "Already running",
                ));
            }
            *running = true;
        }

        {
            let mut stats = self.stats.write().await;
            stats.start_time = Some(SystemTime::now());
        }

        info!("Starting event storage integration");

        // Start the underlying storage system
        self.storage.start().await?;

        // Start monitoring task
        self.start_monitoring_task().await;

        info!("Event storage integration started successfully");
        Ok(())
    }

    /// Start background monitoring task
    async fn start_monitoring_task(&self) {
        let storage = Arc::clone(&self.storage);
        let stats = Arc::clone(&self.stats);
        let running = Arc::clone(&self.running);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // Every 5 minutes

            loop {
                interval.tick().await;

                // Check if still running
                {
                    let running_guard = running.read().await;
                    if !*running_guard {
                        break;
                    }
                }

                // Update statistics
                if let Err(e) = Self::update_monitoring_stats(&storage, &stats).await {
                    error!("Failed to update monitoring stats: {}", e);
                }
            }
        });
    }

    /// Update monitoring statistics
    async fn update_monitoring_stats(
        storage: &Arc<EventStorage>,
        _stats: &Arc<RwLock<StorageIntegrationStats>>,
    ) -> Result<()> {
        let storage_stats = storage.get_storage_stats().await;

        debug!(
            "Storage monitoring: {} events, {} bytes, last cleanup: {:?}",
            storage_stats.total_events, storage_stats.total_size_bytes, storage_stats.last_cleanup
        );

        // Log warnings for large storage usage
        const WARNING_THRESHOLD_GB: u64 = 10 * 1024 * 1024 * 1024; // 10 GB
        if storage_stats.total_size_bytes > WARNING_THRESHOLD_GB {
            warn!(
                "Storage usage is high: {:.2} GB ({} events)",
                storage_stats.total_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                storage_stats.total_events
            );
        }

        // Log warnings for old events
        if let Some(oldest) = storage_stats.oldest_event {
            if let Ok(age) = oldest.elapsed() {
                const WARNING_AGE_DAYS: u64 = 30;
                if age > Duration::from_secs(WARNING_AGE_DAYS * 24 * 3600) {
                    warn!(
                        "Oldest event is {} days old, consider checking cleanup configuration",
                        age.as_secs() / (24 * 3600)
                    );
                }
            }
        }

        Ok(())
    }

    /// Run manual cleanup
    pub async fn run_manual_cleanup(&self) -> Result<CleanupResult> {
        info!("Running manual cleanup");

        let result = self.storage.run_cleanup().await?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_cleanups += 1;
            stats.total_events_deleted += result.events_deleted as u64;
            stats.total_bytes_freed += result.bytes_freed;
            stats.last_cleanup_result = Some(result.clone());

            if !result.errors.is_empty() {
                stats.cleanup_errors += 1;
            }
        }

        info!(
            "Manual cleanup completed: {} events deleted, {} bytes freed",
            result.events_deleted, result.bytes_freed
        );

        Ok(result)
    }

    /// Run cleanup with custom retention period
    pub async fn run_cleanup_with_retention(&self, retention_days: u32) -> Result<CleanupResult> {
        info!(
            "Running cleanup with custom retention: {} days",
            retention_days
        );

        let result = self
            .storage
            .run_cleanup_with_retention(retention_days)
            .await?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_cleanups += 1;
            stats.total_events_deleted += result.events_deleted as u64;
            stats.total_bytes_freed += result.bytes_freed;
            stats.last_cleanup_result = Some(result.clone());

            if !result.errors.is_empty() {
                stats.cleanup_errors += 1;
            }
        }

        info!(
            "Custom cleanup completed: {} events deleted, {} bytes freed",
            result.events_deleted, result.bytes_freed
        );

        Ok(result)
    }

    /// Perform dry run cleanup to preview what would be deleted
    pub async fn dry_run_cleanup(&self) -> Result<CleanupResult> {
        info!("Running dry run cleanup");
        self.storage.dry_run_cleanup().await
    }

    /// Get cleanup status
    pub async fn get_cleanup_status(&self) -> crate::storage::CleanupStatus {
        self.storage.get_cleanup_status().await
    }

    /// Get storage statistics
    pub async fn get_storage_stats(&self) -> StorageStats {
        self.storage.get_storage_stats().await
    }

    /// Get integration statistics
    pub async fn get_integration_stats(&self) -> StorageIntegrationStats {
        let stats = self.stats.read().await;
        stats.clone()
    }

    /// Get combined statistics
    pub async fn get_combined_stats(&self) -> CombinedStorageStats {
        let storage_stats = self.get_storage_stats().await;
        let integration_stats = self.get_integration_stats().await;

        CombinedStorageStats {
            storage: storage_stats,
            integration: integration_stats,
        }
    }

    /// Delete a specific event by ID
    pub async fn delete_event(&self, event_id: &str) -> Result<u64> {
        info!("Manually deleting event: {}", event_id);

        let bytes_freed = self.storage.delete_event_by_id(event_id).await?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_events_deleted += 1;
            stats.total_bytes_freed += bytes_freed;
        }

        info!("Event {} deleted ({} bytes freed)", event_id, bytes_freed);
        Ok(bytes_freed)
    }

    /// Get events in a time range
    pub async fn get_events_in_range(
        &self,
        start: SystemTime,
        end: SystemTime,
    ) -> Vec<crate::storage::StoredEventMetadata> {
        self.storage.get_events_in_range(start, end).await
    }

    /// Get recent events
    pub async fn get_recent_events(
        &self,
        count: usize,
    ) -> Vec<crate::storage::StoredEventMetadata> {
        self.storage.get_recent_events(count).await
    }

    /// Get event by ID
    pub async fn get_event(&self, event_id: &str) -> Option<crate::storage::StoredEventMetadata> {
        self.storage.get_event(event_id).await
    }

    /// Update event access time
    pub async fn update_event_access(&self, event_id: &str) -> Result<()> {
        self.storage.update_event_access(event_id).await
    }

    /// Check if the integration is running
    pub async fn is_running(&self) -> bool {
        let running = self.running.read().await;
        *running
    }

    /// Get uptime of the integration
    pub async fn get_uptime(&self) -> Option<Duration> {
        let stats = self.stats.read().await;
        if let Some(start_time) = stats.start_time {
            start_time.elapsed().ok()
        } else {
            None
        }
    }

    /// Stop the storage integration
    pub async fn stop(&self) -> Result<()> {
        {
            let mut running = self.running.write().await;
            if !*running {
                return Ok(());
            }
            *running = false;
        }

        info!("Stopping event storage integration");

        // Stop the underlying storage system
        self.storage.stop().await?;

        info!("Event storage integration stopped");
        Ok(())
    }

    /// Get health status
    pub async fn get_health_status(&self) -> StorageHealthStatus {
        let is_running = self.is_running().await;
        let storage_stats = self.get_storage_stats().await;
        let integration_stats = self.get_integration_stats().await;

        let mut issues = Vec::new();

        // Check for issues
        if !is_running {
            issues.push("Storage integration is not running".to_string());
        }

        // Check storage size
        const MAX_STORAGE_GB: u64 = 50 * 1024 * 1024 * 1024; // 50 GB
        if storage_stats.total_size_bytes > MAX_STORAGE_GB {
            issues.push(format!(
                "Storage usage is very high: {:.2} GB",
                storage_stats.total_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
            ));
        }

        // Check cleanup errors
        if integration_stats.cleanup_errors > 0 {
            issues.push(format!(
                "Cleanup errors detected: {}",
                integration_stats.cleanup_errors
            ));
        }

        // Check last cleanup
        if let Some(last_cleanup) = storage_stats.last_cleanup {
            if let Ok(elapsed) = last_cleanup.elapsed() {
                if elapsed > Duration::from_secs(25 * 3600) {
                    // 25 hours
                    issues.push("Last cleanup was more than 25 hours ago".to_string());
                }
            }
        } else {
            issues.push("No cleanup has been performed yet".to_string());
        }

        // Check cleanup status for additional insights
        let cleanup_status = self.get_cleanup_status().await;
        if cleanup_status.events_eligible_for_cleanup > 100 {
            issues.push(format!(
                "Large number of events eligible for cleanup: {}",
                cleanup_status.events_eligible_for_cleanup
            ));
        }

        if cleanup_status.bytes_eligible_for_cleanup > 5 * 1024 * 1024 * 1024 {
            // 5 GB
            issues.push(format!(
                "Large amount of data eligible for cleanup: {:.2} GB",
                cleanup_status.bytes_eligible_for_cleanup as f64 / (1024.0 * 1024.0 * 1024.0)
            ));
        }

        let status = if issues.is_empty() {
            HealthStatus::Healthy
        } else if issues.len() <= 2 {
            HealthStatus::Warning
        } else {
            HealthStatus::Critical
        };

        StorageHealthStatus {
            status,
            issues,
            uptime: self.get_uptime().await,
            storage_stats,
            integration_stats,
        }
    }
}

/// Combined storage statistics
#[derive(Debug, Clone)]
pub struct CombinedStorageStats {
    pub storage: StorageStats,
    pub integration: StorageIntegrationStats,
}

/// Health status for storage system
#[derive(Debug, Clone)]
pub struct StorageHealthStatus {
    pub status: HealthStatus,
    pub issues: Vec<String>,
    pub uptime: Option<Duration>,
    pub storage_stats: StorageStats,
    pub integration_stats: StorageIntegrationStats,
}

/// Health status levels
#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Warning,
    Critical,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{CaptureConfig, SystemConfig},
        events::EventBus,
    };
    use tempfile::TempDir;

    fn create_test_configs() -> (CaptureConfig, SystemConfig) {
        let temp_dir = TempDir::new().unwrap();
        let capture_config = CaptureConfig {
            path: temp_dir.path().to_string_lossy().to_string(),
            timestamp_overlay: true,
            timestamp_font_path: "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".to_string(),
            timestamp_font_size: 24.0,
            timestamp_timezone: "UTC".to_string(),
            video_encoding: false,
            keep_images: true,
            save_metadata: true,
            rotation: None,
        };

        let system_config = SystemConfig {
            trim_old: true,
            retention_days: 7,
        };

        (capture_config, system_config)
    }

    #[tokio::test]
    async fn test_storage_integration_builder() {
        let (capture_config, system_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new());

        let integration = EventStorageIntegration::builder()
            .with_capture_config(capture_config)
            .with_system_config(system_config)
            .with_event_bus(event_bus)
            .build()
            .unwrap();

        assert!(!integration.is_running().await);
    }

    #[tokio::test]
    async fn test_storage_integration_lifecycle() {
        let (capture_config, system_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new());

        let integration = EventStorageIntegration::builder()
            .with_capture_config(capture_config)
            .with_system_config(system_config)
            .with_event_bus(event_bus)
            .build()
            .unwrap();

        // Should not be running initially
        assert!(!integration.is_running().await);

        // Start should succeed
        integration.start().await.unwrap();
        assert!(integration.is_running().await);

        // Should have uptime
        assert!(integration.get_uptime().await.is_some());

        // Stop should succeed
        integration.stop().await.unwrap();
        assert!(!integration.is_running().await);
    }

    #[tokio::test]
    async fn test_storage_integration_stats() {
        let (capture_config, system_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new());

        let integration = EventStorageIntegration::builder()
            .with_capture_config(capture_config)
            .with_system_config(system_config)
            .with_event_bus(event_bus)
            .build()
            .unwrap();

        let stats = integration.get_integration_stats().await;
        assert_eq!(stats.total_cleanups, 0);
        assert_eq!(stats.total_events_deleted, 0);
        assert_eq!(stats.total_bytes_freed, 0);
    }

    #[tokio::test]
    async fn test_health_status() {
        let (capture_config, system_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new());

        let integration = EventStorageIntegration::builder()
            .with_capture_config(capture_config)
            .with_system_config(system_config)
            .with_event_bus(event_bus)
            .build()
            .unwrap();

        let health = integration.get_health_status().await;

        // Should have some issues since it's not running
        assert!(!health.issues.is_empty());
        assert_ne!(health.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_cleanup_operations() {
        let (capture_config, system_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new());

        let integration = EventStorageIntegration::builder()
            .with_capture_config(capture_config)
            .with_system_config(system_config)
            .with_event_bus(event_bus)
            .build()
            .unwrap();

        // Test dry run cleanup
        let dry_run_result = integration.dry_run_cleanup().await.unwrap();
        assert_eq!(dry_run_result.events_deleted, 0);
        assert_eq!(dry_run_result.bytes_freed, 0);

        // Test cleanup status
        let cleanup_status = integration.get_cleanup_status().await;
        assert!(!cleanup_status.is_running);
        assert_eq!(cleanup_status.retention_days, 7);
        assert_eq!(cleanup_status.events_eligible_for_cleanup, 0);

        // Test custom retention cleanup
        let custom_result = integration.run_cleanup_with_retention(1).await.unwrap();
        assert_eq!(custom_result.events_deleted, 0);
        assert_eq!(custom_result.bytes_freed, 0);
    }
}
