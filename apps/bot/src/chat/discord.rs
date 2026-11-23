use crate::chat::{ChatError, ChatSink};
use async_trait::async_trait;
use notifications::Notification;

/// Sends a notification to a Discord channel via an incoming webhook URL.
pub struct DiscordWebhookSink {
    pub webhook_url: String,
    pub client: reqwest::Client,
}

impl DiscordWebhookSink {
    pub fn new(webhook_url: String) -> Self {
        Self {
            webhook_url,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl ChatSink for DiscordWebhookSink {
    async fn send(&self, n: &Notification) -> Result<(), ChatError> {
        let payload = serde_json::json!({
            "content": format!("**{}**\n{}", n.title, n.body),
        });
        self.client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ChatError::Transport(e.to_string()))?
            .error_for_status()
            .map_err(|e| ChatError::Transport(e.to_string()))?;
        Ok(())
    }
}
