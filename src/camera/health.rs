/// Status information for the integration
#[derive(Debug, Clone)]
pub struct IntegrationStatus {
    pub camera_capturing: bool,
    pub camera_frame_count: u64,
    pub ring_buffer_capacity: usize,
    pub ring_buffer_frame_count: usize,
    pub ring_buffer_utilization: u64,
    pub frames_pushed: u64,
    pub frames_retrieved: u64,
    pub buffer_overruns: u64,
}

/// Health check result
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub status: HealthStatus,
    pub issues: Vec<String>,
    pub warnings: Vec<String>,
}

/// Health status enumeration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Warning,
    Unhealthy,
}
