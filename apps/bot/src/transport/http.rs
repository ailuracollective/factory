use crate::domain::handler;
use crate::domain::ports::ChatSink;
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use notifications::Notification;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub sink: Arc<dyn ChatSink>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/notify", post(notify))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn notify(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(n): Json<Notification>,
) -> (axum::http::StatusCode, &'static str) {
    let configured = std::env::var("BOT_NOTIFY_SECRET").unwrap_or_default();
    if !configured.is_empty() {
        let provided = headers
            .get("x-bot-secret")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let ok = provided.len() == configured.len()
            && provided
                .bytes()
                .zip(configured.bytes())
                .fold(0u8, |a, (x, y)| a | (x ^ y))
                == 0;
        if !ok {
            return (axum::http::StatusCode::UNAUTHORIZED, "unauthorized");
        }
    }
    match handler::handle(state.sink.as_ref(), n).await {
        Ok(()) => (axum::http::StatusCode::OK, "received"),
        Err(e) => {
            tracing::warn!("{}", e);
            (axum::http::StatusCode::BAD_REQUEST, "error")
        }
    }
}
