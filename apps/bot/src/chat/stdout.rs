use crate::chat::{ChatError, ChatSink};
use async_trait::async_trait;
use notifications::Notification;

/// Default chat sink: prints to stdout. Useful for dev and tests.
pub struct StdoutSink;

#[async_trait]
impl ChatSink for StdoutSink {
    async fn send(&self, n: &Notification) -> Result<(), ChatError> {
        println!("[{}] {}: {}", n.recipient.channel, n.title, n.body);
        Ok(())
    }
}
