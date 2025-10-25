use crate::frame::FrameData;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::{debug, trace, warn};

/// Lock-free circular buffer for frame storage with preroll capability
pub struct RingBuffer {
    /// Array of frame slots protected by RwLocks
    frames: Vec<RwLock<Option<FrameData>>>,
    /// Current write position (atomic for lock-free writes)
    write_index: AtomicUsize,
    /// Total capacity of the buffer
    capacity: usize,
    /// Duration of preroll frames to maintain
    preroll_duration: Duration,
    /// Frame counter for generating unique IDs
    frame_counter: AtomicU64,
    /// Statistics
    stats: RingBufferStats,
}

/// Statistics for ring buffer performance monitoring
#[derive(Debug)]
pub struct RingBufferStats {
    /// Total frames pushed to buffer
    pub frames_pushed: AtomicU64,
    /// Total frames retrieved from buffer
    pub frames_retrieved: AtomicU64,
    /// Number of buffer overruns (old frames overwritten)
    pub buffer_overruns: AtomicU64,
    /// Current buffer utilization (0-100)
    pub utilization_percent: AtomicU64,
}

impl RingBufferStats {
    fn new() -> Self {
        Self {
            frames_pushed: AtomicU64::new(0),
            frames_retrieved: AtomicU64::new(0),
            buffer_overruns: AtomicU64::new(0),
            utilization_percent: AtomicU64::new(0),
        }
    }
    
    /// Get current statistics as a snapshot
    pub fn snapshot(&self) -> RingBufferStatsSnapshot {
        RingBufferStatsSnapshot {
            frames_pushed: self.frames_pushed.load(Ordering::Relaxed),
            frames_retrieved: self.frames_retrieved.load(Ordering::Relaxed),
            buffer_overruns: self.buffer_overruns.load(Ordering::Relaxed),
            utilization_percent: self.utilization_percent.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of ring buffer statistics
#[derive(Debug, Clone)]
pub struct RingBufferStatsSnapshot {
    pub frames_pushed: u64,
    pub frames_retrieved: u64,
    pub buffer_overruns: u64,
    pub utilization_percent: u64,
}

impl RingBuffer {
    /// Create a new ring buffer with specified capacity and preroll duration
    /// 
    /// # Arguments
    /// * `capacity` - Maximum number of frames to store
    /// * `preroll_duration` - Duration of frames to keep for preroll
    /// 
    /// # Example
    /// ```
    /// use std::time::Duration;
    /// use doorcam::ring_buffer::RingBuffer;
    /// 
    /// let buffer = RingBuffer::new(100, Duration::from_secs(5));
    /// ```
    pub fn new(capacity: usize, preroll_duration: Duration) -> Self {
        if capacity == 0 {
            panic!("Ring buffer capacity must be greater than 0");
        }
        
        let mut frames = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            frames.push(RwLock::new(None));
        }
        
        debug!(
            "Created ring buffer with capacity {} and preroll duration {:?}",
            capacity, preroll_duration
        );
        
        Self {
            frames,
            write_index: AtomicUsize::new(0),
            capacity,
            preroll_duration,
            frame_counter: AtomicU64::new(0),
            stats: RingBufferStats::new(),
        }
    }
    
    /// Push a new frame into the buffer
    /// 
    /// This operation is lock-free for the write index but uses async locks
    /// for the frame slots to ensure thread safety.
    /// 
    /// # Arguments
    /// * `frame` - Frame data to store
    pub async fn push_frame(&self, frame: FrameData) {
        let index = self.write_index.fetch_add(1, Ordering::Relaxed) % self.capacity;
        
        trace!(
            "Pushing frame {} to buffer slot {} (id: {})",
            frame.id,
            index,
            frame.id
        );
        
        // Check if we're overwriting an existing frame
        {
            let slot = self.frames[index].read().await;
            if slot.is_some() {
                self.stats.buffer_overruns.fetch_add(1, Ordering::Relaxed);
                trace!("Buffer overrun at slot {}", index);
            }
        }
        
        // Write the new frame
        {
            let mut slot = self.frames[index].write().await;
            *slot = Some(frame);
        }
        
        self.stats.frames_pushed.fetch_add(1, Ordering::Relaxed);
        self.update_utilization().await;
    }
    
    /// Get the most recently pushed frame
    /// 
    /// # Returns
    /// * `Some(FrameData)` - The latest frame if available
    /// * `None` - If no frames have been pushed yet
    pub async fn get_latest_frame(&self) -> Option<FrameData> {
        let current_index = self.write_index.load(Ordering::Relaxed);
        if current_index == 0 {
            return None;
        }
        
        let index = (current_index - 1) % self.capacity;
        let slot = self.frames[index].read().await;
        
        if let Some(frame) = slot.as_ref() {
            self.stats.frames_retrieved.fetch_add(1, Ordering::Relaxed);
            trace!("Retrieved latest frame {} from slot {}", frame.id, index);
            Some(frame.clone())
        } else {
            None
        }
    }
    
    /// Get all frames within the preroll time window
    /// 
    /// Returns frames captured within the preroll duration, ordered chronologically
    /// (oldest first). This is used for motion-triggered recording to include
    /// frames captured before motion was detected.
    /// 
    /// # Returns
    /// Vector of frames within the preroll window, ordered by timestamp
    pub async fn get_preroll_frames(&self) -> Vec<FrameData> {
        let now = SystemTime::now();
        let cutoff = now - self.preroll_duration;
        let mut frames = Vec::new();
        
        let current_index = self.write_index.load(Ordering::Relaxed);
        
        debug!(
            "Collecting preroll frames from {} slots, cutoff: {:?}",
            self.capacity,
            cutoff.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default()
        );
        
        // Collect frames within preroll window
        // Start from the most recent and work backwards
        for i in 0..self.capacity {
            let index = (current_index + self.capacity - 1 - i) % self.capacity;
            let slot = self.frames[index].read().await;
            
            if let Some(frame) = slot.as_ref() {
                if frame.timestamp >= cutoff {
                    frames.push(frame.clone());
                    trace!(
                        "Added frame {} to preroll (age: {}ms)",
                        frame.id,
                        frame.age_ms()
                    );
                } else {
                    // Frames are ordered by time, so we can stop here
                    trace!(
                        "Frame {} too old for preroll (age: {}ms), stopping collection",
                        frame.id,
                        frame.age_ms()
                    );
                    break;
                }
            }
        }
        
        // Reverse to get chronological order (oldest first)
        frames.reverse();
        
        debug!("Collected {} preroll frames", frames.len());
        frames
    }
    
    /// Get frames within a specific time range
    /// 
    /// # Arguments
    /// * `start_time` - Start of time range (inclusive)
    /// * `end_time` - End of time range (inclusive)
    /// 
    /// # Returns
    /// Vector of frames within the time range, ordered by timestamp
    pub async fn get_frames_in_range(
        &self,
        start_time: SystemTime,
        end_time: SystemTime,
    ) -> Vec<FrameData> {
        let mut frames = Vec::new();
        let current_index = self.write_index.load(Ordering::Relaxed);
        
        debug!(
            "Collecting frames in range {:?} to {:?}",
            start_time.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default(),
            end_time.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default()
        );
        
        // Check all slots for frames in range
        for i in 0..self.capacity {
            let index = (current_index + self.capacity - 1 - i) % self.capacity;
            let slot = self.frames[index].read().await;
            
            if let Some(frame) = slot.as_ref() {
                if frame.timestamp >= start_time && frame.timestamp <= end_time {
                    frames.push(frame.clone());
                }
            }
        }
        
        // Sort by timestamp to ensure chronological order
        frames.sort_by_key(|f| f.timestamp);
        
        debug!("Collected {} frames in time range", frames.len());
        frames
    }
    
    /// Get the current buffer capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    
    /// Get the configured preroll duration
    pub fn preroll_duration(&self) -> Duration {
        self.preroll_duration
    }
    
    /// Get current buffer statistics
    pub fn stats(&self) -> RingBufferStatsSnapshot {
        self.stats.snapshot()
    }
    
    /// Clear all frames from the buffer
    pub async fn clear(&self) {
        debug!("Clearing ring buffer");
        
        for slot in &self.frames {
            let mut frame_slot = slot.write().await;
            *frame_slot = None;
        }
        
        // Reset write index but keep frame counter for unique IDs
        self.write_index.store(0, Ordering::Relaxed);
        
        // Reset some stats but keep counters
        self.stats.utilization_percent.store(0, Ordering::Relaxed);
        
        debug!("Ring buffer cleared");
    }
    
    /// Get the next frame ID for new frames
    pub fn next_frame_id(&self) -> u64 {
        self.frame_counter.fetch_add(1, Ordering::Relaxed)
    }
    
    /// Update buffer utilization statistics
    async fn update_utilization(&self) {
        let mut occupied_slots = 0;
        
        // Count occupied slots (this is expensive, so we might want to optimize)
        for slot in &self.frames {
            let frame_slot = slot.read().await;
            if frame_slot.is_some() {
                occupied_slots += 1;
            }
        }
        
        let utilization = (occupied_slots * 100) / self.capacity;
        self.stats.utilization_percent.store(utilization as u64, Ordering::Relaxed);
    }
    
    /// Get approximate number of frames currently in buffer
    /// 
    /// This is an approximation and may not be 100% accurate due to
    /// concurrent access, but is useful for monitoring.
    pub async fn approximate_frame_count(&self) -> usize {
        let mut count = 0;
        
        for slot in &self.frames {
            let frame_slot = slot.read().await;
            if frame_slot.is_some() {
                count += 1;
            }
        }
        
        count
    }
}

/// Builder for creating ring buffers with custom configuration
pub struct RingBufferBuilder {
    capacity: Option<usize>,
    preroll_duration: Option<Duration>,
}

impl RingBufferBuilder {
    /// Create a new ring buffer builder
    pub fn new() -> Self {
        Self {
            capacity: None,
            preroll_duration: None,
        }
    }
    
    /// Set the buffer capacity
    pub fn capacity(mut self, capacity: usize) -> Self {
        self.capacity = Some(capacity);
        self
    }
    
    /// Set the preroll duration
    pub fn preroll_duration(mut self, duration: Duration) -> Self {
        self.preroll_duration = Some(duration);
        self
    }
    
    /// Build the ring buffer with specified configuration
    pub fn build(self) -> Result<RingBuffer, crate::error::DoorcamError> {
        let capacity = self.capacity.ok_or_else(|| {
            crate::error::DoorcamError::system("Ring buffer capacity must be specified")
        })?;
        
        let preroll_duration = self.preroll_duration.ok_or_else(|| {
            crate::error::DoorcamError::system("Ring buffer preroll duration must be specified")
        })?;
        
        if capacity == 0 {
            return Err(crate::error::DoorcamError::system(
                "Ring buffer capacity must be greater than 0",
            ));
        }
        
        Ok(RingBuffer::new(capacity, preroll_duration))
    }
}

impl Default for RingBufferBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;
    
    fn create_test_frame(id: u64, timestamp: SystemTime) -> FrameData {
        FrameData::new(
            id,
            timestamp,
            vec![0u8; 1024],
            640,
            480,
            crate::frame::FrameFormat::Mjpeg,
        )
    }
    
    #[tokio::test]
    async fn test_ring_buffer_creation() {
        let buffer = RingBuffer::new(10, Duration::from_secs(5));
        assert_eq!(buffer.capacity(), 10);
        assert_eq!(buffer.preroll_duration(), Duration::from_secs(5));
    }
    
    #[tokio::test]
    async fn test_push_and_get_latest() {
        let buffer = RingBuffer::new(5, Duration::from_secs(1));
        
        // Initially no frames
        assert!(buffer.get_latest_frame().await.is_none());
        
        // Push a frame
        let frame = create_test_frame(1, SystemTime::now());
        buffer.push_frame(frame.clone()).await;
        
        // Should be able to retrieve it
        let latest = buffer.get_latest_frame().await;
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().id, 1);
    }
    
    #[tokio::test]
    async fn test_buffer_wraparound() {
        let buffer = RingBuffer::new(3, Duration::from_secs(1));
        
        // Fill buffer beyond capacity
        for i in 1..=5 {
            let frame = create_test_frame(i, SystemTime::now());
            buffer.push_frame(frame).await;
        }
        
        // Latest should be frame 5
        let latest = buffer.get_latest_frame().await;
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().id, 5);
        
        // Check that we have overruns
        let stats = buffer.stats();
        assert!(stats.buffer_overruns > 0);
    }
    
    #[tokio::test]
    async fn test_preroll_frames() {
        let buffer = RingBuffer::new(10, Duration::from_millis(100));
        
        let now = SystemTime::now();
        
        // Add frames with different timestamps
        let frame1 = create_test_frame(1, now - Duration::from_millis(150)); // Too old
        let frame2 = create_test_frame(2, now - Duration::from_millis(50));  // Within preroll
        let frame3 = create_test_frame(3, now - Duration::from_millis(25));  // Within preroll
        let frame4 = create_test_frame(4, now);                              // Current
        
        buffer.push_frame(frame1).await;
        buffer.push_frame(frame2).await;
        buffer.push_frame(frame3).await;
        buffer.push_frame(frame4).await;
        
        let preroll = buffer.get_preroll_frames().await;
        
        // Should have frames 2, 3, 4 (frame 1 is too old)
        assert_eq!(preroll.len(), 3);
        assert_eq!(preroll[0].id, 2); // Oldest first
        assert_eq!(preroll[1].id, 3);
        assert_eq!(preroll[2].id, 4); // Newest last
    }
    
    #[tokio::test]
    async fn test_frames_in_range() {
        let buffer = RingBuffer::new(10, Duration::from_secs(1));
        
        let base_time = SystemTime::now() - Duration::from_secs(10);
        
        // Add frames at different times
        for i in 0..5 {
            let timestamp = base_time + Duration::from_secs(i * 2);
            let frame = create_test_frame(i + 1, timestamp);
            buffer.push_frame(frame).await;
        }
        
        // Get frames in middle range
        let start_time = base_time + Duration::from_secs(2);
        let end_time = base_time + Duration::from_secs(6);
        
        let frames = buffer.get_frames_in_range(start_time, end_time).await;
        
        // Should have frames 2, 3, 4 (timestamps at 2s, 4s, 6s)
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].id, 2);
        assert_eq!(frames[1].id, 3);
        assert_eq!(frames[2].id, 4);
    }
    
    #[tokio::test]
    async fn test_clear_buffer() {
        let buffer = RingBuffer::new(5, Duration::from_secs(1));
        
        // Add some frames
        for i in 1..=3 {
            let frame = create_test_frame(i, SystemTime::now());
            buffer.push_frame(frame).await;
        }
        
        assert!(buffer.get_latest_frame().await.is_some());
        
        // Clear buffer
        buffer.clear().await;
        
        assert!(buffer.get_latest_frame().await.is_none());
        assert_eq!(buffer.approximate_frame_count().await, 0);
    }
    
    #[tokio::test]
    async fn test_builder_pattern() {
        let buffer = RingBufferBuilder::new()
            .capacity(20)
            .preroll_duration(Duration::from_secs(3))
            .build()
            .unwrap();
        
        assert_eq!(buffer.capacity(), 20);
        assert_eq!(buffer.preroll_duration(), Duration::from_secs(3));
    }
    
    #[tokio::test]
    async fn test_builder_validation() {
        // Missing capacity
        let result = RingBufferBuilder::new()
            .preroll_duration(Duration::from_secs(1))
            .build();
        assert!(result.is_err());
        
        // Missing preroll duration
        let result = RingBufferBuilder::new()
            .capacity(10)
            .build();
        assert!(result.is_err());
        
        // Zero capacity
        let result = RingBufferBuilder::new()
            .capacity(0)
            .preroll_duration(Duration::from_secs(1))
            .build();
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_concurrent_access() {
        let buffer = Arc::new(RingBuffer::new(100, Duration::from_secs(1)));
        let mut handles = Vec::new();
        
        // Spawn multiple writers
        for i in 0..10 {
            let buffer_clone = Arc::clone(&buffer);
            let handle = tokio::spawn(async move {
                for j in 0..10 {
                    let frame = create_test_frame(
                        (i * 10 + j) as u64,
                        SystemTime::now(),
                    );
                    buffer_clone.push_frame(frame).await;
                }
            });
            handles.push(handle);
        }
        
        // Spawn readers
        for _ in 0..5 {
            let buffer_clone = Arc::clone(&buffer);
            let handle = tokio::spawn(async move {
                for _ in 0..20 {
                    let _ = buffer_clone.get_latest_frame().await;
                    let _ = buffer_clone.get_preroll_frames().await;
                    sleep(Duration::from_millis(1)).await;
                }
            });
            handles.push(handle);
        }
        
        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }
        
        // Verify we have frames and no panics occurred
        assert!(buffer.get_latest_frame().await.is_some());
        let stats = buffer.stats();
        assert_eq!(stats.frames_pushed, 100);
    }

    #[tokio::test]
    async fn test_ring_buffer_stress() {
        use std::sync::Arc;
        
        let buffer = Arc::new(RingBuffer::new(100, Duration::from_millis(10)));
        let mut handles = Vec::new();
        
        // Spawn multiple producers
        for producer_id in 0..5 {
            let buffer_clone = Arc::clone(&buffer);
            let handle = tokio::spawn(async move {
                for i in 0..50 {
                    let frame_id = (producer_id * 50 + i) as u64;
                    let frame = create_test_frame(frame_id, SystemTime::now());
                    buffer_clone.push_frame(frame).await;
                    
                    // Small delay to simulate realistic frame rates
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            });
            handles.push(handle);
        }
        
        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }
        
        // Buffer should be in a consistent state
        let stats = buffer.stats();
        assert!(stats.frames_pushed >= 250); // 5 producers * 50 frames each
        assert!(stats.utilization_percent <= 100);
    }
} 
