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

#[allow(dead_code)]
fn verify_signature(secret: &str, body: &[u8], header: &str) -> bool {
    if secret.is_empty() {
        return true;
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
    if expected.len() != result.len() {
        return false;
    }
    expected
        .iter()
        .zip(result.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

#[cfg(target_arch = "wasm32")]
fn json_response(value: &serde_json::Value, status: u16) -> Result<Response> {
    let mut r = Response::from_json(value)?;
    r = r.with_status(status);
    Ok(r)
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
async fn github_permission(repo: &str, user: &str, token: &str) -> Option<String> {
    let url = format!(
        "https://api.github.com/repos/{}/collaborators/{}/permission",
        repo, user
    );
    let (status, body) = http_get(&url, token).await.ok()?;
    if status != 200 {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    v.get("permission")
        .and_then(|p| p.as_str())
        .map(|s| s.to_string())
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
        .post_async("/admin/jobs/:id/cancel", |_req, ctx| async move {
            let id = decode_id(&ctx.param("id").cloned().unwrap_or_default());
            handle_cancel(&ctx.env, &id).await
        })
        .run(req, env)
        .await
}

#[cfg(target_arch = "wasm32")]
async fn handle_webhook(req: &mut Request, env: &Env) -> Result<Response> {
    let secret = env_str(env, "GITHUB_WEBHOOK_SECRET");
    let sig = req
        .headers()
        .get("x-hub-signature-256")
        .ok()
        .flatten()
        .unwrap_or_default();
    let event = req
        .headers()
        .get("x-github-event")
        .ok()
        .flatten()
        .unwrap_or_default();

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

    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    let _ = ensure_schema(&db).await;

    let sender = payload
        .get("sender")
        .and_then(|s| s.get("login"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let ts = now_ts();

    // load or create policy
    let policy_stmt = db.prepare(
        "SELECT enabled, allowed_users, max_concurrent, daily_quota FROM policies WHERE repo=?1",
    );
    let policy_row: Option<serde_json::Value> =
        match policy_stmt.bind(&[js_sys::JsString::from(repo.as_str()).into()]) {
            Ok(s) => s.first::<serde_json::Value>(None).await.ok().flatten(),
            Err(_) => None,
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
        if let Ok(s) = ins.bind(&[
            js_sys::JsString::from(repo.as_str()).into(),
            js_sys::Number::from(ts as f64).into(),
        ]) {
            let _ = s.run().await;
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
        Ok(s) => s
            .first::<serde_json::Value>(None)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.get("c").and_then(|x| x.as_i64()))
            .unwrap_or(0),
        Err(_) => 0,
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
        Ok(s) => s
            .first::<serde_json::Value>(None)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.get("c").and_then(|x| x.as_i64()))
            .unwrap_or(0),
        Err(_) => 0,
    };
    if daily_count >= daily_quota {
        audit(&db, "policy_rejected", &format!("{}: daily quota", repo)).await;
        return json_response(
            &json!({"error": "too_many_requests", "reason": "daily quota"}),
            429,
        );
    }

    // real permission check
    let admin_token = env_str(env, "GITHUB_TOKEN");
    if !admin_token.is_empty() && !sender.is_empty() {
        if let Some(perm) = github_permission(&repo, &sender, &admin_token).await {
            if !["admin", "maintain", "write"].contains(&perm.as_str()) {
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
    if let Ok(s) = stmt.bind(&[
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
        let _ = s.run().await;
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
    let bootstrap = env_str(env, "RUNNER_BOOTSTRAP_SECRET");
    let provided = req
        .headers()
        .get("x-bootstrap-secret")
        .ok()
        .flatten()
        .unwrap_or_default();
    if !bootstrap.is_empty() && provided != bootstrap {
        return json_response(&json!({"error": "unauthorized"}), 401);
    }
    let body: RegisterReq = match req.json().await {
        Ok(v) => v,
        Err(_) => return json_response(&json!({"error": "invalid json"}), 400),
    };
    let token = format!(
        "tok_{}",
        hex::encode(&js_sys::Date::now().to_string().as_bytes()[..8])
    );
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    let _ = ensure_schema(&db).await;
    let ts = now_ts();
    let stmt =
        db.prepare("INSERT OR REPLACE INTO runners (id, token, created_at) VALUES (?1, ?2, ?3)");
    if let Ok(s) = stmt.bind(&[
        js_sys::JsString::from(body.runner_id.as_str()).into(),
        js_sys::JsString::from(token.as_str()).into(),
        js_sys::Number::from(ts as f64).into(),
    ]) {
        let _ = s.run().await;
    }
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
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let _ = ensure_schema(&db).await;
    let stmt = db.prepare("SELECT token FROM runners WHERE id=?1");
    if let Ok(s) = stmt.bind(&[js_sys::JsString::from(runner_id).into()]) {
        if let Ok(Some(v)) = s.first::<serde_json::Value>(None).await {
            return v.get("token").and_then(|t| t.as_str()) == Some(token);
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
    let _ = ensure_schema(&db).await;

    let stmt = db.prepare("SELECT id FROM jobs WHERE status='pending' ORDER BY created_at LIMIT 1");
    let pending_id: Option<String> = match stmt.first::<serde_json::Value>(None).await {
        Ok(Some(v)) => v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()),
        _ => None,
    };
    let Some(jid) = pending_id else {
        return json_response(&json!({"status": "no_job"}), 204);
    };

    let ts = now_ts();
    let lease = ts + 300;
    let upd = db.prepare(
        "UPDATE jobs SET status='claimed', runner_id=?1, lease_until=?2, attempts=attempts+1, updated_at=?3 WHERE id=?4 AND status='pending'",
    );
    let changes = if let Ok(s) = upd.bind(&[
        js_sys::JsString::from(body.runner_id.as_str()).into(),
        js_sys::Number::from(lease as f64).into(),
        js_sys::Number::from(ts as f64).into(),
        js_sys::JsString::from(jid.as_str()).into(),
    ]) {
        match s.run().await {
            Ok(r) => r.meta().ok().flatten().and_then(|m| m.changes).unwrap_or(0),
            Err(_) => 0,
        }
    } else {
        0
    };
    if changes == 0 {
        return json_response(&json!({"status": "no_job"}), 204);
    }

    let stmt = db.prepare("SELECT * FROM jobs WHERE id=?1");
    let row: Option<serde_json::Value> =
        if let Ok(s) = stmt.bind(&[js_sys::JsString::from(jid.as_str()).into()]) {
            s.first::<serde_json::Value>(None).await.ok().flatten()
        } else {
            None
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
    if let Ok(s) = stmt.bind(&[
        js_sys::JsString::from(status.as_str()).into(),
        js_sys::Number::from(lease as f64).into(),
        js_sys::Number::from(ts as f64).into(),
        js_sys::JsString::from(id).into(),
        js_sys::JsString::from(body.runner_id.as_str()).into(),
    ]) {
        let _ = s.run().await;
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
    if let Ok(s) = stmt.bind(&[
        body.thread_id
            .as_deref()
            .map(|t| js_sys::JsString::from(t).into())
            .unwrap_or(JsValue::null()),
        js_sys::Number::from(ts as f64).into(),
        js_sys::JsString::from(id).into(),
        js_sys::JsString::from(body.runner_id.as_str()).into(),
    ]) {
        let _ = s.run().await;
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
    if let Ok(s) = stmt.bind(&[
        js_sys::JsString::from(err.as_str()).into(),
        js_sys::Number::from(ts as f64).into(),
        js_sys::JsString::from(id).into(),
        js_sys::JsString::from(body.runner_id.as_str()).into(),
    ]) {
        let _ = s.run().await;
    }
    audit(&db, "job_failed", id).await;
    json_response(&json!({"status": "ok"}), 200)
}

#[cfg(target_arch = "wasm32")]
async fn handle_admin_jobs(req: &Request, env: &Env) -> Result<Response> {
    let token = req
        .headers()
        .get("x-admin-token")
        .ok()
        .flatten()
        .unwrap_or_default();
    let admin = env_str(env, "ADMIN_TOKEN");
    if !admin.is_empty() && token != admin {
        return json_response(&json!({"error": "unauthorized"}), 401);
    }
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    let _ = ensure_schema(&db).await;
    let stmt = db.prepare("SELECT * FROM jobs ORDER BY created_at DESC LIMIT 100");
    let rows: Vec<serde_json::Value> = match stmt.all().await {
        Ok(r) => r.results().unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    json_response(&json!({"jobs": rows}), 200)
}

#[cfg(target_arch = "wasm32")]
async fn handle_cancel(env: &Env, id: &str) -> Result<Response> {
    let db = match db(env) {
        Ok(d) => d,
        Err(_) => return json_response(&json!({"error": "db unavailable"}), 500),
    };
    let ts = now_ts();
    let stmt = db.prepare("UPDATE jobs SET status='cancelled', updated_at=?1 WHERE id=?2");
    if let Ok(s) = stmt.bind(&[
        js_sys::Number::from(ts as f64).into(),
        js_sys::JsString::from(id).into(),
    ]) {
        let _ = s.run().await;
    }
    audit(&db, "job_cancelled", id).await;
    json_response(&json!({"status": "ok"}), 200)
}

#[cfg(test)]
mod tests {
    use super::verify_signature;
    use super::MIGRATIONS;

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
    fn verify_signature_empty_secret_accepts() {
        assert!(verify_signature("", b"hello", ""));
    }

    #[test]
    fn verify_signature_rejects_bad() {
        assert!(!verify_signature("secret", b"hello", "sha256=deadbeef"));
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
}
