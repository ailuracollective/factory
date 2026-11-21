use anyhow::Result;
use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
    Router,
};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

use crate::config::Config;
use crate::state::{load_state, save_state, IssueState, IssueStatus};

type HmacSha256 = Hmac<Sha256>;

fn max_attempts(config: &Config) -> u32 {
    if let Ok(s) = std::env::var("ISSUE_FACTORY_MAX_ATTEMPTS") {
        if let Ok(n) = s.parse::<u32>() {
            if n > 0 {
                return n;
            }
        }
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
    if !is_allowed_ip(addr.ip()) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "ip not allowed"})),
        );
    }
    let secret = &state.config.webhook.secret;
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
            )
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
    if expected.len() != result.len() {
        return false;
    }
    expected
        .iter()
        .zip(result.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

async fn handle_payload(state: AppState, event: String, payload: Value) -> String {
    let action = payload
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mention_eligible = matches!(
        (event.as_str(), action.as_str()),
        ("issue_comment", "created" | "edited")
            | ("issues", "opened" | "edited")
            | ("pull_request", "opened" | "edited")
            | ("pull_request_review_comment", "created" | "edited")
    );
    if mention_eligible
        && state.config.mentions.enabled
        && !state.config.mentions.usernames.is_empty()
    {
        if let Some(mention_event) = crate::mentions::event_from_webhook(&event, &payload) {
            let config = state.config.clone();
            let outcome = tokio::task::spawn_blocking(move || {
                let store = crate::mentions::SqliteEventStore;
                let dispatcher = crate::mentions::AgentDispatcher {
                    config: config.clone(),
                };
                let authorizer = crate::mentions::GhAuthorizer;
                let engine =
                    crate::mentions::Engine::new(&config, &store, &dispatcher, &authorizer);
                engine.handle(&mention_event, crate::state::now())
            })
            .await
            .unwrap_or(crate::mentions::Outcome::Failed(
                "mention task panicked".to_string(),
            ));
            match outcome {
                crate::mentions::Outcome::Ignored(_) => {}
                crate::mentions::Outcome::Rejected(reason) => {
                    tracing::info!(reason = %reason, "mention rejected; continuing webhook handling");
                }
                crate::mentions::Outcome::Launched(thread) => {
                    return format!("mention launched: {thread}");
                }
                crate::mentions::Outcome::Failed(reason) => {
                    return format!("mention failed: {reason}");
                }
            }
        }
    }

    if event != "issues" || !["opened", "reopened", "labeled"].contains(&action.as_str()) {
        return format!("ignored: event={event} action={action}");
    }

    let issue = match payload.get("issue") {
        Some(i) if i.is_object() => i.clone(),
        _ => return "ignored: no issue".to_string(),
    };
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
    if !state.config.repo.is_empty() && repo != state.config.repo {
        return "ignored: repo not configured".to_string();
    }

    let key = format!("{}#{}", repo, number);

    // Atomic reserve: decide whether this delivery may launch.
    {
        let mut issues = state.issues.lock().unwrap();
        if let Some(rec) = issues.get(&key) {
            match rec.status {
                IssueStatus::Completed => {
                    return format!("ignored: already completed {}", key);
                }
                IssueStatus::Running => {
                    return format!("ignored: already running {}", key);
                }
                IssueStatus::Failed if rec.attempts >= max_attempts(&state.config) => {
                    return format!("ignored: retries exhausted {}", key);
                }
                _ => {}
            }
        }
        let mut rec = issues.get(&key).cloned().unwrap_or_default();
        rec.status = IssueStatus::Running;
        rec.attempts = rec.attempts.saturating_add(1);
        rec.last_error = None;
        rec.updated_at = crate::state::now();
        issues.insert(key.clone(), rec);
        let snapshot = issues.clone();
        drop(issues);
        let _ = save_state(&snapshot);
    }

    tracing::info!(issue = %key, "reserved issue for launch");

    // resolve project path
    let projects_base = PathBuf::from(&state.config.projects_base);
    let mut project: Option<PathBuf> = None;
    for r in &state.config.repos {
        if r.github == repo || r.name == repo.rsplit('/').next().unwrap_or("") {
            if !r.path.is_empty() {
                project = Some(PathBuf::from(&r.path));
            }
            break;
        }
    }
    if project.is_none() {
        project = Some(projects_base.join(repo.rsplit('/').next().unwrap_or("")));
    }
    let project = project.unwrap();
    if !project.exists() {
        // transition to failed (retryable) state
        let mut issues = state.issues.lock().unwrap();
        if let Some(rec) = issues.get_mut(&key) {
            rec.status = IssueStatus::Failed;
            rec.last_error = Some("no project path".to_string());
            rec.updated_at = crate::state::now();
        }
        let snapshot = issues.clone();
        drop(issues);
        let _ = save_state(&snapshot);
        return format!("ignored: no project for {}", repo);
    }

    let (existing_thread, existing_pr, attempt) = {
        let issues = state.issues.lock().unwrap();
        let rec = issues.get(&key);
        (
            rec.and_then(|r| r.thread_id.clone()),
            rec.and_then(|r| r.pr_url.clone()),
            rec.map(|r| r.attempts).unwrap_or(1),
        )
    };
    let exec = match crate::execution::create_execution(
        &key,
        &repo,
        number,
        attempt,
        Some("webhook"),
        Some("local"),
        Some(&format!("webhook:{event}:{action}")),
    ) {
        Ok(exec) => exec,
        Err(e) => {
            let mut issues = state.issues.lock().unwrap();
            if let Some(rec) = issues.get_mut(&key) {
                rec.status = IssueStatus::Failed;
                rec.last_error = Some(format!("failed to register execution: {e}"));
                rec.updated_at = crate::state::now();
            }
            let snapshot = issues.clone();
            drop(issues);
            let _ = save_state(&snapshot);
            return format!("failed to register execution: {e}");
        }
    };
    let exec_id = Some(exec.execution_id.clone());
    let issue_for_launch = issue.clone();
    let project_for_launch = project.clone();
    let config_for_launch = state.config.clone();
    let existing_thread_for_launch = existing_thread.clone();
    let exec_for_launch = exec_id.clone();
    let launch_result = tokio::task::spawn_blocking(move || {
        crate::agent::launch_agent_with_execution(crate::agent::AgentLaunchRequest {
            issue: &issue_for_launch,
            project: &project_for_launch,
            config: &config_for_launch,
            prompt: None,
            existing_thread: existing_thread_for_launch.as_deref(),
            attempt,
            execution_id: exec_for_launch.as_deref(),
        })
    })
    .await;
    let pr_url = existing_pr.or_else(|| crate::mentions::discover_pr_url(&repo, number as u64));

    let (thread_id, failure_message) = match launch_result {
        Ok(Ok(id)) => (Some(id), None),
        Ok(Err(e)) => (None, Some(e)),
        Err(e) => (None, Some(format!("launch task failed: {e}"))),
    };

    {
        let mut issues = state.issues.lock().unwrap();
        let rec = issues.entry(key.clone()).or_default();
        if let Some(ref id) = thread_id {
            rec.status = IssueStatus::Completed;
            rec.thread_id = Some(id.clone());
            rec.last_error = None;
            tracing::info!(issue = %key, thread_id = %id, "launch succeeded");
        } else {
            rec.status = IssueStatus::Failed;
            rec.last_error = failure_message.clone();
            tracing::error!(issue = %key, "launch failed");
        }
        if let Some(ref url) = pr_url {
            rec.pr_url = Some(url.clone());
        }
        rec.updated_at = crate::state::now();
        let snapshot = issues.clone();
        drop(issues);
        let _ = save_state(&snapshot);
    }

    if let Some(err) = failure_message {
        if let Some(ref id) = exec_id {
            if crate::execution::is_cancel_requested(id) {
                let _ = crate::execution::mark_cancel_confirmed(id, "cancelled");
            } else {
                let _ = crate::execution::fail_execution(id, &err);
            }
        }
        let assignees = issue_assignee_logins(&issue);
        report_launch_failure(&repo, number, &err, &assignees);
        format!("failed {}", key)
    } else {
        if let Some(ref id) = exec_id {
            let _ =
                crate::execution::complete_execution(id, thread_id.as_deref(), pr_url.as_deref());
        }
        format!("launched {}", key)
    }
}

fn issue_assignee_logins(issue: &Value) -> Vec<String> {
    issue
        .get("assignees")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("login").and_then(|l| l.as_str()))
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn mention_prefix(assignees: &[String]) -> String {
    if assignees.is_empty() {
        String::new()
    } else {
        let mentions = assignees
            .iter()
            .map(|a| format!("@{}", a.trim_start_matches('@')))
            .collect::<Vec<_>>()
            .join(" ");
        format!("{mentions}\n\n")
    }
}

fn report_launch_failure(repo: &str, number: i64, error: &str, assignees: &[String]) {
    let detail = error.chars().take(1000).collect::<String>();
    let mentions = mention_prefix(assignees);
    let body = format!(
        "{}\n{}Factory failed to launch an agent for this issue.\n\n```\n{}\n```",
        crate::mentions::REPORT_MARKER,
        mentions,
        detail
    );
    let path = format!("repos/{repo}/issues/{number}/comments");
    let body_arg = format!("body={body}");
    let output = std::process::Command::new("gh")
        .args(["api", "-X", "POST", &path, "-f", &body_arg])
        .output();
    match output {
        Ok(o) if o.status.success() => {}
        Ok(o) => tracing::warn!(
            repo = %repo,
            "failed to post failure comment: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => tracing::warn!(repo = %repo, "failed to post failure comment: {e}"),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn verify_signature_empty_secret() {
        assert!(verify_signature("", b"hello", ""));
    }

    #[test]
    fn verify_signature_rejects_bad() {
        assert!(!verify_signature("secret", b"hello", "sha256=deadbeef"));
    }

    #[test]
    fn issue_matches_label_labeled() {
        let issue = json!({"labels": []});
        let payload = json!({"label": {"name": "status/ready"}});
        assert!(issue_matches_label(
            "status/ready",
            &issue,
            "labeled",
            &payload
        ));
    }

    #[test]
    fn issue_matches_label_opened() {
        let issue = json!({"labels": [{"name": "status/ready"}, {"name": "ai"}]});
        let payload = json!({});
        assert!(issue_matches_label(
            "status/ready",
            &issue,
            "opened",
            &payload
        ));
        assert!(!issue_matches_label("other", &issue, "opened", &payload));
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

#[cfg(test)]
#[allow(clippy::await_holding_lock)]
mod lifecycle_tests {
    use super::*;
    use crate::config::{Config, RepoEntry};
    use crate::state::{load_state, IssueStatus};
    use serde_json::json;
    use std::sync::Mutex;

    fn make_config(tmp: &std::path::Path) -> Config {
        Config {
            repos: vec![RepoEntry {
                name: "repo".to_string(),
                path: tmp.to_string_lossy().to_string(),
                github: "org/repo".to_string(),
                hook_id: None,
                enabled: true,
            }],
            ..Config::default()
        }
    }

    fn write_script(name: &str, body: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(name);
        std::fs::write(&p, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }

    #[tokio::test]
    async fn failed_launch_allows_retry() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join("factory_lc_failed");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let state_file = std::env::temp_dir().join("factory_lc_failed_state.json");
        let _ = std::fs::remove_file(&state_file);
        std::env::set_var("ISSUE_FACTORY_STATE_FILE", state_file.to_str().unwrap());

        let t3_fail = write_script("t3_fail.sh", "#!/bin/sh\nexit 1\n");
        std::env::set_var("ISSUE_FACTORY_T3", t3_fail.to_str().unwrap());

        let state = AppState {
            config: Arc::new(make_config(&tmp)),
            issues: Arc::new(Mutex::new(load_state())),
        };

        let payload = json!({
            "action": "opened",
            "issue": {"number": 1, "title": "t", "labels": [{"name": "status/ready"}], "url": "u"},
            "repository": {"full_name": "org/repo"}
        });

        let r = handle_payload(state.clone(), "issues".to_string(), payload.clone()).await;
        assert!(r.starts_with("failed"), "{}", r);

        let t3_ok = write_script(
            "t3_ok.sh",
            "#!/bin/sh\necho '{\"threadId\":\"thread-123\"}'\n",
        );
        std::env::set_var("ISSUE_FACTORY_T3", t3_ok.to_str().unwrap());

        let r2 = handle_payload(state.clone(), "issues".to_string(), payload).await;
        assert!(r2.starts_with("launched"), "{}", r2);

        let issues = state.issues.lock().unwrap();
        let rec = issues.get("org/repo#1").unwrap();
        assert_eq!(rec.status, IssueStatus::Completed);
        assert_eq!(rec.thread_id.as_deref(), Some("thread-123"));

        std::env::remove_var("ISSUE_FACTORY_T3");
        std::env::remove_var("ISSUE_FACTORY_STATE_FILE");
    }

    #[tokio::test]
    async fn duplicate_delivery_is_idempotent() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join("factory_lc_dup");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let state_file = std::env::temp_dir().join("factory_lc_dup_state.json");
        let _ = std::fs::remove_file(&state_file);
        std::env::set_var("ISSUE_FACTORY_STATE_FILE", state_file.to_str().unwrap());

        let t3_ok = write_script(
            "t3_ok2.sh",
            "#!/bin/sh\necho '{\"threadId\":\"thread-dup\"}'\n",
        );
        std::env::set_var("ISSUE_FACTORY_T3", t3_ok.to_str().unwrap());

        let state = AppState {
            config: Arc::new(make_config(&tmp)),
            issues: Arc::new(Mutex::new(load_state())),
        };
        let payload = json!({
            "action": "opened",
            "issue": {"number": 2, "title": "t", "labels": [{"name": "status/ready"}], "url": "u"},
            "repository": {"full_name": "org/repo"}
        });

        let r1 = handle_payload(state.clone(), "issues".to_string(), payload.clone()).await;
        assert!(r1.starts_with("launched"), "{}", r1);
        let r2 = handle_payload(state.clone(), "issues".to_string(), payload).await;
        assert!(r2.contains("already"), "{}", r2);

        std::env::remove_var("ISSUE_FACTORY_T3");
        std::env::remove_var("ISSUE_FACTORY_STATE_FILE");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_deliveries_single_launch() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join("factory_lc_conc");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let state_file = std::env::temp_dir().join("factory_lc_conc_state.json");
        let _ = std::fs::remove_file(&state_file);
        std::env::set_var("ISSUE_FACTORY_STATE_FILE", state_file.to_str().unwrap());

        let t3_slow = write_script(
            "t3_slow.sh",
            "#!/bin/sh\nsleep 1\necho '{\"threadId\":\"thread-conc\"}'\n",
        );
        std::env::set_var("ISSUE_FACTORY_T3", t3_slow.to_str().unwrap());

        let state = AppState {
            config: Arc::new(make_config(&tmp)),
            issues: Arc::new(Mutex::new(load_state())),
        };
        let payload = json!({
            "action": "opened",
            "issue": {"number": 3, "title": "t", "labels": [{"name": "status/ready"}], "url": "u"},
            "repository": {"full_name": "org/repo"}
        });

        let h1 = tokio::spawn(handle_payload(
            state.clone(),
            "issues".to_string(),
            payload.clone(),
        ));
        let h2 = tokio::spawn(handle_payload(state.clone(), "issues".to_string(), payload));
        let a = h1.await.unwrap();
        let b = h2.await.unwrap();
        // one launched, one ignored as already running
        let vals = [a, b];
        assert!(vals.iter().any(|v| v.starts_with("launched")), "{:?}", vals);
        assert!(
            vals.iter().any(|v| v.contains("already running")),
            "{:?}",
            vals
        );

        std::env::remove_var("ISSUE_FACTORY_T3");
        std::env::remove_var("ISSUE_FACTORY_STATE_FILE");
    }

    #[tokio::test]
    async fn restart_recovery_marks_running_failed() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let state_file = std::env::temp_dir().join("factory_lc_restart_state.json");
        let _ = std::fs::remove_file(&state_file);
        std::env::set_var("ISSUE_FACTORY_STATE_FILE", state_file.to_str().unwrap());

        // persist a running record
        let mut m = std::collections::HashMap::new();
        m.insert(
            "org/repo#9".to_string(),
            IssueState {
                status: IssueStatus::Running,
                attempts: 1,
                ..IssueState::default()
            },
        );
        crate::state::save_state(&m).unwrap();

        let loaded = load_state();
        assert_eq!(
            loaded.get("org/repo#9").unwrap().status,
            IssueStatus::Failed
        );

        std::env::remove_var("ISSUE_FACTORY_STATE_FILE");
    }

    #[tokio::test]
    async fn issue_comment_mention_launches_via_webhook() {
        let _lock = crate::state::tests::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join("factory_lc_mention_wh");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let state_file = std::env::temp_dir().join("factory_lc_mention_wh_state.json");
        let _ = std::fs::remove_file(&state_file);
        std::env::set_var("ISSUE_FACTORY_STATE_FILE", state_file.to_str().unwrap());

        let t3_ok = write_script(
            "t3_mention_wh.sh",
            "#!/bin/sh\necho '{\"threadId\":\"thread-mention-wh\"}'\n",
        );
        std::env::set_var("ISSUE_FACTORY_T3", t3_ok.to_str().unwrap());

        let mut config = make_config(&tmp);
        config.mentions.enabled = true;
        config.mentions.usernames = vec!["alice".to_string()];
        config.mentions.command = "factory".to_string();

        let state = AppState {
            config: Arc::new(config),
            issues: Arc::new(Mutex::new(load_state())),
        };

        let payload = json!({
            "action": "created",
            "issue": {"number": 7, "title": "Bug", "html_url": "u", "pull_request": null},
            "comment": {
                "id": 1001,
                "body": "@alice factory fix it",
                "updated_at": "2026-01-01T00:00:00Z",
                "html_url": "c",
                "user": {"login": "alice", "type": "User"}
            },
            "repository": {"full_name": "org/repo"}
        });

        let r = handle_payload(state.clone(), "issue_comment".to_string(), payload).await;
        assert!(r.starts_with("mention launched"), "{}", r);

        std::env::remove_var("ISSUE_FACTORY_T3");
        std::env::remove_var("ISSUE_FACTORY_STATE_FILE");
    }
}
