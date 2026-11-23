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

async fn notify(State(state): State<AppState>, Json(n): Json<Notification>) -> &'static str {
    match handler::handle(state.sink.as_ref(), n).await {
        Ok(()) => "received",
        Err(e) => {
            tracing::warn!("{}", e);
            "error"
        }
    }
}
