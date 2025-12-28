use std::time::Instant;

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
    pub fn update_frame_stats(&mut self, frame_size: usize) {
        self.frames_processed += 1;
        self.bytes_streamed += frame_size as u64;
        self.last_frame_time = Some(Instant::now());
    }

    pub fn record_dropped_frame(&mut self) {
        self.frames_dropped += 1;
    }

    pub fn calculate_fps(&self, window_seconds: f64) -> f64 {
        if let Some(last_time) = self.last_frame_time {
            let elapsed = last_time.elapsed().as_secs_f64();
            if elapsed > 0.0 && elapsed <= window_seconds {
                return self.frames_processed as f64 / elapsed;
            }
        }
        0.0
    }

    pub fn efficiency(&self) -> f64 {
        let total = self.frames_processed + self.frames_dropped;
        if total > 0 {
            self.frames_processed as f64 / total as f64
        } else {
            1.0
        }
    }
}

/// Stream server statistics and monitoring
#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    pub active_connections: u32,
    pub total_connections: u64,
    pub frames_streamed: u64,
    pub bytes_streamed: u64,
    pub errors: u64,
}
