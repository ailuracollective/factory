use ohkami::{claw::Path, Request, Response};

use super::json_response;
use crate::infra;
use crate::repos::{jobs, runners};

pub async fn handle_admin_jobs(req: &Request, env: &worker::Env) -> Response {
    if let Some(resp) = super::guard(req, env, "x-admin-token", "ADMIN_TOKEN") {
        return resp;
    }
    let conn = match infra::db::connect(env).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if infra::db::ensure_schema(&conn).await.is_err() {
        return json_response(&serde_json::json!({"error": "db unavailable"}), 500);
    }
    let rows = match jobs::list(&conn).await {
        Ok(r) => r,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    json_response(&serde_json::json!({"jobs": rows}), 200)
}

pub async fn handle_cancel(Path(id): Path<String>, req: &Request, env: &worker::Env) -> Response {
    if let Some(resp) = super::guard(req, env, "x-admin-token", "ADMIN_TOKEN") {
        return resp;
    }
    let conn = match infra::db::connect(env).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if infra::db::ensure_schema(&conn).await.is_err() {
        return json_response(&serde_json::json!({"error": "db unavailable"}), 500);
    }
    let ts = infra::time::now_ts();
    let changes = match jobs::cancel(&conn, &id, ts).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if changes == 0 {
        return match jobs::cancel_status(&conn, &id).await {
            Ok(Some(status)) if status == "cancelled" => json_response(
                &serde_json::json!({"status": "ok", "job_status": "cancelled", "idempotent": true}),
                200,
            ),
            Ok(Some(status)) if status == "completed" || status == "failed" => json_response(
                &serde_json::json!({"error": "job already terminal", "job_status": status}),
                409,
            ),
            Ok(Some(status)) => json_response(
                &serde_json::json!({"error": "job not cancellable", "job_status": status}),
                409,
            ),
            Ok(None) => json_response(&serde_json::json!({"error": "job not found"}), 404),
            Err(_) => json_response(&serde_json::json!({"error": "db unavailable"}), 500),
        };
    }
    infra::audit(&conn, "job_cancelled", &id).await;
    json_response(
        &serde_json::json!({"status": "ok", "job_status": "cancelled"}),
        200,
    )
}

pub async fn handle_revoke_runner(
    Path(id): Path<String>,
    req: &Request,
    env: &worker::Env,
) -> Response {
    if let Some(resp) = super::guard(req, env, "x-admin-token", "ADMIN_TOKEN") {
        return resp;
    }
    let conn = match infra::db::connect(env).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if infra::db::ensure_schema(&conn).await.is_err() {
        return json_response(&serde_json::json!({"error": "db unavailable"}), 500);
    }
    let ts = infra::time::now_ts();
    let changes = match runners::revoke(&conn, &id, ts).await {
        Ok(c) => c,
        Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
    };
    if changes == 0 {
        return json_response(&serde_json::json!({"error": "runner not found"}), 404);
    }
    infra::audit(&conn, "runner_revoked", &id).await;
    json_response(
        &serde_json::json!({"status": "revoked", "runner_id": id}),
        200,
    )
}

pub async fn handle_rotate_runner(
    Path(id): Path<String>,
    req: &Request,
    env: &worker::Env,
) -> Response {
    if let Some(resp) = super::guard(req, env, "x-admin-token", "ADMIN_TOKEN") {
        return resp;
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
    let changes =
        match runners::rotate(&conn, &id, &crate::domain::auth::hash_token(&token), ts).await {
            Ok(c) => c,
            Err(_) => return json_response(&serde_json::json!({"error": "db unavailable"}), 500),
        };
    if changes == 0 {
        return json_response(&serde_json::json!({"error": "runner not found"}), 404);
    }
    infra::audit(&conn, "runner_rotated", &id).await;
    json_response(
        &serde_json::json!({"status": "rotated", "runner_id": id, "token": token}),
        200,
    )
}
