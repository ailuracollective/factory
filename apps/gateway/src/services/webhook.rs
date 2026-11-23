use ohkami::Request;

use super::json_response;
use crate::domain::{auth, github, validation};
use crate::infra;
use crate::repos::{jobs, policies};

pub async fn handle_webhook(req: &Request, env: &worker::Env) -> ohkami::Response {
    let secret = infra::env_str(env, "GITHUB_WEBHOOK_SECRET");
    if secret.is_empty() {
        return json_response(
            &serde_json::json!({"error": "configuration error", "reason": "GITHUB_WEBHOOK_SECRET is not configured"}),
            503,
        );
    }
    let sig = super::header(req, "x-hub-signature-256");
    let event = super::header(req, "x-github-event");

    let body_bytes = req.payload().unwrap_or(&[]);
    if !auth::verify_signature(&secret, body_bytes, &sig) {
        return json_response(&serde_json::json!({"error": "bad signature"}), 401);
    }

    let payload: serde_json::Value = match serde_json::from_slice(body_bytes) {
        Ok(v) => v,
        Err(_) => return json_response(&serde_json::json!({"error": "invalid json"}), 400),
    };

    if event != "issues" {
        return json_response(
            &serde_json::json!({"status": "ignored", "reason": format!("event={}", event)}),
            202,
        );
    }
    let action = payload.get("action").and_then(|v| v.as_str()).unwrap_or("");
    if !["opened", "reopened", "labeled"].contains(&action) {
        return json_response(
            &serde_json::json!({"status": "ignored", "reason": format!("action={}", action)}),
            202,
        );
    }

    let issue = match payload.get("issue") {
        Some(i) if i.is_object() => i,
        _ => {
            return json_response(
                &serde_json::json!({"status": "ignored", "reason": "no issue"}),
                202,
            );
        }
    };
    let number = match issue.get("number").and_then(|v| v.as_i64()) {
        Some(n) => n,
        None => {
            return json_response(
                &serde_json::json!({"status": "ignored", "reason": "bad number"}),
                202,
            );
        }
    };

    let label = "status/ready";
    let matches = if action == "labeled" {
        payload
            .get("label")
            .and_then(|l| l.get("name"))
            .and_then(|n| n.as_str())
            == Some(label)
    } else {
        issue
            .get("labels")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| l.get("name").and_then(|n| n.as_str()))
                    .any(|n| n == label)
            })
            .unwrap_or(false)
    };
    if !matches {
        return json_response(
            &serde_json::json!({"status": "ignored", "reason": "label mismatch"}),
            202,
        );
    }

    let repo = payload
        .get("repository")
        .and_then(|r| r.get("full_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if repo.is_empty() {
        return json_response(
            &serde_json::json!({"status": "ignored", "reason": "no repo"}),
            202,
        );
    }
    if !validation::is_valid_repo_name(&repo) {
        return json_response(
            &serde_json::json!({"error": "forbidden", "reason": "invalid repo"}),
            403,
        );
    }

    let github_token = infra::env_str(env, "GITHUB_TOKEN");
    if github_token.is_empty() {
        return json_response(
            &serde_json::json!({"error": "configuration error", "reason": "GITHUB_TOKEN is not configured"}),
            503,
        );
    }
    if !validation::repo_is_allowed(&infra::env_str(env, "GITHUB_ALLOWED_REPOS"), &repo) {
        return json_response(
            &serde_json::json!({"error": "forbidden", "reason": "repo not allowed"}),
            403,
        );
    }
    if !validation::installation_allowed(&infra::env_str(env, "GITHUB_INSTALLATION_ID"), &payload) {
        return json_response(
            &serde_json::json!({"error": "forbidden", "reason": "installation not allowed"}),
            403,
        );
    }

    let sender = payload
        .get("sender")
        .and_then(|s| s.get("login"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if sender.is_empty() {
        return json_response(
            &serde_json::json!({"error": "forbidden", "reason": "missing actor"}),
            403,
        );
    }

    let conn = match infra::db::connect(env).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if infra::db::ensure_schema(&conn).await.is_err() {
        return json_response(&serde_json::json!({"error": "db unavailable"}), 500);
    }

    let ts = infra::time::now_ts();
    let policy = match policies::get_or_create(&conn, &repo, ts).await {
        Ok(p) => p,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };

    if policy.enabled == 0 {
        infra::audit(&conn, "policy_rejected", &format!("{}: disabled", repo)).await;
        return json_response(
            &serde_json::json!({"error": "forbidden", "reason": "disabled"}),
            403,
        );
    }
    let allowed: Vec<String> = serde_json::from_str(&policy.allowed_users).unwrap_or_default();
    if !allowed.is_empty() && !allowed.contains(&sender) {
        infra::audit(
            &conn,
            "policy_rejected",
            &format!("{}: user not allowed", repo),
        )
        .await;
        return json_response(
            &serde_json::json!({"error": "forbidden", "reason": "user not allowed"}),
            403,
        );
    }

    let conc_count = match jobs::count_concurrent(&conn, &repo).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if conc_count >= policy.max_concurrent {
        infra::audit(
            &conn,
            "policy_rejected",
            &format!("{}: concurrency limit", repo),
        )
        .await;
        return json_response(
            &serde_json::json!({"error": "too_many_requests", "reason": "concurrency limit"}),
            429,
        );
    }

    let day_start = ts - (ts % 86400);
    let daily_count = match jobs::count_daily(&conn, &repo, day_start).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if daily_count >= policy.daily_quota {
        infra::audit(&conn, "policy_rejected", &format!("{}: daily quota", repo)).await;
        return json_response(
            &serde_json::json!({"error": "too_many_requests", "reason": "daily quota"}),
            429,
        );
    }

    let api_base = infra::env_str(env, "GITHUB_API_BASE");
    match infra::http::github_repo_scope(&api_base, &repo, &github_token).await {
        github::RepoCheck::Authorized => {}
        github::RepoCheck::Denied => {
            infra::audit(
                &conn,
                "policy_rejected",
                &format!("{}: repo not authorized", repo),
            )
            .await;
            return json_response(
                &serde_json::json!({"error": "forbidden", "reason": "repo not authorized"}),
                403,
            );
        }
        github::RepoCheck::Unavailable => {
            return json_response(
                &serde_json::json!({"error": "github unavailable", "reason": "repo lookup failed"}),
                503,
            );
        }
    }

    match infra::http::github_permission(&api_base, &repo, &sender, &github_token).await {
        github::PermissionCheck::Allowed => {}
        github::PermissionCheck::Insufficient => {
            infra::audit(
                &conn,
                "policy_rejected",
                &format!("{}: insufficient permission", repo),
            )
            .await;
            return json_response(
                &serde_json::json!({"error": "forbidden", "reason": "insufficient permission"}),
                403,
            );
        }
        github::PermissionCheck::Unavailable => {
            return json_response(
                &serde_json::json!({"error": "github unavailable", "reason": "permission lookup failed"}),
                503,
            );
        }
    }

    let id = format!("{}#{}", repo, number);
    let title = issue.get("title").and_then(|v| v.as_str());
    let url = issue.get("url").and_then(|v| v.as_str());
    if jobs::create(&conn, &id, &repo, number, title, url, ts)
        .await
        .is_err()
    {
        return json_response(&serde_json::json!({"error": "db unavailable"}), 500);
    }

    infra::audit(&conn, "webhook_accepted", &id).await;
    json_response(&serde_json::json!({"status": "accepted", "job": id}), 202)
}
