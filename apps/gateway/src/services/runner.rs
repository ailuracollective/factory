use ohkami::{Request, Response, claw::Path};
use serde::Deserialize;

use super::json_response;
use crate::domain::{auth, validation};
use crate::infra;
use crate::repos::{jobs, runners};
use sea_orm::ConnectionTrait;

#[derive(Deserialize)]
struct RegisterReq {
    runner_id: String,
    #[serde(default)]
    callback_url: Option<String>,
    #[serde(default)]
    callback_secret: Option<String>,
}

pub async fn handle_register(req: &Request, env: &worker::Env) -> Response {
    if let Some(resp) = super::guard(req, env, "x-bootstrap-secret", "RUNNER_BOOTSTRAP_SECRET") {
        return resp;
    }
    let body: RegisterReq = match serde_json::from_slice(req.payload().unwrap_or(&[])) {
        Ok(v) => v,
        Err(_) => return json_response(&serde_json::json!({"error": "invalid json"}), 400),
    };
    if !validation::is_valid_runner_id(&body.runner_id) {
        return json_response(&serde_json::json!({"error": "invalid runner_id"}), 400);
    }
    let token = match infra::time::random_runner_token() {
        Some(t) => t,
        None => {
            return json_response(
                &serde_json::json!({"error": "token generation failed"}),
                500,
            );
        }
    };
    let conn = match infra::db::connect(env).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if infra::db::ensure_schema(&conn).await.is_err() {
        return json_response(&serde_json::json!({"error": "db unavailable"}), 500);
    }
    let ts = infra::time::now_ts();
    let callback_url = body
        .callback_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let callback_secret = body
        .callback_secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if runners::register(
        &conn,
        &body.runner_id,
        &auth::hash_token(&token),
        callback_url,
        callback_secret,
        ts,
    )
    .await
    .is_err()
    {
        return json_response(&serde_json::json!({"error": "db unavailable"}), 500);
    }
    infra::audit(&conn, "runner_registered", &body.runner_id).await;
    if callback_url.is_some() {
        dispatch_pending(&conn).await;
    }
    json_response(
        &serde_json::json!({"runner_id": body.runner_id, "token": token}),
        200,
    )
}

async fn verify_runner(conn: &sea_orm::DatabaseConnection, runner_id: &str, token: &str) -> bool {
    if runner_id.is_empty() || token.is_empty() {
        return false;
    }
    let stored = match runners::token_hash(conn, runner_id).await {
        Ok(v) => v,
        Err(_) => return false,
    };
    match stored {
        Some(s) => auth::constant_time_eq(s.as_bytes(), auth::hash_token(token).as_bytes()),
        None => false,
    }
}

#[derive(Deserialize)]
struct HeartbeatReq {
    runner_id: String,
    token: String,
    status: Option<String>,
}

pub async fn handle_heartbeat(
    Path(id): Path<String>,
    req: &Request,
    env: &worker::Env,
) -> Response {
    let body: HeartbeatReq = match serde_json::from_slice(req.payload().unwrap_or(&[])) {
        Ok(v) => v,
        Err(_) => return json_response(&serde_json::json!({"error": "invalid json"}), 400),
    };
    let conn = match infra::db::connect(env).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if !verify_runner(&conn, &body.runner_id, &body.token).await {
        return json_response(&serde_json::json!({"error": "unauthorized"}), 401);
    }
    let ts = infra::time::now_ts();
    let lease = ts + 300;
    let status = body.status.unwrap_or_else(|| "running".to_string());
    let changes = match jobs::heartbeat(&conn, &id, &body.runner_id, &status, lease, ts).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if changes == 0 {
        return json_response(
            &serde_json::json!({"error": "job not found or not owned"}),
            404,
        );
    }
    let job_status = jobs::cancel_status(&conn, &id).await.ok().flatten();
    json_response(
        &serde_json::json!({"status": "ok", "job_status": job_status}),
        200,
    )
}

#[derive(Deserialize)]
struct CompleteReq {
    runner_id: String,
    token: String,
    thread_id: Option<String>,
}

pub async fn handle_complete(Path(id): Path<String>, req: &Request, env: &worker::Env) -> Response {
    let body: CompleteReq = match serde_json::from_slice(req.payload().unwrap_or(&[])) {
        Ok(v) => v,
        Err(_) => return json_response(&serde_json::json!({"error": "invalid json"}), 400),
    };
    let conn = match infra::db::connect(env).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if !verify_runner(&conn, &body.runner_id, &body.token).await {
        return json_response(&serde_json::json!({"error": "unauthorized"}), 401);
    }
    let ts = infra::time::now_ts();
    let changes =
        match jobs::complete(&conn, &id, &body.runner_id, body.thread_id.as_deref(), ts).await {
            Ok(c) => c,
            Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
        };
    if changes == 0 {
        return json_response(
            &serde_json::json!({"error": "job not found or not owned"}),
            404,
        );
    }
    infra::audit(&conn, "job_completed", &id).await;
    json_response(&serde_json::json!({"status": "ok"}), 200)
}

#[derive(Deserialize)]
struct FailReq {
    runner_id: String,
    token: String,
    error: Option<String>,
}

pub async fn handle_fail(Path(id): Path<String>, req: &Request, env: &worker::Env) -> Response {
    let body: FailReq = match serde_json::from_slice(req.payload().unwrap_or(&[])) {
        Ok(v) => v,
        Err(_) => return json_response(&serde_json::json!({"error": "invalid json"}), 400),
    };
    let conn = match infra::db::connect(env).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if !verify_runner(&conn, &body.runner_id, &body.token).await {
        return json_response(&serde_json::json!({"error": "unauthorized"}), 401);
    }
    let ts = infra::time::now_ts();
    let err = body.error.unwrap_or_else(|| "unknown error".to_string());
    let changes = match jobs::fail(&conn, &id, &body.runner_id, &err, ts).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if changes == 0 {
        return json_response(
            &serde_json::json!({"error": "job not found or not owned"}),
            404,
        );
    }
    infra::audit(&conn, "job_failed", &id).await;
    json_response(&serde_json::json!({"status": "ok"}), 200)
}

pub async fn dispatch_pending(conn: &sea_orm::DatabaseConnection) {
    for _ in 0..10 {
        let Some(id) = jobs::find_pending(conn).await.ok().flatten() else {
            break;
        };
        if !dispatch_job(conn, &id).await {
            break;
        }
    }
}

pub async fn dispatch_job(conn: &sea_orm::DatabaseConnection, job_id: &str) -> bool {
    let job = match jobs::get(conn, job_id).await {
        Ok(Some(j)) => j,
        _ => return false,
    };
    let targets = match runners::callback_targets(conn).await {
        Ok(t) => t,
        Err(_) => return false,
    };
    let body = match serde_json::to_string(&job) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let ts = infra::time::now_ts();
    let lease = ts + 300;
    for t in targets {
        let changes = match jobs::claim(conn, job_id, &t.runner_id, lease, ts).await {
            Ok(c) => c,
            Err(_) => continue,
        };
        if changes == 0 {
            continue;
        }
        match infra::http::http_post_json(&t.url, &t.secret, &body).await {
            Ok((status, _)) if (200..300).contains(&status) => {
                infra::audit(conn, "job_dispatched", job_id).await;
                return true;
            }
            _ => {
                // revert claim so another runner or retry can pick it up
                let _ = conn
                    .execute_raw(sea_orm::Statement::from_sql_and_values(
                        sea_orm::DatabaseBackend::Sqlite,
                        "UPDATE jobs SET status='pending', runner_id=NULL, lease_until=NULL, updated_at=? WHERE id=? AND status='claimed' AND runner_id=?",
                        [infra::time::now_ts().into(), job_id.into(), t.runner_id.into()],
                    ))
                    .await;
            }
        }
    }
    false
}
