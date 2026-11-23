use async_trait::async_trait;
use notifications::Notification;

/// Outbound port: renders a notification in a chat platform.
#[async_trait]
pub trait ChatSink: Send + Sync {
    async fn send(&self, notification: &Notification) -> Result<(), ChatError>;
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ChatError {
    Transport(String),
    Format(String),
}

impl std::fmt::Display for ChatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatError::Transport(m) => write!(f, "transport: {}", m),
            ChatError::Format(m) => write!(f, "format: {}", m),
        }
    }
}

impl std::error::Error for ChatError {}
