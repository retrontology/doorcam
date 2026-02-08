/// Component lifecycle states
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

/// System shutdown reason
#[derive(Debug, Clone)]
pub enum ShutdownReason {
    Signal(String),
    Error(String),
    UserRequest,
    HealthCheck,
}
