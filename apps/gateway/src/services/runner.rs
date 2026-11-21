use ohkami::{claw::Path, Request, Response};
use serde::Deserialize;

use super::json_response;
use crate::domain::{auth, validation};
use crate::infra;
use crate::repos::{jobs, runners};

#[derive(Deserialize)]
struct RegisterReq {
    runner_id: String,
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
            )
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
    if runners::register(&conn, &body.runner_id, &auth::hash_token(&token), ts)
        .await
        .is_err()
    {
        return json_response(&serde_json::json!({"error": "db unavailable"}), 500);
    }
    infra::audit(&conn, "runner_registered", &body.runner_id).await;
    json_response(
        &serde_json::json!({"runner_id": body.runner_id, "token": token}),
        200,
    )
}

#[derive(Deserialize)]
struct ClaimReq {
    runner_id: String,
    token: String,
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

pub async fn handle_claim(req: &Request, env: &worker::Env) -> Response {
    let body: ClaimReq = match serde_json::from_slice(req.payload().unwrap_or(&[])) {
        Ok(v) => v,
        Err(_) => return json_response(&serde_json::json!({"error": "invalid json"}), 400),
    };
    let conn = match infra::db::connect(env).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if infra::db::ensure_schema(&conn).await.is_err() {
        return json_response(&serde_json::json!({"error": "db unavailable"}), 500);
    }
    if !verify_runner(&conn, &body.runner_id, &body.token).await {
        return json_response(&serde_json::json!({"error": "unauthorized"}), 401);
    }

    let pending_id = match jobs::find_pending(&conn).await {
        Ok(v) => v,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    let Some(jid) = pending_id else {
        return json_response(&serde_json::json!({"status": "no_job"}), 204);
    };

    let ts = infra::time::now_ts();
    let lease = ts + 300;
    let changes = match jobs::claim(&conn, &jid, &body.runner_id, lease, ts).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if changes == 0 {
        return json_response(&serde_json::json!({"status": "no_job"}), 204);
    }
    let row = match jobs::get(&conn, &jid).await {
        Ok(v) => v,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    match row {
        Some(j) => json_response(&j, 200),
        None => json_response(&serde_json::json!({"status": "no_job"}), 204),
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
    json_response(&serde_json::json!({"status": "ok"}), 200)
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
