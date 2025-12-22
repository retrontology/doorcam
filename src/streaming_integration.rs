use crate::{
    config::StreamConfig,
    events::{DoorcamEvent, EventBus},
    ring_buffer::RingBuffer,
    streaming::StreamServer,
    error::Result,
};
use std::sync::Arc;
use tokio::time::{interval, Duration, Instant};
use tracing::{debug, error, info, warn};

/// Integration layer between streaming server and ring buffer
/// Handles frame synchronization, rate limiting, and quality adaptation
pub struct StreamingIntegration {
    ring_buffer: Arc<RingBuffer>,
    event_bus: Arc<EventBus>,
    config: StreamConfig,
    stats: StreamingStats,
    target_fps: u32,
}

/// Statistics for streaming performance monitoring
#[derive(Debug, Clone, Default)]
pub struct StreamingStats {
    pub frames_processed: u64,
    pub frames_dropped: u64,
    pub bytes_streamed: u64,
    pub active_connections: u32,
    pub average_fps: f64,
    pub last_frame_time: Option<Instant>,
}

impl StreamingStats {
    /// Update statistics with a new frame
    pub fn update_frame_stats(&mut self, frame_size: usize) {
        self.frames_processed += 1;
        self.bytes_streamed += frame_size as u64;
        self.last_frame_time = Some(Instant::now());
    }

    /// Record a dropped frame
    pub fn record_dropped_frame(&mut self) {
        self.frames_dropped += 1;
    }

    /// Calculate current FPS over the last period
    pub fn calculate_fps(&self, window_seconds: f64) -> f64 {
        if let Some(last_time) = self.last_frame_time {
            let elapsed = last_time.elapsed().as_secs_f64();
            if elapsed > 0.0 && elapsed <= window_seconds {
                return self.frames_processed as f64 / elapsed;
            }
        }
        0.0
    }

    /// Get streaming efficiency (frames processed / total frames)
    pub fn efficiency(&self) -> f64 {
        let total = self.frames_processed + self.frames_dropped;
        if total > 0 {
            self.frames_processed as f64 / total as f64
        } else {
            1.0
        }
    }
}

impl StreamingIntegration {
    /// Create a new streaming integration
    pub fn new(
        config: StreamConfig,
        ring_buffer: Arc<RingBuffer>,
        event_bus: Arc<EventBus>,
        target_fps: u32,
    ) -> Result<Self> {
        Ok(Self {
            ring_buffer,
            event_bus,
            config,
            stats: StreamingStats::default(),
            target_fps,
        })
    }

    /// Start the streaming integration
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting streaming integration");

        // Start the HTTP server in a background task
        let server_handle = {
            let server = StreamServer::new(
                self.config.clone(),
                Arc::clone(&self.ring_buffer),
                Arc::clone(&self.event_bus),
                self.target_fps,
            );
            
            tokio::spawn(async move {
                if let Err(e) = server.start().await {
                    error!("Stream server error: {}", e);
                }
            })
        };

        // Start frame synchronization monitoring
        let sync_handle = self.start_frame_sync_monitor().await?;

        // Start statistics reporting
        let stats_handle = self.start_stats_reporter().await?;

        // Wait for any task to complete (which indicates an error)
        tokio::select! {
            result = server_handle => {
                match result {
                    Ok(_) => info!("Stream server completed"),
                    Err(e) => error!("Stream server task error: {}", e),
                }
            }
            result = sync_handle => {
                match result {
                    Ok(_) => info!("Frame sync monitor completed"),
                    Err(e) => error!("Frame sync monitor task error: {}", e),
                }
            }
            result = stats_handle => {
                match result {
                    Ok(_) => info!("Stats reporter completed"),
                    Err(e) => error!("Stats reporter task error: {}", e),
                }
            }
        }

        Ok(())
    }

    /// Start frame synchronization monitoring
    async fn start_frame_sync_monitor(&self) -> Result<tokio::task::JoinHandle<()>> {
        let ring_buffer = Arc::clone(&self.ring_buffer);
        let event_bus = Arc::clone(&self.event_bus);
        
        let handle = tokio::spawn(async move {
            let mut event_receiver = event_bus.subscribe();
            let mut last_frame_id = 0u64;
            let mut frame_check_interval = interval(Duration::from_millis(100));

            info!("Frame synchronization monitor started");

            loop {
                tokio::select! {
                    _ = frame_check_interval.tick() => {
                        // Check for new frames and ensure streaming is synchronized
                        if let Some(latest_frame) = ring_buffer.get_latest_frame().await {
                            if latest_frame.id > last_frame_id {
                                let frames_missed = latest_frame.id - last_frame_id - 1;
                                if frames_missed > 0 {
                                    debug!(
                                        "Frame sync: {} frames between {} and {} (missed: {})",
                                        latest_frame.id - last_frame_id,
                                        last_frame_id,
                                        latest_frame.id,
                                        frames_missed
                                    );
                                }
                                last_frame_id = latest_frame.id;
                            }
                        }
                    }
                    
                    event_result = event_receiver.recv() => {
                        match event_result {
                            Ok(DoorcamEvent::FrameReady { frame_id, .. }) => {
                                debug!("Frame sync: Frame {} ready for streaming", frame_id);
                            }
                            Ok(DoorcamEvent::SystemError { component, error }) if component == "camera" => {
                                warn!("Camera error detected, may affect streaming: {}", error);
                            }
                            Ok(_) => {
                                // Ignore other events
                            }
                            Err(e) => {
                                error!("Frame sync monitor event error: {}", e);
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    /// Start statistics reporting
    async fn start_stats_reporter(&self) -> Result<tokio::task::JoinHandle<()>> {
        let event_bus = Arc::clone(&self.event_bus);
        let ring_buffer = Arc::clone(&self.ring_buffer);
        
        let handle = tokio::spawn(async move {
            let mut stats_interval = interval(Duration::from_secs(30));
            let mut last_stats_time = Instant::now();
            let mut last_frame_count = 0u64;

            info!("Statistics reporter started");

            loop {
                stats_interval.tick().await;

                // Collect current statistics
                let ring_stats = ring_buffer.stats();
                let current_time = Instant::now();
                let elapsed = current_time.duration_since(last_stats_time).as_secs_f64();
                
                let frames_processed = ring_stats.frames_pushed - last_frame_count;
                let fps = if elapsed > 0.0 {
                    frames_processed as f64 / elapsed
                } else {
                    0.0
                };

                info!(
                    "Streaming stats: {:.1} FPS, {}% buffer utilization, {} total frames",
                    fps,
                    ring_stats.utilization_percent,
                    ring_stats.frames_pushed
                );

                // Update for next iteration
                last_stats_time = current_time;
                last_frame_count = ring_stats.frames_pushed;

                // Publish statistics event
                let _ = event_bus.publish(DoorcamEvent::SystemError {
                    component: "streaming_stats".to_string(),
                    error: format!(
                        "FPS: {:.1}, Buffer: {}%, Frames: {}",
                        fps,
                        ring_stats.utilization_percent,
                        ring_stats.frames_pushed
                    ),
                }).await;
            }
        });

        Ok(handle)
    }

    /// Get current streaming statistics
    pub fn stats(&self) -> &StreamingStats {
        &self.stats
    }

    /// Check if streaming is healthy
    pub fn is_healthy(&self) -> bool {
        // Consider streaming healthy if we've processed frames recently
        if let Some(last_time) = self.stats.last_frame_time {
            last_time.elapsed() < Duration::from_secs(10)
        } else {
            false
        }
    }
}

/// Frame rate adapter for dynamic quality adjustment
pub struct FrameRateAdapter {
    target_fps: f64,
    current_fps: f64,
    adaptation_factor: f64,
    min_fps: f64,
    max_fps: f64,
}

impl FrameRateAdapter {
    /// Create a new frame rate adapter
    pub fn new(target_fps: f64) -> Self {
        Self {
            target_fps,
            current_fps: target_fps,
            adaptation_factor: 0.1, // 10% adaptation per update
            min_fps: 1.0,
            max_fps: 60.0,
        }
    }

    /// Update the adapter with current performance metrics
    pub fn update(&mut self, actual_fps: f64, buffer_utilization: f64) {
        // Adapt frame rate based on buffer utilization and performance
        let target_adjustment = if buffer_utilization > 0.8 {
            // High buffer utilization, reduce frame rate
            -self.adaptation_factor
        } else if buffer_utilization < 0.3 && actual_fps < self.target_fps {
            // Low buffer utilization and low FPS, increase frame rate
            self.adaptation_factor
        } else {
            0.0
        };

        self.current_fps = (self.current_fps * (1.0 + target_adjustment))
            .clamp(self.min_fps, self.max_fps);

        debug!(
            "Frame rate adaptation: target={:.1}, current={:.1}, actual={:.1}, buffer={:.1}%",
            self.target_fps, self.current_fps, actual_fps, buffer_utilization * 100.0
        );
    }

    /// Get the current recommended frame interval
    pub fn frame_interval(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.current_fps)
    }

    /// Get the current FPS setting
    pub fn current_fps(&self) -> f64 {
        self.current_fps
    }
}

/// Quality adapter for dynamic image quality adjustment
pub struct QualityAdapter {
    base_quality: u8,
    current_quality: u8,
    min_quality: u8,
    max_quality: u8,
}

impl QualityAdapter {
    /// Create a new quality adapter
    pub fn new(base_quality: u8) -> Self {
        Self {
            base_quality,
            current_quality: base_quality,
            min_quality: 30,
            max_quality: 95,
        }
    }

    /// Update quality based on network conditions
    pub fn update(&mut self, bandwidth_utilization: f64, frame_drop_rate: f64) {
        if bandwidth_utilization > 0.9 || frame_drop_rate > 0.1 {
            // High bandwidth usage or frame drops, reduce quality
            self.current_quality = (self.current_quality.saturating_sub(5))
                .max(self.min_quality);
        } else if bandwidth_utilization < 0.5 && frame_drop_rate < 0.01 {
            // Low bandwidth usage and no drops, increase quality
            self.current_quality = (self.current_quality.saturating_add(2))
                .min(self.max_quality);
        }

        debug!(
            "Quality adaptation: base={}, current={}, bandwidth={:.1}%, drops={:.1}%",
            self.base_quality,
            self.current_quality,
            bandwidth_utilization * 100.0,
            frame_drop_rate * 100.0
        );
    }

    /// Get the current quality setting
    pub fn current_quality(&self) -> u8 {
        self.current_quality
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        events::EventBus,
        ring_buffer::RingBuffer,
    };
    use std::time::Duration;

    #[tokio::test]
    async fn test_streaming_integration_creation() {
        let config = StreamConfig {
            ip: "127.0.0.1".to_string(),
            port: 8080,
            rotation: None,
        };
        let ring_buffer = Arc::new(RingBuffer::new(10, Duration::from_secs(1)));
        let event_bus = Arc::new(EventBus::new(10));

        let integration = StreamingIntegration::new(config, ring_buffer, event_bus, 30);
        assert!(integration.is_ok());
    }

    #[test]
    fn test_streaming_stats() {
        let mut stats = StreamingStats::default();
        
        // Update with frame data
        stats.update_frame_stats(1024);
        assert_eq!(stats.frames_processed, 1);
        assert_eq!(stats.bytes_streamed, 1024);
        
        // Record dropped frame
        stats.record_dropped_frame();
        assert_eq!(stats.frames_dropped, 1);
        
        // Check efficiency
        assert_eq!(stats.efficiency(), 0.5); // 1 processed / 2 total
    }

    #[test]
    fn test_frame_rate_adapter() {
        let mut adapter = FrameRateAdapter::new(30.0);
        
        // High buffer utilization should reduce FPS
        adapter.update(25.0, 0.9);
        assert!(adapter.current_fps() < 30.0);
        
        // Low buffer utilization should increase FPS
        adapter.update(15.0, 0.2);
        // FPS should increase from the reduced value
    }

    #[test]
    fn test_quality_adapter() {
        let mut adapter = QualityAdapter::new(80);
        
        // High bandwidth utilization should reduce quality
        adapter.update(0.95, 0.15);
        assert!(adapter.current_quality() < 80);
        
        // Low bandwidth utilization should increase quality
        adapter.update(0.3, 0.005);
        // Quality should increase from the reduced value
    }

    #[tokio::test]
    async fn test_streaming_health_check() {
        let config = StreamConfig {
            ip: "127.0.0.1".to_string(),
            port: 8080,
            rotation: None,
        };
        let ring_buffer = Arc::new(RingBuffer::new(10, Duration::from_secs(1)));
        let event_bus = Arc::new(EventBus::new(10));

        let integration = StreamingIntegration::new(config, ring_buffer, event_bus, 30).unwrap();
        
        // Should not be healthy initially (no frames processed)
        assert!(!integration.is_healthy());
    }
}