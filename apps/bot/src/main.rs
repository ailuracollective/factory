mod chat;
mod domain;
mod transport;

use crate::chat::discord::DiscordWebhookSink;
use crate::chat::stdout::StdoutSink;
use crate::transport::http::{AppState, router};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let sink: Arc<dyn crate::domain::ports::ChatSink> = match std::env::var("DISCORD_WEBHOOK_URL") {
        Ok(url) if !url.is_empty() => {
            tracing::info!("using Discord webhook sink");
            Arc::new(DiscordWebhookSink::new(url))
        }
        _ => {
            tracing::info!("DISCORD_WEBHOOK_URL not set; using stdout sink");
            Arc::new(StdoutSink)
        }
    };

    let state = AppState { sink };
    let app = router(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8790));
    tracing::info!("bot listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
