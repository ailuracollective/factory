use crate::domain::ports::{ChatError, ChatSink};
use notifications::Notification;

/// Handles an incoming notification: validate, then render via the chat sink.
pub async fn handle<S: ChatSink + ?Sized>(sink: &S, n: Notification) -> Result<(), HandleError> {
    n.validate().map_err(HandleError::Invalid)?;
    sink.send(&n).await.map_err(HandleError::Chat)
}

#[derive(Debug)]
pub enum HandleError {
    Invalid(notifications::ValidationError),
    Chat(ChatError),
}

impl std::fmt::Display for HandleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandleError::Invalid(e) => write!(f, "invalid notification: {}", e),
            HandleError::Chat(e) => write!(f, "chat error: {}", e),
        }
    }
}

impl std::error::Error for HandleError {}
