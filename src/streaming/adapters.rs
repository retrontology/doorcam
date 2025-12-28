/// Frame rate adapter for dynamic quality adjustment
pub struct FrameRateAdapter {
    target_fps: f64,
    current_fps: f64,
}

impl FrameRateAdapter {
    pub fn new(target_fps: f64) -> Self {
        Self {
            target_fps,
            current_fps: target_fps,
        }
    }

    pub fn adjust(&mut self, buffer_utilization: f64, _network_load: f64) -> f64 {
        if buffer_utilization > 0.8 {
            self.current_fps = (self.current_fps * 0.9).max(self.target_fps * 0.5);
        } else if buffer_utilization < 0.5 {
            self.current_fps = (self.current_fps * 1.05).min(self.target_fps);
        }

        self.current_fps
    }
}

/// Quality adapter for dynamic image quality adjustment
pub struct QualityAdapter {
    base_quality: u8,
    current_quality: u8,
}

impl QualityAdapter {
    pub fn new(base_quality: u8) -> Self {
        Self {
            base_quality,
            current_quality: base_quality,
        }
    }

    pub fn adjust(&mut self, bandwidth_utilization: f64) -> u8 {
        if bandwidth_utilization > 0.8 {
            self.current_quality = self.current_quality.saturating_sub(5).max(30);
        } else if bandwidth_utilization < 0.5 {
            self.current_quality = (self.current_quality + 5).min(self.base_quality);
        }

        self.current_quality
    }
}
