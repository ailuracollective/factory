use anyhow::Result;
use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
    Router,
};
use std::net::{IpAddr, SocketAddr};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

use crate::agent::launch_agent;
use crate::config::Config;
use crate::state::{load_state, save_state};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub processed: Arc<Mutex<Vec<String>>>,
    pub threads: Arc<Mutex<HashMap<String, String>>>,
}

pub async fn serve(config: Config) -> Result<()> {
    let (processed, threads) = load_state();
    let state = AppState {
        config: Arc::new(config),
        processed: Arc::new(Mutex::new(processed)),
        threads: Arc::new(Mutex::new(threads)),
    };

    let port = state.config.webhook.port;
    let app = Router::new()
        .route("/health", get(health))
        .route("/", post(webhook))
        .with_state(state.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Webhook listener on http://0.0.0.0:{} (secret={})", port, !state.config.webhook.secret.is_empty());

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
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
    // IP allowlist (GitHub hooks + localhost + Tailscale)
    if !is_allowed_ip(addr.ip()) {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "ip not allowed"})));
    }
    // verify signature
    let secret = &state.config.webhook.secret;
    let sig_header = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !verify_signature(secret, body.as_bytes(), sig_header) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "bad signature"})));
    }

    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !event.is_empty() && event != "issues" {
        return (StatusCode::ACCEPTED, Json(serde_json::json!({"status": "ignored", "reason": format!("event={}", event)})));
    }

    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "invalid json"}))),
    };
    if !payload.is_object() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "invalid payload"})));
    }

    // spawn background task
    let state_clone = state.clone();
    tokio::spawn(async move {
        let status = handle_payload(state_clone, payload).await;
        tracing::info!("webhook result: {}", status);
    });

    (StatusCode::ACCEPTED, Json(serde_json::json!({"status": "accepted"})))
}

fn verify_signature(secret: &str, body: &[u8], signature_header: &str) -> bool {
    if secret.is_empty() {
        return true;
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
    // constant-time compare
    if expected.len() != result.len() {
        return false;
    }
    expected
        .iter()
        .zip(result.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

async fn handle_payload(state: AppState, payload: Value) -> String {
    let action = payload.get("action").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let issue = match payload.get("issue") {
        Some(i) if i.is_object() => i.clone(),
        _ => return "ignored: no issue".to_string(),
    };
    if !["opened", "reopened", "labeled"].contains(&action.as_str()) {
        return format!("ignored: action={}", action);
    }
    let number = match issue.get("number").and_then(|v| v.as_i64()) {
        Some(n) => n,
        None => return "ignored: bad number".to_string(),
    };

    let label = state.config.defaults.label.clone();
    if !label.is_empty() && !issue_matches_label(&label, &issue, &action, &payload) {
        return "ignored: label mismatch".to_string();
    }

    let repo = payload
        .get("repository")
        .and_then(|r| r.get("full_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let repo = if repo.is_empty() {
        state.config.repo.clone()
    } else {
        repo
    };
    // single-repo filter
    if !state.config.repo.is_empty() && repo != state.config.repo {
        return "ignored: repo not configured".to_string();
    }

    let key = format!("{}#{}", repo, number);

    // check + reserve processed (before launching to avoid races)
    {
        let mut proc = state.processed.lock().unwrap();
        if proc.contains(&key) {
            return format!("ignored: already processed {}", key);
        }
        proc.push(key.clone());
    }
    // save state (reserved)
    {
        let p = state.processed.lock().unwrap();
        let t = state.threads.lock().unwrap();
        let _ = save_state(&p, &t);
    }

    // resolve project path
    let projects_base = PathBuf::from(&state.config.projects_base);
    let mut project: Option<PathBuf> = None;
    for r in &state.config.repos {
        if r.github == repo || r.name == repo.split('/').last().unwrap_or("") {
            if !r.path.is_empty() {
                project = Some(PathBuf::from(&r.path));
            }
            break;
        }
    }
    if project.is_none() {
        project = Some(projects_base.join(repo.split('/').last().unwrap_or("")));
    }
    let project = project.unwrap();
    if !project.exists() {
        // un-reserve
        let mut proc = state.processed.lock().unwrap();
        proc.retain(|k| k != &key);
        let p = state.processed.lock().unwrap();
        let t = state.threads.lock().unwrap();
        let _ = save_state(&p, &t);
        return format!("ignored: no project for {}", repo);
    }

    let thread_id = launch_agent(&issue, &project, &state.config);

    if let Some(ref id) = thread_id {
        let mut thr = state.threads.lock().unwrap();
        thr.insert(key.clone(), id.clone());
        let p = state.processed.lock().unwrap();
        let t = state.threads.lock().unwrap();
        let _ = save_state(&p, &t);
    }

    match thread_id {
        Some(_) => format!("launched {}", key),
        None => format!("failed {}", key),
    }
}

fn is_allowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_loopback() {
                return true;
            }
            let o = v4.octets();
            // Tailscale CGNAT range 100.64.0.0/10
            if o[0] == 100 && (o[1] & 0xC0) == 64 {
                return true;
            }
            // GitHub hook IP ranges
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

fn issue_matches_label(label: &str, issue: &Value, action: &str, payload: &Value) -> bool {
    if action == "labeled" {
        let name = payload
            .get("label")
            .and_then(|l| l.get("name"))
            .and_then(|n| n.as_str());
        return name == Some(label);
    }
    let labels = issue.get("labels").and_then(|v| v.as_array());
    match labels {
        Some(arr) => arr
            .iter()
            .filter_map(|l| l.get("name").and_then(|n| n.as_str()))
            .any(|n| n == label),
        None => false,
    }
}
