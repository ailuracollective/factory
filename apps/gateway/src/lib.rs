// Native builds only exist to run unit tests; runtime helpers are wasm-only.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub mod domain;
pub mod entities;
pub mod migration;

#[cfg(target_arch = "wasm32")]
pub mod infra;
#[cfg(target_arch = "wasm32")]
pub mod repos;
#[cfg(target_arch = "wasm32")]
pub mod services;

#[cfg(target_arch = "wasm32")]
use ohkami::prelude::*;

#[cfg(target_arch = "wasm32")]
#[ohkami::worker]
async fn my_worker() -> Ohkami {
    Ohkami::new((
        "/health".GET(health),
        "/webhooks/github".POST(services::webhook::handle_webhook),
        "/runners/register".POST(services::runner::handle_register),
        "/jobs/claim".POST(services::runner::handle_claim),
        "/jobs/:id/heartbeat".POST(services::runner::handle_heartbeat),
        "/jobs/:id/complete".POST(services::runner::handle_complete),
        "/jobs/:id/fail".POST(services::runner::handle_fail),
        "/admin/jobs".GET(services::admin::handle_admin_jobs),
        "/admin/jobs/:id/cancel".POST(services::admin::handle_cancel),
        "/admin/runners/:id/revoke".POST(services::admin::handle_revoke_runner),
        "/admin/runners/:id/rotate".POST(services::admin::handle_rotate_runner),
    ))
}

#[cfg(target_arch = "wasm32")]
async fn health() -> &'static str {
    "ok"
}
