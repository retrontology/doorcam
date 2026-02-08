use std::time::SystemTime;

/// Display integration statistics
#[derive(Debug, Clone, Default)]
pub struct DisplayStats {
    pub frames_rendered: u64,
    pub render_errors: u64,
    pub touch_events: u64,
    pub activations: u64,
    pub last_frame_time: Option<SystemTime>,
}

impl DisplayStats {
    pub fn record_frame_render(&mut self) {
        self.frames_rendered += 1;
        self.last_frame_time = Some(SystemTime::now());
    }

    pub fn record_render_error(&mut self) {
        self.render_errors += 1;
        self.last_frame_time = Some(SystemTime::now());
    }

    pub fn record_touch_event(&mut self) {
        self.touch_events += 1;
    }

    pub fn record_activation(&mut self) {
        self.activations += 1;
    }

    pub fn render_success_rate(&self) -> f64 {
        if self.frames_rendered == 0 {
            0.0
        } else {
            (self.frames_rendered - self.render_errors) as f64 / self.frames_rendered as f64
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
