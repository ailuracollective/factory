use anyhow::Result;
use axum::{
    Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

use crate::config::Config;
use crate::interface::cli::runner_cmd;
use crate::state::{IssueState, IssueStatus, load_state, save_state};

type HmacSha256 = Hmac<Sha256>;

fn max_attempts(config: &Config) -> u32 {
    if let Ok(s) = std::env::var("ISSUE_FACTORY_MAX_ATTEMPTS")
        && let Ok(n) = s.parse::<u32>()
        && n > 0
    {
        return n;
    }
    if config.defaults.max_attempts > 0 {
        config.defaults.max_attempts
    } else {
        3
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub issues: Arc<Mutex<HashMap<String, IssueState>>>,
}

pub async fn serve(config: Config) -> Result<()> {
    let issues = load_state();
    // Persist reconciled state (interrupted runs -> failed) for determinism.
    let _ = save_state(&issues);

    let state = AppState {
        config: Arc::new(config),
        issues: Arc::new(Mutex::new(issues)),
    };

    let port = state.config.webhook.port;
    let bind = std::env::var("ISSUE_FACTORY_WEBHOOK_BIND")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| state.config.webhook.bind.clone());
    let app = Router::new()
        .route("/health", get(health))
        .route("/", post(webhook))
        .route("/runner/jobs", post(runner_job))
        .with_state(state.clone());

    let ip: std::net::IpAddr = bind
        .parse()
        .unwrap_or_else(|_| std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)));
    let addr = SocketAddr::new(ip, port);
    tracing::info!(
        "Webhook listener on http://0.0.0.0:{} (secret={})",
        port,
        !state.config.webhook.secret.is_empty()
    );

    let listener = TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

async fn webhook(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: String,
) -> (StatusCode, Json<serde_json::Value>) {
    let secret = &state.config.webhook.secret;
    // Only enforce the IP allowlist in the unsigned (dev) mode; a valid
    // signature is sufficient authentication in production.
    if secret.is_empty()
        && !std::env::var("ISSUE_FACTORY_ALLOW_UNSIGNED_WEBHOOKS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        && !is_allowed_ip(addr.ip())
    {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "ip not allowed"})),
        );
    }
    let sig_header = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !verify_signature(secret, body.as_bytes(), sig_header) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "bad signature"})),
        );
    }

    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !event.is_empty()
        && !matches!(
            event.as_str(),
            "issues" | "issue_comment" | "pull_request" | "pull_request_review_comment"
        )
    {
        return (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"status": "ignored", "reason": format!("event={}", event)})),
        );
    }

    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid json"})),
            );
        }
    };
    if !payload.is_object() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid payload"})),
        );
    }

    let state_clone = state.clone();
    tokio::spawn(async move {
        let status = handle_payload(state_clone, event, payload).await;
        tracing::info!("webhook result: {}", status);
    });

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"status": "accepted"})),
    )
}

async fn runner_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> (StatusCode, Json<serde_json::Value>) {
    let secret = &state.config.gateway.callback_secret;
    let provided = headers
        .get("x-factory-dispatch-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let expected = if secret.is_empty() {
        String::new()
    } else {
        hex::encode(Sha256::digest(secret.as_bytes()))
    };
    if expected.is_empty() || provided.is_empty() || provided != expected {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "bad secret"})),
        );
    }
    let job: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid json"})),
            );
        }
    };
    let state_clone = state.clone();
    tokio::spawn(async move {
        let config = state_clone.config.clone();
        let _ = runner_cmd::process_job(config, job).await;
    });
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"status": "accepted"})),
    )
}

fn verify_signature(secret: &str, body: &[u8], signature_header: &str) -> bool {
    if secret.is_empty() {
        // Fail closed by default; allow unsigned webhooks only for local dev.
        return std::env::var("ISSUE_FACTORY_ALLOW_UNSIGNED_WEBHOOKS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    }
    if !signature_header.starts_with("sha256=") {
        return false;
    }
    let expected_hex = &signature_header[7..];
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(body);
    let result = mac.finalize().into_bytes();
    let expected = hex::decode(expected_hex).unwrap_or_default();
    if expected.len() != result.len() {
        return false;
    }
    expected
        .iter()
        .zip(result.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

fn is_allowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_loopback() {
                return true;
            }
            let o = v4.octets();
            if o[0] == 100 && (o[1] & 0xC0) == 64 {
                return true;
            }
            if o[0] == 192 && o[1] == 30 && o[2] >= 252 {
                return true;
            }
            if o[0] == 185 && o[1] == 199 && o[2] >= 108 && o[2] <= 111 {
                return true;
            }
            if o[0] == 140 && o[1] == 82 && o[2] >= 112 && o[2] <= 127 {
                return true;
            }
            if o[0] == 143 && o[1] == 55 && o[2] >= 64 && o[2] <= 79 {
                return true;
            }
            false
        }
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

mod handler;
use handler::handle_payload;
#[cfg(test)]
use handler::issue_matches_label;

#[cfg(test)]
mod tests;
