// Native builds only exist to run unit tests; runtime helpers are wasm-only.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

#[cfg(target_arch = "wasm32")]
use js_sys::Date;
#[cfg(target_arch = "wasm32")]
use serde_json::json;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;
#[cfg(target_arch = "wasm32")]
use worker::*;

pub const MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        1,
        "0001_initial",
        include_str!("../migrations/0001_initial.sql"),
    ),
    (
        2,
        "0002_policies",
        include_str!("../migrations/0002_policies.sql"),
    ),
    (
        3,
        "0003_runner_tokens",
        include_str!("../migrations/0003_runner_tokens.sql"),
    ),
];

#[cfg(target_arch = "wasm32")]
fn now_ts() -> i64 {
    (Date::now() / 1000.0) as i64
}

#[cfg(target_arch = "wasm32")]
fn env_str(env: &Env, key: &str) -> String {
    if let Ok(s) = env.secret(key) {
        return s.to_string();
    }
    if let Ok(v) = env.var(key) {
        return v.to_string();
    }
    String::new()
}

#[cfg(target_arch = "wasm32")]
fn db(env: &Env) -> Result<D1Database> {
    env.d1("DB")
}

#[cfg(target_arch = "wasm32")]
async fn apply_migrations(db: &D1Database) -> Result<()> {
    db.exec(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at INTEGER NOT NULL
        );",
    )
    .await?;
    for (version, name, sql) in MIGRATIONS {
        let stmt = db.prepare("SELECT version FROM schema_migrations WHERE version=?1");
        let applied = match stmt.bind(&[js_sys::Number::from(*version as f64).into()]) {
            Ok(s) => s
                .first::<serde_json::Value>(None)
                .await
                .ok()
                .flatten()
                .is_some(),
            Err(_) => false,
        };
        if applied {
            continue;
        }
        db.exec(sql).await?;
        let ins = db.prepare(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
        );
        if let Ok(s) = ins.bind(&[
            js_sys::Number::from(*version as f64).into(),
            js_sys::JsString::from(*name).into(),
            js_sys::Number::from(now_ts() as f64).into(),
        ]) {
            let _ = s.run().await;
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn ensure_schema(db: &D1Database) -> Result<()> {
    apply_migrations(db).await
}

#[cfg(target_arch = "wasm32")]
async fn audit(db: &D1Database, event: &str, detail: &str) {
    let stmt = db.prepare("INSERT INTO audit (ts, event, detail) VALUES (?1, ?2, ?3)");
    if let Ok(s) = stmt.bind(&[
        js_sys::Number::from(now_ts() as f64).into(),
        js_sys::JsString::from(event).into(),
        js_sys::JsString::from(detail).into(),
    ]) {
        let _ = s.run().await;
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[allow(dead_code)]
fn verify_signature(secret: &str, body: &[u8], header: &str) -> bool {
    // Fail closed: an unset secret must never authenticate a webhook.
    if secret.is_empty() {
        return false;
    }
    if !header.starts_with("sha256=") {
        return false;
    }
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let expected_hex = &header[7..];
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(body);
    let result = mac.finalize().into_bytes();
    let expected = hex::decode(expected_hex).unwrap_or_default();
    constant_time_eq(&expected, &result)
}

#[derive(Debug, PartialEq, Eq)]
enum AuthDecision {
    Allowed,
    MissingConfig,
    Denied,
}

fn check_secret(configured: &str, provided: &str) -> AuthDecision {
    if configured.is_empty() {
        return AuthDecision::MissingConfig;
    }
    if provided.is_empty() {
        return AuthDecision::Denied;
    }
    if constant_time_eq(configured.as_bytes(), provided.as_bytes()) {
        AuthDecision::Allowed
    } else {
        AuthDecision::Denied
    }
}

fn hash_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn format_runner_token(bytes: &[u8]) -> String {
    format!("frt_{}", hex::encode(bytes))
}

#[cfg(target_arch = "wasm32")]
fn random_runner_token() -> Option<String> {
    use wasm_bindgen::JsCast;
    let global = js_sys::global();
    let crypto_val = js_sys::Reflect::get(&global, &js_sys::JsString::from("crypto")).ok()?;
    let crypto: web_sys::Crypto = crypto_val.dyn_into().ok()?;
    let mut bytes = [0u8; 32];
    crypto.get_random_values_with_u8_array(&mut bytes).ok()?;
    Some(format_runner_token(&bytes))
}

fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 100
        && !segment.starts_with('.')
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn is_valid_repo_name(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or("");
    let name = parts.next().unwrap_or("");
    parts.next().is_none() && is_valid_segment(owner) && is_valid_segment(name)
}

fn is_valid_runner_id(runner_id: &str) -> bool {
    !runner_id.is_empty()
        && runner_id.len() <= 128
        && runner_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
}

fn repo_is_allowed(configured: &str, repo: &str) -> bool {
    let allowed: Vec<&str> = configured
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    // An empty allowlist defers repository scope to the GitHub token itself.
    allowed.is_empty() || allowed.iter().any(|r| r.eq_ignore_ascii_case(repo))
}

fn installation_allowed(configured: &str, payload: &serde_json::Value) -> bool {
    let configured = configured.trim();
    if configured.is_empty() {
        return true;
    }
    let expected: i64 = match configured.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    payload
        .get("installation")
        .and_then(|i| i.get("id"))
        .and_then(|v| v.as_i64())
        == Some(expected)
}

#[derive(Debug, PartialEq, Eq)]
enum PermissionCheck {
    Allowed,
    Insufficient,
    Unavailable,
}

fn evaluate_permission(status: u16, body: &str) -> PermissionCheck {
    // 404 means the actor is not a collaborator; anything else non-200 is an API failure.
    if status == 404 {
        return PermissionCheck::Insufficient;
    }
    if status != 200 {
        return PermissionCheck::Unavailable;
    }
    let value: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return PermissionCheck::Unavailable,
    };
    match value.get("permission").and_then(|p| p.as_str()) {
        Some("admin" | "maintain" | "write") => PermissionCheck::Allowed,
        // Unknown or insufficient permissions always deny.
        Some(_) => PermissionCheck::Insufficient,
        None => PermissionCheck::Insufficient,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum RepoCheck {
    Authorized,
    Denied,
    Unavailable,
}

fn evaluate_repo(status: u16, body: &str, expected: &str) -> RepoCheck {
    if status == 403 || status == 404 {
        return RepoCheck::Denied;
    }
    if status != 200 {
        return RepoCheck::Unavailable;
    }
    let value: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return RepoCheck::Unavailable,
    };
    match value.get("full_name").and_then(|v| v.as_str()) {
        Some(name) if name.eq_ignore_ascii_case(expected) => RepoCheck::Authorized,
        _ => RepoCheck::Denied,
    }
}

#[cfg(target_arch = "wasm32")]
fn json_response(value: &serde_json::Value, status: u16) -> Result<Response> {
    let mut r = Response::from_json(value)?;
    r = r.with_status(status);
    Ok(r)
}

#[cfg(target_arch = "wasm32")]
fn header(req: &Request, name: &str) -> String {
    req.headers().get(name).ok().flatten().unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
fn guard(req: &Request, env: &Env, header_name: &str, env_key: &str) -> Result<Option<Response>> {
    let decision = check_secret(&env_str(env, env_key), &header(req, header_name));
    match decision {
        AuthDecision::Allowed => Ok(None),
        AuthDecision::MissingConfig => Ok(Some(json_response(
            &json!({"error": "configuration error", "reason": format!("{} is not configured", env_key)}),
            503,
        )?)),
        AuthDecision::Denied => Ok(Some(json_response(&json!({"error": "unauthorized"}), 401)?)),
    }
}

#[cfg(target_arch = "wasm32")]
async fn http_get(url: &str, token: &str) -> Result<(u16, String)> {
    use wasm_bindgen::JsCast;
    let global = js_sys::global();
    let fetch_val = js_sys::Reflect::get(&global, &js_sys::JsString::from("fetch"))
        .map_err(|_| worker::Error::RustError("no fetch".into()))?;
    let fetch_fn: js_sys::Function = fetch_val
        .dyn_into()
        .map_err(|_| worker::Error::RustError("fetch not fn".into()))?;
    let init = js_sys::Object::new();
    let headers = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &headers,
        &js_sys::JsString::from("Authorization"),
        &js_sys::JsString::from(format!("Bearer {}", token)),
    );
    let _ = js_sys::Reflect::set(
        &headers,
        &js_sys::JsString::from("Accept"),
        &js_sys::JsString::from("application/vnd.github+json"),
    );
    let _ = js_sys::Reflect::set(
        &headers,
        &js_sys::JsString::from("User-Agent"),
        &js_sys::JsString::from("ailura-factory-gateway"),
    );
    let _ = js_sys::Reflect::set(&init, &js_sys::JsString::from("headers"), &headers);
    let promise = fetch_fn
        .call2(&global, &js_sys::JsString::from(url), &init)
        .map_err(|_| worker::Error::RustError("fetch call failed".into()))?;
    let promise: js_sys::Promise = promise
        .dyn_into()
        .map_err(|_| worker::Error::RustError("not promise".into()))?;
    let resp_val = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|_| worker::Error::RustError("fetch await failed".into()))?;
    let resp: web_sys::Response = resp_val
        .dyn_into()
        .map_err(|_| worker::Error::RustError("not response".into()))?;
    let status = resp.status();
    let text_promise = resp
        .text()
        .map_err(|_| worker::Error::RustError("no text".into()))?;
    let text_val = wasm_bindgen_futures::JsFuture::from(text_promise)
        .await
        .map_err(|_| worker::Error::RustError("text await failed".into()))?;
    let text = text_val.as_string().unwrap_or_default();
    Ok((status, text))
}

#[cfg(target_arch = "wasm32")]
async fn github_repo_scope(repo: &str, token: &str) -> RepoCheck {
    let url = format!("https://api.github.com/repos/{}", repo);
    match http_get(&url, token).await {
        Ok((status, body)) => evaluate_repo(status, &body, repo),
        Err(_) => RepoCheck::Unavailable,
    }
}

#[cfg(target_arch = "wasm32")]
async fn github_permission(repo: &str, user: &str, token: &str) -> PermissionCheck {
    let url = format!(
        "https://api.github.com/repos/{}/collaborators/{}/permission",
        repo, user
    );
    match http_get(&url, token).await {
        Ok((status, body)) => evaluate_permission(status, &body),
        Err(_) => PermissionCheck::Unavailable,
    }
}

#[cfg(target_arch = "wasm32")]
fn decode_id(raw: &str) -> String {
    js_sys::decode_uri_component(raw)
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| raw.to_string())
}

#[cfg(target_arch = "wasm32")]
#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let router = Router::new();
    router
        .get("/health", |_req, _ctx| Response::ok("ok"))
        .post_async("/webhooks/github", |mut req, ctx| async move {
            handle_webhook(&mut req, &ctx.env).await
        })
        .post_async("/runners/register", |mut req, ctx| async move {
            handle_register_runner(&mut req, &ctx.env).await
        })
        .post_async("/jobs/claim", |mut req, ctx| async move {
            handle_claim(&mut req, &ctx.env).await
        })
        .post_async("/jobs/:id/heartbeat", |mut req, ctx| async move {
            let id = decode_id(&ctx.param("id").cloned().unwrap_or_default());
            handle_heartbeat(&mut req, &ctx.env, &id).await
        })
        .post_async("/jobs/:id/complete", |mut req, ctx| async move {
            let id = decode_id(&ctx.param("id").cloned().unwrap_or_default());
            handle_complete(&mut req, &ctx.env, &id).await
        })
        .post_async("/jobs/:id/fail", |mut req, ctx| async move {
            let id = decode_id(&ctx.param("id").cloned().unwrap_or_default());
            handle_fail(&mut req, &ctx.env, &id).await
        })
        .get_async("/admin/jobs", |req, ctx| async move {
            handle_admin_jobs(&req, &ctx.env).await
        })
        .post_async("/admin/jobs/:id/cancel", |req, ctx| async move {
            let id = decode_id(&ctx.param("id").cloned().unwrap_or_default());
            handle_cancel(&req, &ctx.env, &id).await
        })
        .post_async("/admin/runners/:id/revoke", |req, ctx| async move {
            let id = decode_id(&ctx.param("id").cloned().unwrap_or_default());
            handle_revoke_runner(&req, &ctx.env, &id).await
        })
        .post_async("/admin/runners/:id/rotate", |req, ctx| async move {
            let id = decode_id(&ctx.param("id").cloned().unwrap_or_default());
            handle_rotate_runner(&req, &ctx.env, &id).await
        })
        .run(req, env)
        .await
}

#[cfg(target_arch = "wasm32")]
async fn handle_webhook(req: &mut Request, env: &Env) -> Result<Response> {
    let secret = env_str(env, "GITHUB_WEBHOOK_SECRET");
    if secret.is_empty() {
        return json_response(
            &json!({"error": "configuration error", "reason": "GITHUB_WEBHOOK_SECRET is not configured"}),
            503,
        );
    }
    let sig = header(req, "x-hub-signature-256");
    let event = header(req, "x-github-event");

    let body = req.text().await.unwrap_or_default();
    if !verify_signature(&secret, body.as_bytes(), &sig) {
        return json_response(&json!({"error": "bad signature"}), 401);
    }

    let payload: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return json_response(&json!({"error": "invalid json"}), 400),
    };

    if event != "issues" {
        return json_response(
            &json!({"status": "ignored", "reason": format!("event={}", event)}),
            202,
        );
    }

    let action = payload.get("action").and_then(|v| v.as_str()).unwrap_or("");
    if !["opened", "reopened", "labeled"].contains(&action) {
        return json_response(
            &json!({"status": "ignored", "reason": format!("action={}", action)}),
            202,
        );
    }

    let issue = match payload.get("issue") {
        Some(i) if i.is_object() => i,
        _ => return json_response(&json!({"status": "ignored", "reason": "no issue"}), 202),
    };
    let number = match issue.get("number").and_then(|v| v.as_i64()) {
        Some(n) => n,
        None => return json_response(&json!({"status": "ignored", "reason": "bad number"}), 202),
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
            &json!({"status": "ignored", "reason": "label mismatch"}),
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
        return json_response(&json!({"status": "ignored", "reason": "no repo"}), 202);
    }
    if !is_valid_repo_name(&repo) {
        return json_response(
            &json!({"error": "forbidden", "reason": "invalid repo"}),
            403,
        );
    }

    // GitHub authorization must be configured and must authorize this installation/repo.
    let github_token = env_str(env, "GITHUB_TOKEN");
    if github_token.is_empty() {
        return json_response(
            &json!({"error": "configuration error", "reason": "GITHUB_TOKEN is not configured"}),
            503,
        );
    }
    if !repo_is_allowed(&env_str(env, "GITHUB_ALLOWED_REPOS"), &repo) {
        return json_response(
            &json!({"error": "forbidden", "reason": "repo not allowed"}),
            403,
        );
    }
    if !installation_allowed(&env_str(env, "GITHUB_INSTALLATION_ID"), &payload) {
        return json_response(
            &json!({"error": "forbidden", "reason": "installation not allowed"}),
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
            &json!({"error": "forbidden", "reason": "missing actor"}),
            403,
        );
    }

    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if ensure_schema(&db).await.is_err() {
        return json_response(&json!({"error": "db unavailable"}), 500);
    }

    let ts = now_ts();

    // load or create policy
    let policy_stmt = db.prepare(
        "SELECT enabled, allowed_users, max_concurrent, daily_quota FROM policies WHERE repo=?1",
    );
    let policy_result = match policy_stmt.bind(&[js_sys::JsString::from(repo.as_str()).into()]) {
        Ok(s) => s.first::<serde_json::Value>(None).await,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    let policy_row = match policy_result {
        Ok(row) => row,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    let (enabled, allowed_users, max_concurrent, daily_quota) = if let Some(row) = policy_row {
        (
            row.get("enabled").and_then(|v| v.as_i64()).unwrap_or(1),
            row.get("allowed_users")
                .and_then(|v| v.as_str())
                .unwrap_or("[]")
                .to_string(),
            row.get("max_concurrent")
                .and_then(|v| v.as_i64())
                .unwrap_or(2),
            row.get("daily_quota")
                .and_then(|v| v.as_i64())
                .unwrap_or(20),
        )
    } else {
        let ins = db.prepare(
            "INSERT INTO policies (repo, enabled, allowed_users, max_concurrent, daily_quota, updated_at) VALUES (?1,1,'[]',2,20,?2)",
        );
        let created = match ins.bind(&[
            js_sys::JsString::from(repo.as_str()).into(),
            js_sys::Number::from(ts as f64).into(),
        ]) {
            Ok(s) => s.run().await,
            Err(_) => Err(worker::Error::RustError("policy bind failed".into())),
        };
        if created.is_err() {
            return json_response(&json!({"error": "db unavailable"}), 500);
        }
        (1, "[]".to_string(), 2, 20)
    };

    if enabled == 0 {
        audit(&db, "policy_rejected", &format!("{}: disabled", repo)).await;
        return json_response(&json!({"error": "forbidden", "reason": "disabled"}), 403);
    }
    let allowed: Vec<String> = serde_json::from_str(&allowed_users).unwrap_or_default();
    if !allowed.is_empty() && !allowed.contains(&sender) {
        audit(
            &db,
            "policy_rejected",
            &format!("{}: user not allowed", repo),
        )
        .await;
        return json_response(
            &json!({"error": "forbidden", "reason": "user not allowed"}),
            403,
        );
    }

    // concurrency count
    let conc_stmt = db.prepare(
        "SELECT COUNT(*) as c FROM jobs WHERE repo=?1 AND status IN ('pending','claimed','running')",
    );
    let conc_count = match conc_stmt.bind(&[js_sys::JsString::from(repo.as_str()).into()]) {
        Ok(s) => match s.first::<serde_json::Value>(None).await {
            Ok(row) => row
                .and_then(|v| v.get("c").and_then(|x| x.as_i64()))
                .unwrap_or(0),
            Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
        },
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if conc_count >= max_concurrent {
        audit(
            &db,
            "policy_rejected",
            &format!("{}: concurrency limit", repo),
        )
        .await;
        return json_response(
            &json!({"error": "too_many_requests", "reason": "concurrency limit"}),
            429,
        );
    }

    // daily quota
    let day_start = ts - (ts % 86400);
    let quota_stmt =
        db.prepare("SELECT COUNT(*) as c FROM jobs WHERE repo=?1 AND created_at >= ?2");
    let daily_count = match quota_stmt.bind(&[
        js_sys::JsString::from(repo.as_str()).into(),
        js_sys::Number::from(day_start as f64).into(),
    ]) {
        Ok(s) => match s.first::<serde_json::Value>(None).await {
            Ok(row) => row
                .and_then(|v| v.get("c").and_then(|x| x.as_i64()))
                .unwrap_or(0),
            Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
        },
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if daily_count >= daily_quota {
        audit(&db, "policy_rejected", &format!("{}: daily quota", repo)).await;
        return json_response(
            &json!({"error": "too_many_requests", "reason": "daily quota"}),
            429,
        );
    }

    // Verify the repo is reachable with the configured credential before trusting the actor.
    match github_repo_scope(&repo, &github_token).await {
        RepoCheck::Authorized => {}
        RepoCheck::Denied => {
            audit(
                &db,
                "policy_rejected",
                &format!("{}: repo not authorized", repo),
            )
            .await;
            return json_response(
                &json!({"error": "forbidden", "reason": "repo not authorized"}),
                403,
            );
        }
        RepoCheck::Unavailable => {
            return json_response(
                &json!({"error": "github unavailable", "reason": "repo lookup failed"}),
                503,
            );
        }
    }

    // Permission checks are mandatory; an empty allowed_users list is not a substitute.
    match github_permission(&repo, &sender, &github_token).await {
        PermissionCheck::Allowed => {}
        PermissionCheck::Insufficient => {
            audit(
                &db,
                "policy_rejected",
                &format!("{}: insufficient permission", repo),
            )
            .await;
            return json_response(
                &json!({"error": "forbidden", "reason": "insufficient permission"}),
                403,
            );
        }
        PermissionCheck::Unavailable => {
            return json_response(
                &json!({"error": "github unavailable", "reason": "permission lookup failed"}),
                503,
            );
        }
    }

    let id = format!("{}#{}", repo, number);
    let title = issue
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let url = issue
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let stmt = db.prepare(
        "INSERT OR IGNORE INTO jobs (id, repo, issue_number, title, url, status, attempts, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 0, ?6, ?6)",
    );
    let inserted = match stmt.bind(&[
        js_sys::JsString::from(id.as_str()).into(),
        js_sys::JsString::from(repo.as_str()).into(),
        js_sys::Number::from(number as f64).into(),
        title
            .as_deref()
            .map(|t| js_sys::JsString::from(t).into())
            .unwrap_or(JsValue::null()),
        url.as_deref()
            .map(|u| js_sys::JsString::from(u).into())
            .unwrap_or(JsValue::null()),
        js_sys::Number::from(ts as f64).into(),
    ]) {
        Ok(s) => s.run().await,
        Err(_) => Err(worker::Error::RustError("job bind failed".into())),
    };
    if inserted.is_err() {
        return json_response(&json!({"error": "db unavailable"}), 500);
    }

    audit(&db, "webhook_accepted", &id).await;
    json_response(&json!({"status": "accepted", "job": id}), 202)
}

#[cfg(target_arch = "wasm32")]
#[derive(serde::Deserialize)]
struct RegisterReq {
    runner_id: String,
}

#[cfg(target_arch = "wasm32")]
async fn handle_register_runner(req: &mut Request, env: &Env) -> Result<Response> {
    if let Some(resp) = guard(req, env, "x-bootstrap-secret", "RUNNER_BOOTSTRAP_SECRET")? {
        return Ok(resp);
    }
    let body: RegisterReq = match req.json().await {
        Ok(v) => v,
        Err(_) => return json_response(&json!({"error": "invalid json"}), 400),
    };
    if !is_valid_runner_id(&body.runner_id) {
        return json_response(&json!({"error": "invalid runner_id"}), 400);
    }
    let token = match random_runner_token() {
        Some(t) => t,
        None => return json_response(&json!({"error": "token generation failed"}), 500),
    };
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if ensure_schema(&db).await.is_err() {
        return json_response(&json!({"error": "db unavailable"}), 500);
    }
    let ts = now_ts();
    let stmt = db.prepare(
        "INSERT INTO runners (id, token_hash, created_at, rotated_at, revoked_at) VALUES (?1, ?2, ?3, NULL, NULL) ON CONFLICT(id) DO UPDATE SET token_hash=?2, rotated_at=?3, revoked_at=NULL",
    );
    let stored = match stmt.bind(&[
        js_sys::JsString::from(body.runner_id.as_str()).into(),
        js_sys::JsString::from(hash_token(&token).as_str()).into(),
        js_sys::Number::from(ts as f64).into(),
    ]) {
        Ok(s) => s.run().await,
        Err(_) => Err(worker::Error::RustError("runner bind failed".into())),
    };
    if stored.is_err() {
        return json_response(&json!({"error": "db unavailable"}), 500);
    }
    audit(&db, "runner_registered", &body.runner_id).await;
    json_response(&json!({"runner_id": body.runner_id, "token": token}), 200)
}

#[cfg(target_arch = "wasm32")]
#[derive(serde::Deserialize)]
struct ClaimReq {
    runner_id: String,
    token: String,
}

#[cfg(target_arch = "wasm32")]
async fn verify_runner(env: &Env, runner_id: &str, token: &str) -> bool {
    if runner_id.is_empty() || token.is_empty() {
        return false;
    }
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return false,
    };
    if ensure_schema(&db).await.is_err() {
        return false;
    }
    let stmt = db.prepare("SELECT token_hash FROM runners WHERE id=?1 AND revoked_at IS NULL");
    if let Ok(s) = stmt.bind(&[js_sys::JsString::from(runner_id).into()]) {
        if let Ok(Some(v)) = s.first::<serde_json::Value>(None).await {
            if let Some(stored) = v.get("token_hash").and_then(|t| t.as_str()) {
                return constant_time_eq(stored.as_bytes(), hash_token(token).as_bytes());
            }
        }
    }
    false
}

#[cfg(target_arch = "wasm32")]
async fn handle_claim(req: &mut Request, env: &Env) -> Result<Response> {
    let body: ClaimReq = match req.json().await {
        Ok(v) => v,
        Err(_) => return json_response(&json!({"error": "invalid json"}), 400),
    };
    if !verify_runner(env, &body.runner_id, &body.token).await {
        return json_response(&json!({"error": "unauthorized"}), 401);
    }
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if ensure_schema(&db).await.is_err() {
        return json_response(&json!({"error": "db unavailable"}), 500);
    }

    let stmt = db.prepare("SELECT id FROM jobs WHERE status='pending' ORDER BY created_at LIMIT 1");
    let pending_id: Option<String> = match stmt.first::<serde_json::Value>(None).await {
        Ok(Some(v)) => v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()),
        Ok(None) => None,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    let Some(jid) = pending_id else {
        return json_response(&json!({"status": "no_job"}), 204);
    };

    let ts = now_ts();
    let lease = ts + 300;
    let upd = db.prepare(
        "UPDATE jobs SET status='claimed', runner_id=?1, lease_until=?2, attempts=attempts+1, updated_at=?3 WHERE id=?4 AND status='pending'",
    );
    let changes = match upd.bind(&[
        js_sys::JsString::from(body.runner_id.as_str()).into(),
        js_sys::Number::from(lease as f64).into(),
        js_sys::Number::from(ts as f64).into(),
        js_sys::JsString::from(jid.as_str()).into(),
    ]) {
        Ok(s) => match s.run().await {
            Ok(r) => r.meta().ok().flatten().and_then(|m| m.changes).unwrap_or(0),
            Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
        },
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if changes == 0 {
        return json_response(&json!({"status": "no_job"}), 204);
    }

    let stmt = db.prepare("SELECT * FROM jobs WHERE id=?1");
    let row: Option<serde_json::Value> =
        match stmt.bind(&[js_sys::JsString::from(jid.as_str()).into()]) {
            Ok(s) => match s.first::<serde_json::Value>(None).await {
                Ok(row) => row,
                Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
            },
            Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
        };
    match row {
        Some(j) => json_response(&j, 200),
        None => json_response(&json!({"status": "no_job"}), 204),
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(serde::Deserialize)]
struct HeartbeatReq {
    runner_id: String,
    token: String,
    status: Option<String>,
}

#[cfg(target_arch = "wasm32")]
async fn handle_heartbeat(req: &mut Request, env: &Env, id: &str) -> Result<Response> {
    let body: HeartbeatReq = match req.json().await {
        Ok(v) => v,
        Err(_) => return json_response(&json!({"error": "invalid json"}), 400),
    };
    if !verify_runner(env, &body.runner_id, &body.token).await {
        return json_response(&json!({"error": "unauthorized"}), 401);
    }
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    let ts = now_ts();
    let lease = ts + 300;
    let status = body.status.unwrap_or_else(|| "running".to_string());
    let stmt = db.prepare(
        "UPDATE jobs SET status=?1, lease_until=?2, updated_at=?3 WHERE id=?4 AND runner_id=?5",
    );
    let changes = match stmt.bind(&[
        js_sys::JsString::from(status.as_str()).into(),
        js_sys::Number::from(lease as f64).into(),
        js_sys::Number::from(ts as f64).into(),
        js_sys::JsString::from(id).into(),
        js_sys::JsString::from(body.runner_id.as_str()).into(),
    ]) {
        Ok(s) => match s.run().await {
            Ok(r) => r.meta().ok().flatten().and_then(|m| m.changes).unwrap_or(0),
            Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
        },
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if changes == 0 {
        return json_response(&json!({"error": "job not found or not owned"}), 404);
    }
    json_response(&json!({"status": "ok"}), 200)
}

#[cfg(target_arch = "wasm32")]
#[derive(serde::Deserialize)]
struct CompleteReq {
    runner_id: String,
    token: String,
    thread_id: Option<String>,
}

#[cfg(target_arch = "wasm32")]
async fn handle_complete(req: &mut Request, env: &Env, id: &str) -> Result<Response> {
    let body: CompleteReq = match req.json().await {
        Ok(v) => v,
        Err(_) => return json_response(&json!({"error": "invalid json"}), 400),
    };
    if !verify_runner(env, &body.runner_id, &body.token).await {
        return json_response(&json!({"error": "unauthorized"}), 401);
    }
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    let ts = now_ts();
    let stmt = db.prepare(
        "UPDATE jobs SET status='completed', thread_id=?1, updated_at=?2, last_error=NULL WHERE id=?3 AND runner_id=?4",
    );
    let changes = match stmt.bind(&[
        body.thread_id
            .as_deref()
            .map(|t| js_sys::JsString::from(t).into())
            .unwrap_or(JsValue::null()),
        js_sys::Number::from(ts as f64).into(),
        js_sys::JsString::from(id).into(),
        js_sys::JsString::from(body.runner_id.as_str()).into(),
    ]) {
        Ok(s) => match s.run().await {
            Ok(r) => r.meta().ok().flatten().and_then(|m| m.changes).unwrap_or(0),
            Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
        },
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if changes == 0 {
        return json_response(&json!({"error": "job not found or not owned"}), 404);
    }
    audit(&db, "job_completed", id).await;
    json_response(&json!({"status": "ok"}), 200)
}

#[cfg(target_arch = "wasm32")]
#[derive(serde::Deserialize)]
struct FailReq {
    runner_id: String,
    token: String,
    error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
async fn handle_fail(req: &mut Request, env: &Env, id: &str) -> Result<Response> {
    let body: FailReq = match req.json().await {
        Ok(v) => v,
        Err(_) => return json_response(&json!({"error": "invalid json"}), 400),
    };
    if !verify_runner(env, &body.runner_id, &body.token).await {
        return json_response(&json!({"error": "unauthorized"}), 401);
    }
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    let ts = now_ts();
    let err = body.error.unwrap_or_else(|| "unknown error".to_string());
    let stmt = db.prepare(
        "UPDATE jobs SET status='failed', last_error=?1, updated_at=?2 WHERE id=?3 AND runner_id=?4",
    );
    let changes = match stmt.bind(&[
        js_sys::JsString::from(err.as_str()).into(),
        js_sys::Number::from(ts as f64).into(),
        js_sys::JsString::from(id).into(),
        js_sys::JsString::from(body.runner_id.as_str()).into(),
    ]) {
        Ok(s) => match s.run().await {
            Ok(r) => r.meta().ok().flatten().and_then(|m| m.changes).unwrap_or(0),
            Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
        },
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if changes == 0 {
        return json_response(&json!({"error": "job not found or not owned"}), 404);
    }
    audit(&db, "job_failed", id).await;
    json_response(&json!({"status": "ok"}), 200)
}

#[cfg(target_arch = "wasm32")]
async fn handle_admin_jobs(req: &Request, env: &Env) -> Result<Response> {
    if let Some(resp) = guard(req, env, "x-admin-token", "ADMIN_TOKEN")? {
        return Ok(resp);
    }
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if ensure_schema(&db).await.is_err() {
        return json_response(&json!({"error": "db unavailable"}), 500);
    }
    let stmt = db.prepare("SELECT * FROM jobs ORDER BY created_at DESC LIMIT 100");
    let rows: Vec<serde_json::Value> = match stmt.all().await {
        Ok(r) => match r.results() {
            Ok(rows) => rows,
            Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
        },
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    json_response(&json!({"jobs": rows}), 200)
}

#[cfg(target_arch = "wasm32")]
async fn handle_cancel(req: &Request, env: &Env, id: &str) -> Result<Response> {
    if let Some(resp) = guard(req, env, "x-admin-token", "ADMIN_TOKEN")? {
        return Ok(resp);
    }
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if ensure_schema(&db).await.is_err() {
        return json_response(&json!({"error": "db unavailable"}), 500);
    }
    let ts = now_ts();
    let stmt = db.prepare("UPDATE jobs SET status='cancelled', updated_at=?1 WHERE id=?2");
    let changes = match stmt.bind(&[
        js_sys::Number::from(ts as f64).into(),
        js_sys::JsString::from(id).into(),
    ]) {
        Ok(s) => match s.run().await {
            Ok(r) => r.meta().ok().flatten().and_then(|m| m.changes).unwrap_or(0),
            Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
        },
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if changes == 0 {
        return json_response(&json!({"error": "job not found"}), 404);
    }
    audit(&db, "job_cancelled", id).await;
    json_response(&json!({"status": "ok"}), 200)
}

#[cfg(target_arch = "wasm32")]
async fn handle_revoke_runner(req: &Request, env: &Env, id: &str) -> Result<Response> {
    if let Some(resp) = guard(req, env, "x-admin-token", "ADMIN_TOKEN")? {
        return Ok(resp);
    }
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if ensure_schema(&db).await.is_err() {
        return json_response(&json!({"error": "db unavailable"}), 500);
    }
    let ts = now_ts();
    let stmt = db.prepare("UPDATE runners SET revoked_at=?1 WHERE id=?2");
    let changes = match stmt.bind(&[
        js_sys::Number::from(ts as f64).into(),
        js_sys::JsString::from(id).into(),
    ]) {
        Ok(s) => match s.run().await {
            Ok(r) => r.meta().ok().flatten().and_then(|m| m.changes).unwrap_or(0),
            Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
        },
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if changes == 0 {
        return json_response(&json!({"error": "runner not found"}), 404);
    }
    audit(&db, "runner_revoked", id).await;
    json_response(&json!({"status": "revoked", "runner_id": id}), 200)
}

#[cfg(target_arch = "wasm32")]
async fn handle_rotate_runner(req: &Request, env: &Env, id: &str) -> Result<Response> {
    if let Some(resp) = guard(req, env, "x-admin-token", "ADMIN_TOKEN")? {
        return Ok(resp);
    }
    let token = match random_runner_token() {
        Some(t) => t,
        None => return json_response(&json!({"error": "token generation failed"}), 500),
    };
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if ensure_schema(&db).await.is_err() {
        return json_response(&json!({"error": "db unavailable"}), 500);
    }
    let ts = now_ts();
    let stmt =
        db.prepare("UPDATE runners SET token_hash=?1, rotated_at=?2, revoked_at=NULL WHERE id=?3");
    let changes = match stmt.bind(&[
        js_sys::JsString::from(hash_token(&token).as_str()).into(),
        js_sys::Number::from(ts as f64).into(),
        js_sys::JsString::from(id).into(),
    ]) {
        Ok(s) => match s.run().await {
            Ok(r) => r.meta().ok().flatten().and_then(|m| m.changes).unwrap_or(0),
            Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
        },
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    if changes == 0 {
        return json_response(&json!({"error": "runner not found"}), 404);
    }
    audit(&db, "runner_rotated", id).await;
    json_response(
        &json!({"status": "rotated", "runner_id": id, "token": token}),
        200,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_sorted_and_unique() {
        let versions: Vec<i64> = MIGRATIONS.iter().map(|m| m.0).collect();
        let mut sorted = versions.clone();
        sorted.sort();
        assert_eq!(versions, sorted, "migrations must be sorted by version");
        let unique: std::collections::BTreeSet<i64> = versions.iter().copied().collect();
        assert_eq!(
            unique.len(),
            versions.len(),
            "migration versions must be unique"
        );
        assert!(!MIGRATIONS.is_empty(), "at least one migration required");
        for m in MIGRATIONS {
            assert!(!m.1.is_empty(), "migration name must not be empty");
            assert!(!m.2.trim().is_empty(), "migration sql must not be empty");
        }
    }

    #[test]
    fn verify_signature_empty_secret_rejects() {
        assert!(!verify_signature("", b"hello", ""));
        assert!(!verify_signature("", b"hello", "sha256=00"));
    }

    #[test]
    fn verify_signature_rejects_bad() {
        assert!(!verify_signature("secret", b"hello", "sha256=deadbeef"));
        assert!(!verify_signature("secret", b"hello", ""));
        assert!(!verify_signature("secret", b"hello", "sha1=deadbeef"));
    }

    #[test]
    fn verify_signature_accepts_valid() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type H = Hmac<Sha256>;
        let mut mac = H::new_from_slice(b"secret").unwrap();
        mac.update(b"hello");
        let hexsig = hex::encode(mac.finalize().into_bytes());
        let header = format!("sha256={}", hexsig);
        assert!(verify_signature("secret", b"hello", &header));
    }

    #[test]
    fn secret_checks_fail_closed() {
        assert_eq!(check_secret("", ""), AuthDecision::MissingConfig);
        assert_eq!(check_secret("", "anything"), AuthDecision::MissingConfig);
        assert_eq!(check_secret("admin", ""), AuthDecision::Denied);
        assert_eq!(check_secret("admin", "wrong"), AuthDecision::Denied);
        assert_eq!(check_secret("admin", "admin"), AuthDecision::Allowed);
    }

    #[test]
    fn runner_tokens_are_hashed_and_never_time_derived() {
        let token = format_runner_token(&[0xab; 32]);
        assert!(token.starts_with("frt_"));
        assert_eq!(token.len(), 4 + 64);
        assert_eq!(hash_token("token").len(), 64);
        assert_ne!(hash_token("token"), hash_token("token2"));

        // Old scheme formatted a timestamp; a fixed byte pattern must not leak it.
        let a = format_runner_token(&[1; 32]);
        let b = format_runner_token(&[1; 32]);
        assert_eq!(a, b);
        assert_ne!(a, format_runner_token(&[2; 32]));
    }

    #[test]
    fn runner_and_repo_identifiers_are_validated() {
        assert!(is_valid_runner_id("runner-1"));
        assert!(is_valid_runner_id("user@host"));
        assert!(!is_valid_runner_id(""));
        assert!(!is_valid_runner_id("bad runner"));
        assert!(!is_valid_runner_id("../runner"));

        assert!(is_valid_repo_name("owner/repo"));
        assert!(is_valid_repo_name("owner/repo.name"));
        assert!(!is_valid_repo_name("owner"));
        assert!(!is_valid_repo_name("owner/repo/extra"));
        assert!(!is_valid_repo_name("owner/../repo"));
        assert!(!is_valid_repo_name("owner/repo name"));
    }

    #[test]
    fn repo_allowlist_matches_case_insensitively() {
        assert!(repo_is_allowed("", "owner/repo"));
        assert!(repo_is_allowed("owner/repo", "Owner/Repo"));
        assert!(repo_is_allowed("a/b, owner/repo", "owner/repo"));
        assert!(!repo_is_allowed("owner/other", "owner/repo"));
    }

    #[test]
    fn installation_scope_is_enforced_when_configured() {
        let payload = serde_json::json!({"installation": {"id": 42}});
        assert!(installation_allowed("", &payload));
        assert!(installation_allowed("42", &payload));
        assert!(!installation_allowed("43", &payload));
        assert!(!installation_allowed("not-a-number", &payload));
        assert!(!installation_allowed("42", &serde_json::json!({})));
    }

    #[test]
    fn permission_evaluation_denies_unknown_and_failures() {
        assert_eq!(
            evaluate_permission(200, r#"{"permission":"admin"}"#),
            PermissionCheck::Allowed
        );
        assert_eq!(
            evaluate_permission(200, r#"{"permission":"write"}"#),
            PermissionCheck::Allowed
        );
        assert_eq!(
            evaluate_permission(200, r#"{"permission":"read"}"#),
            PermissionCheck::Insufficient
        );
        assert_eq!(
            evaluate_permission(200, r#"{"permission":"triage"}"#),
            PermissionCheck::Insufficient
        );
        assert_eq!(
            evaluate_permission(200, "{}"),
            PermissionCheck::Insufficient
        );
        assert_eq!(
            evaluate_permission(404, r#"{"message":"Not Found"}"#),
            PermissionCheck::Insufficient
        );
        assert_eq!(evaluate_permission(500, "{}"), PermissionCheck::Unavailable);
        assert_eq!(
            evaluate_permission(200, "not json"),
            PermissionCheck::Unavailable
        );
    }

    #[test]
    fn repo_scope_evaluation_denies_unknown_and_failures() {
        assert_eq!(
            evaluate_repo(200, r#"{"full_name":"owner/repo"}"#, "owner/repo"),
            RepoCheck::Authorized
        );
        assert_eq!(
            evaluate_repo(200, r#"{"full_name":"owner/repo"}"#, "Owner/Repo"),
            RepoCheck::Authorized
        );
        assert_eq!(
            evaluate_repo(200, r#"{"full_name":"attacker/repo"}"#, "owner/repo"),
            RepoCheck::Denied
        );
        assert_eq!(evaluate_repo(404, "{}", "owner/repo"), RepoCheck::Denied);
        assert_eq!(
            evaluate_repo(500, "{}", "owner/repo"),
            RepoCheck::Unavailable
        );
        assert_eq!(
            evaluate_repo(200, "not json", "owner/repo"),
            RepoCheck::Unavailable
        );
    }
}
