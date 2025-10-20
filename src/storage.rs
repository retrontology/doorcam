use crate::{
    config::{CaptureConfig, SystemConfig},
    error::{DoorcamError, Result},
    events::{DoorcamEvent, EventBus},
};
use chrono::{DateTime, Utc, Duration as ChronoDuration, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};


/// Event storage system for managing captured video events and automatic cleanup
pub struct EventStorage {
    capture_config: CaptureConfig,
    system_config: SystemConfig,
    event_bus: Arc<EventBus>,
    event_registry: Arc<RwLock<EventRegistry>>,
    cleanup_running: Arc<RwLock<bool>>,
}

/// Registry of stored events with metadata
#[derive(Debug, Clone, Default)]
struct EventRegistry {
    events: HashMap<String, StoredEventMetadata>,
    last_cleanup: Option<SystemTime>,
}

/// Metadata for a stored event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEventMetadata {
    pub event_id: String,
    pub timestamp: SystemTime,
    pub directory_path: PathBuf,
    pub file_count: u32,
    pub total_size_bytes: u64,
    pub event_type: StoredEventType,
    pub created_at: SystemTime,
    pub last_accessed: SystemTime,
}

/// Types of stored events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StoredEventType {
    MotionCapture {
        motion_area: f64,
        preroll_frames: usize,
        postroll_frames: usize,
    },
    ManualCapture {
        trigger_reason: String,
    },
}

/// Statistics about the event storage system
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub total_events: usize,
    pub total_size_bytes: u64,
    pub oldest_event: Option<SystemTime>,
    pub newest_event: Option<SystemTime>,
    pub events_by_type: HashMap<String, usize>,
    pub last_cleanup: Option<SystemTime>,
}

/// Cleanup operation result
#[derive(Debug, Clone)]
pub struct CleanupResult {
    pub events_deleted: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
    pub duration: Duration,
}

/// Cleanup status information
#[derive(Debug, Clone)]
pub struct CleanupStatus {
    pub is_running: bool,
    pub retention_days: u32,
    pub last_cleanup: Option<SystemTime>,
    pub events_eligible_for_cleanup: usize,
    pub bytes_eligible_for_cleanup: u64,
    pub total_events: usize,
}

impl EventStorage {
    /// Create a new event storage system
    pub fn new(
        capture_config: CaptureConfig,
        system_config: SystemConfig,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            capture_config,
            system_config,
            event_bus,
            event_registry: Arc::new(RwLock::new(EventRegistry::default())),
            cleanup_running: Arc::new(RwLock::new(false)),
        }
    }

    /// Start the event storage system
    pub async fn start(&self) -> Result<()> {
        info!("Starting event storage system");

        // Create base capture directory if it doesn't exist
        let capture_path = PathBuf::from(&self.capture_config.path);
        if !capture_path.exists() {
            fs::create_dir_all(&capture_path).await
                .map_err(|e| DoorcamError::component("event_storage", &format!("Failed to create capture directory: {}", e)))?;
            info!("Created capture directory: {}", capture_path.display());
        }

        // Load existing events from filesystem
        self.scan_and_register_existing_events().await?;

        // Subscribe to capture completion events
        let mut event_receiver = self.event_bus.subscribe();
        let storage_system = Arc::new(self.clone());

        tokio::spawn(async move {
            while let Ok(event) = event_receiver.recv().await {
                match event {
                    DoorcamEvent::CaptureCompleted { event_id, file_count } => {
                        if let Err(e) = storage_system.register_completed_capture(&event_id, file_count).await {
                            error!("Failed to register completed capture {}: {}", event_id, e);
                        }
                    }
                    _ => {} // Ignore other events
                }
            }
        });

        // Start cleanup scheduler if enabled
        if self.system_config.trim_old {
            self.start_cleanup_scheduler().await?;
        }

        info!("Event storage system started successfully");
        Ok(())
    }

    /// Scan filesystem and register existing events
    async fn scan_and_register_existing_events(&self) -> Result<()> {
        let capture_path = PathBuf::from(&self.capture_config.path);
        
        debug!("Scanning for existing events in: {}", capture_path.display());

        let mut entries = fs::read_dir(&capture_path).await
            .map_err(|e| DoorcamError::component("event_storage", &format!("Failed to read capture directory: {}", e)))?;

        let mut registered_count = 0;

        while let Some(entry) = entries.next_entry().await
            .map_err(|e| DoorcamError::component("event_storage", &format!("Failed to read directory entry: {}", e)))? {
            
            let path = entry.path();
            
            if path.is_dir() {
                if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                    // Check if directory name matches timestamp pattern (YYYYMMDD_HHMMSS_mmm)
                    if self.is_valid_event_directory_name(dir_name) {
                        match self.register_existing_event(&path).await {
                            Ok(_) => {
                                registered_count += 1;
                                debug!("Registered existing event: {}", dir_name);
                            }
                            Err(e) => {
                                warn!("Failed to register existing event {}: {}", dir_name, e);
                            }
                        }
                    }
                }
            }
        }

        info!("Registered {} existing events", registered_count);
        Ok(())
    }

    /// Check if directory name matches expected timestamp pattern
    fn is_valid_event_directory_name(&self, name: &str) -> bool {
        // Pattern: YYYYMMDD_HHMMSS_mmm (e.g., 20231019_143022_123)
        name.len() == 19 && 
        name.chars().enumerate().all(|(i, c)| {
            match i {
                8 | 15 => c == '_',
                _ => c.is_ascii_digit(),
            }
        })
    }

    /// Register an existing event directory
    async fn register_existing_event(&self, event_dir: &Path) -> Result<()> {
        let dir_name = event_dir.file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DoorcamError::component("event_storage", "Invalid directory name"))?;

        // Parse timestamp from directory name
        let timestamp = self.parse_timestamp_from_directory_name(dir_name)?;

        // Calculate directory size and file count
        let (file_count, total_size) = self.calculate_directory_stats(event_dir).await?;

        // Try to load metadata if it exists
        let metadata_path = event_dir.join("metadata.json");
        let event_type = if metadata_path.exists() {
            match self.load_event_type_from_metadata(&metadata_path).await {
                Ok(event_type) => event_type,
                Err(e) => {
                    warn!("Failed to load metadata for {}: {}, using default", dir_name, e);
                    StoredEventType::MotionCapture {
                        motion_area: 0.0,
                        preroll_frames: 0,
                        postroll_frames: 0,
                    }
                }
            }
        } else {
            StoredEventType::MotionCapture {
                motion_area: 0.0,
                preroll_frames: 0,
                postroll_frames: 0,
            }
        };

        // Create event metadata
        let event_metadata = StoredEventMetadata {
            event_id: dir_name.to_string(),
            timestamp,
            directory_path: event_dir.to_path_buf(),
            file_count,
            total_size_bytes: total_size,
            event_type,
            created_at: timestamp,
            last_accessed: SystemTime::now(),
        };

        // Register in memory
        {
            let mut registry = self.event_registry.write().await;
            registry.events.insert(dir_name.to_string(), event_metadata);
        }

        Ok(())
    }

    /// Parse timestamp from directory name (YYYYMMDD_HHMMSS_mmm)
    fn parse_timestamp_from_directory_name(&self, name: &str) -> Result<SystemTime> {
        if name.len() != 19 {
            return Err(DoorcamError::component("event_storage", "Invalid timestamp format"));
        }

        let year: i32 = name[0..4].parse()
            .map_err(|_| DoorcamError::component("event_storage", "Invalid year in timestamp"))?;
        let month: u32 = name[4..6].parse()
            .map_err(|_| DoorcamError::component("event_storage", "Invalid month in timestamp"))?;
        let day: u32 = name[6..8].parse()
            .map_err(|_| DoorcamError::component("event_storage", "Invalid day in timestamp"))?;
        let hour: u32 = name[9..11].parse()
            .map_err(|_| DoorcamError::component("event_storage", "Invalid hour in timestamp"))?;
        let minute: u32 = name[11..13].parse()
            .map_err(|_| DoorcamError::component("event_storage", "Invalid minute in timestamp"))?;
        let second: u32 = name[13..15].parse()
            .map_err(|_| DoorcamError::component("event_storage", "Invalid second in timestamp"))?;
        let millisecond: u32 = name[16..19].parse()
            .map_err(|_| DoorcamError::component("event_storage", "Invalid millisecond in timestamp"))?;

        // Create DateTime and convert to SystemTime
        let datetime = chrono::Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .ok_or_else(|| DoorcamError::component("event_storage", "Invalid datetime"))?
            + ChronoDuration::milliseconds(millisecond as i64);

        let timestamp = UNIX_EPOCH + Duration::from_secs(datetime.timestamp() as u64) 
            + Duration::from_nanos(datetime.timestamp_subsec_nanos() as u64);

        Ok(timestamp)
    }

    /// Calculate directory statistics (file count and total size)
    async fn calculate_directory_stats(&self, dir_path: &Path) -> Result<(u32, u64)> {
        let mut file_count = 0u32;
        let mut total_size = 0u64;

        let mut entries = fs::read_dir(dir_path).await
            .map_err(|e| DoorcamError::component("event_storage", &format!("Failed to read directory: {}", e)))?;

        while let Some(entry) = entries.next_entry().await
            .map_err(|e| DoorcamError::component("event_storage", &format!("Failed to read directory entry: {}", e)))? {
            
            let metadata = entry.metadata().await
                .map_err(|e| DoorcamError::component("event_storage", &format!("Failed to read file metadata: {}", e)))?;

            if metadata.is_file() {
                file_count += 1;
                total_size += metadata.len();
            }
        }

        Ok((file_count, total_size))
    }

    /// Load event type from metadata file
    async fn load_event_type_from_metadata(&self, metadata_path: &Path) -> Result<StoredEventType> {
        let metadata_content = fs::read_to_string(metadata_path).await
            .map_err(|e| DoorcamError::component("event_storage", &format!("Failed to read metadata file: {}", e)))?;

        let metadata: serde_json::Value = serde_json::from_str(&metadata_content)
            .map_err(|e| DoorcamError::component("event_storage", &format!("Failed to parse metadata JSON: {}", e)))?;

        // Extract motion area if available
        let motion_area = metadata.get("motion_area")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let preroll_frames = metadata.get("preroll_frame_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let postroll_frames = metadata.get("postroll_frame_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        Ok(StoredEventType::MotionCapture {
            motion_area,
            preroll_frames,
            postroll_frames,
        })
    }

    /// Register a completed capture event
    async fn register_completed_capture(&self, event_id: &str, file_count: u32) -> Result<()> {
        debug!("Registering completed capture: {}", event_id);

        // Find the event directory
        let capture_path = PathBuf::from(&self.capture_config.path);
        let mut event_dir = None;

        let mut entries = fs::read_dir(&capture_path).await
            .map_err(|e| DoorcamError::component("event_storage", &format!("Failed to read capture directory: {}", e)))?;

        while let Some(entry) = entries.next_entry().await
            .map_err(|e| DoorcamError::component("event_storage", &format!("Failed to read directory entry: {}", e)))? {
            
            let path = entry.path();
            if path.is_dir() {
                // Check if this directory contains the event_id in its metadata
                let metadata_path = path.join("metadata.json");
                if metadata_path.exists() {
                    if let Ok(content) = fs::read_to_string(&metadata_path).await {
                        if content.contains(event_id) {
                            event_dir = Some(path);
                            break;
                        }
                    }
                }
            }
        }

        let event_dir = event_dir.ok_or_else(|| {
            DoorcamError::component("event_storage", &format!("Event directory not found for {}", event_id))
        })?;

        // Register the event
        self.register_existing_event(&event_dir).await?;

        info!("Registered completed capture: {} with {} files", event_id, file_count);
        Ok(())
    }
}

impl Clone for EventStorage {
    fn clone(&self) -> Self {
        Self {
            capture_config: self.capture_config.clone(),
            system_config: self.system_config.clone(),
            event_bus: Arc::clone(&self.event_bus),
            event_registry: Arc::clone(&self.event_registry),
            cleanup_running: Arc::clone(&self.cleanup_running),
        }
    }
}

impl EventStorage {
    /// Start the cleanup scheduler
    async fn start_cleanup_scheduler(&self) -> Result<()> {
        info!("Starting cleanup scheduler (retention: {} days)", self.system_config.retention_days);

        let storage_system = Arc::new(self.clone());
        
        // Main cleanup scheduler task
        tokio::spawn(async move {
            // Run cleanup every hour, with exponential backoff on failures
            let base_interval = Duration::from_secs(3600); // 1 hour
            let mut current_interval = base_interval;
            let max_interval = Duration::from_secs(24 * 3600); // 24 hours max
            let mut consecutive_failures = 0u32;
            
            loop {
                let mut interval = tokio::time::interval(current_interval);
                interval.tick().await; // Skip first immediate tick
                interval.tick().await; // Wait for the actual interval
                
                match storage_system.run_cleanup().await {
                    Ok(result) => {
                        // Reset interval on success
                        current_interval = base_interval;
                        consecutive_failures = 0;
                        
                        info!(
                            "Scheduled cleanup completed successfully: {} events deleted, {} bytes freed",
                            result.events_deleted, result.bytes_freed
                        );
                        
                        // Log warnings if cleanup had errors but didn't fail completely
                        if !result.errors.is_empty() {
                            warn!("Cleanup completed with {} errors: {:?}", result.errors.len(), result.errors);
                        }
                    }
                    Err(e) => {
                        consecutive_failures += 1;
                        error!("Cleanup operation failed (attempt {}): {}", consecutive_failures, e);
                        
                        // Exponential backoff: double the interval up to max_interval
                        if consecutive_failures <= 5 {
                            current_interval = std::cmp::min(
                                current_interval * 2,
                                max_interval
                            );
                            warn!("Increasing cleanup interval to {:?} due to failures", current_interval);
                        }
                        
                        // If we've had too many consecutive failures, log critical error
                        if consecutive_failures >= 10 {
                            error!("Cleanup has failed {} consecutive times - manual intervention may be required", consecutive_failures);
                        }
                    }
                }
            }
        });

        // Initial cleanup task with delay
        tokio::spawn({
            let storage_system = Arc::new(self.clone());
            async move {
                // Wait for system to stabilize before first cleanup
                tokio::time::sleep(Duration::from_secs(60)).await;
                
                info!("Running initial cleanup");
                match storage_system.run_cleanup().await {
                    Ok(result) => {
                        info!(
                            "Initial cleanup completed: {} events deleted, {} bytes freed",
                            result.events_deleted, result.bytes_freed
                        );
                    }
                    Err(e) => {
                        error!("Initial cleanup failed: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Run cleanup operation to remove old events
    pub async fn run_cleanup(&self) -> Result<CleanupResult> {
        // Check if cleanup is already running
        {
            let cleanup_running = self.cleanup_running.read().await;
            if *cleanup_running {
                debug!("Cleanup already running, skipping");
                return Ok(CleanupResult {
                    events_deleted: 0,
                    bytes_freed: 0,
                    errors: vec!["Cleanup already running".to_string()],
                    duration: Duration::ZERO,
                });
            }
        }

        // Set cleanup running flag
        {
            let mut cleanup_running = self.cleanup_running.write().await;
            *cleanup_running = true;
        }

        let start_time = std::time::Instant::now();
        info!("Starting cleanup operation");

        let result = self.perform_cleanup().await;

        // Clear cleanup running flag
        {
            let mut cleanup_running = self.cleanup_running.write().await;
            *cleanup_running = false;
        }

        // Update last cleanup time
        {
            let mut registry = self.event_registry.write().await;
            registry.last_cleanup = Some(SystemTime::now());
        }

        let duration = start_time.elapsed();
        
        match &result {
            Ok(cleanup_result) => {
                info!(
                    "Cleanup completed: {} events deleted, {} bytes freed, {} errors, took {:?}",
                    cleanup_result.events_deleted,
                    cleanup_result.bytes_freed,
                    cleanup_result.errors.len(),
                    duration
                );
            }
            Err(e) => {
                error!("Cleanup failed: {}", e);
            }
        }

        result
    }

    /// Perform the actual cleanup operation
    async fn perform_cleanup(&self) -> Result<CleanupResult> {
        let retention_duration = Duration::from_secs(self.system_config.retention_days as u64 * 24 * 3600);
        let cutoff_time = SystemTime::now() - retention_duration;

        debug!("Cleanup cutoff time: {:?}", cutoff_time);

        let mut events_to_delete = Vec::new();

        // Find events older than retention period
        {
            let registry = self.event_registry.read().await;
            for (event_id, metadata) in &registry.events {
                if metadata.timestamp < cutoff_time {
                    events_to_delete.push((event_id.clone(), metadata.clone()));
                }
            }
        }

        info!("Found {} events to delete", events_to_delete.len());

        let mut events_deleted = 0;
        let mut bytes_freed = 0u64;
        let mut errors = Vec::new();

        // Delete old events
        for (event_id, metadata) in events_to_delete {
            match self.delete_event(&event_id, &metadata).await {
                Ok(deleted_bytes) => {
                    events_deleted += 1;
                    bytes_freed += deleted_bytes;
                    debug!("Deleted event: {} ({} bytes)", event_id, deleted_bytes);
                }
                Err(e) => {
                    let error_msg = format!("Failed to delete event {}: {}", event_id, e);
                    error!("{}", error_msg);
                    errors.push(error_msg);
                }
            }
        }

        Ok(CleanupResult {
            events_deleted,
            bytes_freed,
            errors,
            duration: Duration::ZERO, // Will be set by caller
        })
    }

    /// Delete a specific event and its files
    async fn delete_event(&self, event_id: &str, metadata: &StoredEventMetadata) -> Result<u64> {
        debug!("Deleting event: {} at {}", event_id, metadata.directory_path.display());

        // Enhanced timestamp validation with multiple safety checks
        let retention_duration = Duration::from_secs(self.system_config.retention_days as u64 * 24 * 3600);
        let cutoff_time = SystemTime::now() - retention_duration;

        if metadata.timestamp >= cutoff_time {
            return Err(DoorcamError::component(
                "event_storage", 
                &format!("Refusing to delete recent event {} (timestamp validation failed)", event_id)
            ));
        }

        // Additional safety check: ensure event is at least 1 hour old regardless of retention policy
        let minimum_age = Duration::from_secs(3600); // 1 hour
        let minimum_cutoff = SystemTime::now() - minimum_age;
        if metadata.timestamp >= minimum_cutoff {
            return Err(DoorcamError::component(
                "event_storage",
                &format!("Refusing to delete very recent event {} (minimum age safety check)", event_id)
            ));
        }

        // Validate deletion safety before proceeding
        self.validate_deletion_safety(&metadata.directory_path)?;

        // Double-check directory exists and is within capture path
        if !metadata.directory_path.exists() {
            warn!("Event directory already deleted: {}", metadata.directory_path.display());
            
            // Remove from registry
            {
                let mut registry = self.event_registry.write().await;
                registry.events.remove(event_id);
            }
            
            return Ok(0);
        }

        let capture_path = PathBuf::from(&self.capture_config.path);
        if !metadata.directory_path.starts_with(&capture_path) {
            return Err(DoorcamError::component(
                "event_storage",
                &format!("Event directory {} is outside capture path", metadata.directory_path.display())
            ));
        }

        // Calculate actual size before deletion
        let (_, actual_size) = self.calculate_directory_stats(&metadata.directory_path).await?;

        // Create backup of metadata before deletion (for recovery purposes)
        let backup_metadata = serde_json::to_string_pretty(&metadata)
            .map_err(|e| DoorcamError::component("event_storage", &format!("Failed to serialize metadata: {}", e)))?;
        
        let backup_path = metadata.directory_path.with_extension("deleted.json");
        if let Err(e) = fs::write(&backup_path, backup_metadata).await {
            warn!("Failed to create deletion backup for {}: {}", event_id, e);
        }

        // Remove directory and all contents
        fs::remove_dir_all(&metadata.directory_path).await
            .map_err(|e| DoorcamError::component(
                "event_storage", 
                &format!("Failed to delete directory {}: {}", metadata.directory_path.display(), e)
            ))?;

        // Remove from registry
        {
            let mut registry = self.event_registry.write().await;
            registry.events.remove(event_id);
        }

        info!("Deleted event: {} ({} bytes)", event_id, actual_size);
        Ok(actual_size)
    }

    /// Get storage statistics
    pub async fn get_storage_stats(&self) -> StorageStats {
        let registry = self.event_registry.read().await;
        
        let mut total_size_bytes = 0u64;
        let mut oldest_event = None;
        let mut newest_event = None;
        let mut events_by_type = HashMap::new();

        for metadata in registry.events.values() {
            total_size_bytes += metadata.total_size_bytes;
            
            // Track oldest and newest events
            if oldest_event.is_none() || metadata.timestamp < oldest_event.unwrap() {
                oldest_event = Some(metadata.timestamp);
            }
            if newest_event.is_none() || metadata.timestamp > newest_event.unwrap() {
                newest_event = Some(metadata.timestamp);
            }

            // Count events by type
            let type_name = match &metadata.event_type {
                StoredEventType::MotionCapture { .. } => "motion_capture",
                StoredEventType::ManualCapture { .. } => "manual_capture",
            };
            *events_by_type.entry(type_name.to_string()).or_insert(0) += 1;
        }

        StorageStats {
            total_events: registry.events.len(),
            total_size_bytes,
            oldest_event,
            newest_event,
            events_by_type,
            last_cleanup: registry.last_cleanup,
        }
    }

    /// Get events within a time range
    pub async fn get_events_in_range(&self, start: SystemTime, end: SystemTime) -> Vec<StoredEventMetadata> {
        let registry = self.event_registry.read().await;
        
        registry.events.values()
            .filter(|metadata| metadata.timestamp >= start && metadata.timestamp <= end)
            .cloned()
            .collect()
    }

    /// Get recent events (last N events)
    pub async fn get_recent_events(&self, count: usize) -> Vec<StoredEventMetadata> {
        let registry = self.event_registry.read().await;
        
        let mut events: Vec<_> = registry.events.values().cloned().collect();
        events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp)); // Sort by timestamp descending
        events.truncate(count);
        events
    }

    /// Get event by ID
    pub async fn get_event(&self, event_id: &str) -> Option<StoredEventMetadata> {
        let registry = self.event_registry.read().await;
        registry.events.get(event_id).cloned()
    }

    /// Update last accessed time for an event
    pub async fn update_event_access(&self, event_id: &str) -> Result<()> {
        let mut registry = self.event_registry.write().await;
        
        if let Some(metadata) = registry.events.get_mut(event_id) {
            metadata.last_accessed = SystemTime::now();
            debug!("Updated access time for event: {}", event_id);
        }
        
        Ok(())
    }

    /// Force cleanup of specific event (for testing or manual cleanup)
    pub async fn delete_event_by_id(&self, event_id: &str) -> Result<u64> {
        let metadata = {
            let registry = self.event_registry.read().await;
            registry.events.get(event_id).cloned()
        };

        if let Some(metadata) = metadata {
            self.delete_event(event_id, &metadata).await
        } else {
            Err(DoorcamError::component("event_storage", &format!("Event not found: {}", event_id)))
        }
    }

    /// Stop the event storage system
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping event storage system");
        
        // Wait for any running cleanup to complete with timeout
        let timeout_duration = Duration::from_secs(30);
        let start_time = std::time::Instant::now();
        
        loop {
            let cleanup_running = {
                let guard = self.cleanup_running.read().await;
                *guard
            };
            
            if !cleanup_running {
                break;
            }
            
            if start_time.elapsed() > timeout_duration {
                warn!("Timeout waiting for cleanup to complete, forcing shutdown");
                break;
            }
            
            debug!("Waiting for cleanup to complete...");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        info!("Event storage system stopped");
        Ok(())
    }

    /// Run cleanup with custom retention period (for testing or manual operations)
    pub async fn run_cleanup_with_retention(&self, retention_days: u32) -> Result<CleanupResult> {
        // Check if cleanup is already running
        {
            let cleanup_running = self.cleanup_running.read().await;
            if *cleanup_running {
                debug!("Cleanup already running, skipping custom cleanup");
                return Ok(CleanupResult {
                    events_deleted: 0,
                    bytes_freed: 0,
                    errors: vec!["Cleanup already running".to_string()],
                    duration: Duration::ZERO,
                });
            }
        }

        // Set cleanup running flag
        {
            let mut cleanup_running = self.cleanup_running.write().await;
            *cleanup_running = true;
        }

        let start_time = std::time::Instant::now();
        info!("Starting custom cleanup with {} day retention", retention_days);

        let result = self.perform_cleanup_with_retention(retention_days).await;

        // Clear cleanup running flag
        {
            let mut cleanup_running = self.cleanup_running.write().await;
            *cleanup_running = false;
        }

        let duration = start_time.elapsed();
        
        match &result {
            Ok(cleanup_result) => {
                info!(
                    "Custom cleanup completed: {} events deleted, {} bytes freed, {} errors, took {:?}",
                    cleanup_result.events_deleted,
                    cleanup_result.bytes_freed,
                    cleanup_result.errors.len(),
                    duration
                );
            }
            Err(e) => {
                error!("Custom cleanup failed: {}", e);
            }
        }

        result
    }

    /// Perform cleanup with custom retention period
    async fn perform_cleanup_with_retention(&self, retention_days: u32) -> Result<CleanupResult> {
        let retention_duration = Duration::from_secs(retention_days as u64 * 24 * 3600);
        let cutoff_time = SystemTime::now() - retention_duration;

        debug!("Custom cleanup cutoff time: {:?} (retention: {} days)", cutoff_time, retention_days);

        let mut events_to_delete = Vec::new();

        // Find events older than retention period
        {
            let registry = self.event_registry.read().await;
            for (event_id, metadata) in &registry.events {
                if metadata.timestamp < cutoff_time {
                    events_to_delete.push((event_id.clone(), metadata.clone()));
                }
            }
        }

        info!("Found {} events to delete with {} day retention", events_to_delete.len(), retention_days);

        let mut events_deleted = 0;
        let mut bytes_freed = 0u64;
        let mut errors = Vec::new();

        // Delete old events
        for (event_id, metadata) in events_to_delete {
            match self.delete_event(&event_id, &metadata).await {
                Ok(deleted_bytes) => {
                    events_deleted += 1;
                    bytes_freed += deleted_bytes;
                    debug!("Deleted event: {} ({} bytes)", event_id, deleted_bytes);
                }
                Err(e) => {
                    let error_msg = format!("Failed to delete event {}: {}", event_id, e);
                    error!("{}", error_msg);
                    errors.push(error_msg);
                }
            }
        }

        Ok(CleanupResult {
            events_deleted,
            bytes_freed,
            errors,
            duration: Duration::ZERO, // Will be set by caller
        })
    }

    /// Get cleanup statistics and status
    pub async fn get_cleanup_status(&self) -> CleanupStatus {
        let registry = self.event_registry.read().await;
        let cleanup_running = {
            let guard = self.cleanup_running.read().await;
            *guard
        };

        let retention_duration = Duration::from_secs(self.system_config.retention_days as u64 * 24 * 3600);
        let cutoff_time = SystemTime::now() - retention_duration;

        let mut events_eligible_for_cleanup = 0;
        let mut bytes_eligible_for_cleanup = 0u64;

        for metadata in registry.events.values() {
            if metadata.timestamp < cutoff_time {
                events_eligible_for_cleanup += 1;
                bytes_eligible_for_cleanup += metadata.total_size_bytes;
            }
        }

        CleanupStatus {
            is_running: cleanup_running,
            retention_days: self.system_config.retention_days,
            last_cleanup: registry.last_cleanup,
            events_eligible_for_cleanup,
            bytes_eligible_for_cleanup,
            total_events: registry.events.len(),
        }
    }

    /// Create a timestamped directory name for a new event
    pub fn create_event_directory_name(timestamp: SystemTime) -> String {
        let datetime: DateTime<Utc> = timestamp.into();
        // Format: YYYYMMDD_HHMMSS_mmm (19 characters total)
        datetime.format("%Y%m%d_%H%M%S_%3f").to_string()
    }

    /// Validate that a directory is safe to delete (additional safety check)
    fn validate_deletion_safety(&self, path: &Path) -> Result<()> {
        let capture_path = PathBuf::from(&self.capture_config.path);
        
        // Must be within capture directory
        if !path.starts_with(&capture_path) {
            return Err(DoorcamError::component(
                "event_storage",
                "Path is outside capture directory"
            ));
        }

        // Must be a directory
        if !path.is_dir() {
            return Err(DoorcamError::component(
                "event_storage",
                "Path is not a directory"
            ));
        }

        // Directory name must match timestamp pattern
        if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
            if !self.is_valid_event_directory_name(dir_name) {
                return Err(DoorcamError::component(
                    "event_storage",
                    "Directory name doesn't match expected pattern"
                ));
            }
        } else {
            return Err(DoorcamError::component(
                "event_storage",
                "Invalid directory name"
            ));
        }

        // Additional safety checks
        
        // Ensure path depth is reasonable (not too deep or too shallow)
        let relative_path = path.strip_prefix(&capture_path)
            .map_err(|_| DoorcamError::component("event_storage", "Failed to get relative path"))?;
        
        if relative_path.components().count() != 1 {
            return Err(DoorcamError::component(
                "event_storage",
                "Event directory must be directly under capture path"
            ));
        }

        // Check that directory is not the capture root itself
        if path == capture_path {
            return Err(DoorcamError::component(
                "event_storage",
                "Cannot delete capture root directory"
            ));
        }

        // Verify directory contains expected event files (basic sanity check)
        let has_event_files = std::fs::read_dir(path)
            .map_err(|e| DoorcamError::component("event_storage", &format!("Cannot read directory: {}", e)))?
            .any(|entry| {
                if let Ok(entry) = entry {
                    if let Some(name) = entry.file_name().to_str() {
                        return name.ends_with(".jpg") || name.ends_with(".jpeg") || name == "metadata.json";
                    }
                }
                false
            });

        if !has_event_files {
            warn!("Directory {} does not contain expected event files", path.display());
        }

        Ok(())
    }

    /// Perform dry run cleanup to see what would be deleted without actually deleting
    pub async fn dry_run_cleanup(&self) -> Result<CleanupResult> {
        let retention_duration = Duration::from_secs(self.system_config.retention_days as u64 * 24 * 3600);
        let cutoff_time = SystemTime::now() - retention_duration;

        debug!("Dry run cleanup cutoff time: {:?}", cutoff_time);

        let mut events_to_delete = Vec::new();
        let mut total_bytes = 0u64;

        // Find events older than retention period
        {
            let registry = self.event_registry.read().await;
            for (event_id, metadata) in &registry.events {
                if metadata.timestamp < cutoff_time {
                    events_to_delete.push(event_id.clone());
                    total_bytes += metadata.total_size_bytes;
                }
            }
        }

        info!("Dry run: would delete {} events ({} bytes)", events_to_delete.len(), total_bytes);

        Ok(CleanupResult {
            events_deleted: events_to_delete.len(),
            bytes_freed: total_bytes,
            errors: Vec::new(),
            duration: Duration::ZERO,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{CaptureConfig, SystemConfig},
        events::EventBus,
    };
    use tempfile::TempDir;

    use chrono::Datelike;

    fn create_test_configs() -> (CaptureConfig, SystemConfig) {
        let temp_dir = TempDir::new().unwrap();
        let capture_config = CaptureConfig {
            preroll_seconds: 5,
            postroll_seconds: 10,
            path: temp_dir.path().to_string_lossy().to_string(),
            timestamp_overlay: true,
            video_encoding: false,
            keep_images: true,
        };

        let system_config = SystemConfig {
            trim_old: true,
            retention_days: 7,
            ring_buffer_capacity: 150,
            event_bus_capacity: 100,
        };

        (capture_config, system_config)
    }

    #[tokio::test]
    async fn test_event_storage_creation() {
        let (capture_config, system_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new(10));

        let storage = EventStorage::new(capture_config, system_config, event_bus);
        
        let stats = storage.get_storage_stats().await;
        assert_eq!(stats.total_events, 0);
    }

    #[tokio::test]
    async fn test_directory_name_validation() {
        let (capture_config, system_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new(10));
        let storage = EventStorage::new(capture_config, system_config, event_bus);

        assert!(storage.is_valid_event_directory_name("20231019_143022_123"));
        assert!(!storage.is_valid_event_directory_name("invalid_name"));
        assert!(!storage.is_valid_event_directory_name("20231019_143022")); // Too short
        assert!(!storage.is_valid_event_directory_name("20231019-143022-123")); // Wrong separators
    }

    #[tokio::test]
    async fn test_timestamp_parsing() {
        let (capture_config, system_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new(10));
        let storage = EventStorage::new(capture_config, system_config, event_bus);

        let timestamp = storage.parse_timestamp_from_directory_name("20231019_143022_123").unwrap();
        
        // Verify the timestamp is reasonable (should be in 2023)
        let datetime: DateTime<Utc> = timestamp.into();
        assert_eq!(datetime.year(), 2023);
        assert_eq!(datetime.month(), 10);
        assert_eq!(datetime.day(), 19);
    }

    #[tokio::test]
    async fn test_event_directory_name_creation() {
        let now = SystemTime::now();
        let dir_name = EventStorage::create_event_directory_name(now);
        
        // Should be 19 characters long
        assert_eq!(dir_name.len(), 19);
        
        // Should contain underscores at positions 8 and 15
        assert_eq!(dir_name.chars().nth(8).unwrap(), '_');
        assert_eq!(dir_name.chars().nth(15).unwrap(), '_');
    }

    #[tokio::test]
    async fn test_storage_stats() {
        let (capture_config, system_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new(10));
        let storage = EventStorage::new(capture_config, system_config, event_bus);

        // Initially empty
        let stats = storage.get_storage_stats().await;
        assert_eq!(stats.total_events, 0);
        assert_eq!(stats.total_size_bytes, 0);
        assert!(stats.oldest_event.is_none());
        assert!(stats.newest_event.is_none());
    }

    #[tokio::test]
    async fn test_cleanup_status() {
        let (capture_config, system_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new(10));
        let storage = EventStorage::new(capture_config, system_config, event_bus);

        let status = storage.get_cleanup_status().await;
        assert!(!status.is_running);
        assert_eq!(status.retention_days, 7);
        assert_eq!(status.events_eligible_for_cleanup, 0);
        assert_eq!(status.bytes_eligible_for_cleanup, 0);
        assert_eq!(status.total_events, 0);
    }

    #[tokio::test]
    async fn test_dry_run_cleanup() {
        let (capture_config, system_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new(10));
        let storage = EventStorage::new(capture_config, system_config, event_bus);

        let result = storage.dry_run_cleanup().await.unwrap();
        assert_eq!(result.events_deleted, 0);
        assert_eq!(result.bytes_freed, 0);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validation_safety() {
        let (capture_config, system_config) = create_test_configs();
        let event_bus = Arc::new(EventBus::new(10));
        let capture_path = PathBuf::from(&capture_config.path);
        let storage = EventStorage::new(capture_config, system_config, event_bus);

        // Test validation of capture root (should fail)
        let result = storage.validate_deletion_safety(&capture_path);
        assert!(result.is_err());

        // Test validation of path outside capture directory (should fail)
        let outside_path = PathBuf::from("/tmp/not_capture");
        let result = storage.validate_deletion_safety(&outside_path);
        assert!(result.is_err());
    }
}