use thiserror::Error;

#[derive(Error, Debug)]
pub enum DoorcamError {
    #[error("Configuration error: {0}")]
    Config(#[from] config::ConfigError),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] toml::ser::Error),
    
    #[error("Deserialization error: {0}")]
    Deserialization(#[from] toml::de::Error),
    
    #[error("System error: {message}")]
    System { message: String },
    
    #[error("Component error in {component}: {message}")]
    Component { component: String, message: String },
}

impl DoorcamError {
    pub fn system<S: Into<String>>(message: S) -> Self {
        Self::System {
            message: message.into(),
        }
    }
    
    pub fn component<S: Into<String>>(component: S, message: S) -> Self {
        Self::Component {
            component: component.into(),
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, DoorcamError>;