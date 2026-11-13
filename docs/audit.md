# Technical Audit — Ailura Factory

## Current state (pre-monorepo)

Single Rust crate `factory` (edition 2021) with modules:

- `main.rs` — clap CLI dispatch
- `cli.rs` — command implementations
- `config.rs` — `~/.config/ailuracollective/factory/config.json`
- `state.rs` — processed issues / thread ids (`state.json`)
- `listener.rs` — axum webhook server (HMAC verify, IP allowlist, background tasks)
- `agent.rs` — builds prompt, invokes `t3 thread start`
- `repo_manager.rs` — repo resolution, T3 registration, label sync
- `webhook_manager.rs` — GitHub webhook CRUD via `gh`
- `doctor.rs` — dependency diagnostics

## Public CLI commands

`init`, `repo add/list/remove/sync`, `start/stop/restart/status`, `secret rotate`, `logs`, `doctor`, `test`, `run`, `listen`. These must remain compatible.

## Config & state

JSON on disk under `~/.config/ailuracollective/factory/`. Env overrides: `ISSUE_FACTORY_*`. State persists issues map with status (pending/running/failed/completed), attempts, thread_id, last_error.

## T3 integration

`agent::launch_agent` shells out to `t3 thread start` with provider/model/worktree/branch. Parsing thread id from JSON stdout. Must remain; provider abstraction added later without breaking.

## Repo/worktree handling

`repo_manager::resolve_project_path`, `add_repo` detects slug from git remote, registers T3 project, syncs labels from Ailura standards repo. No clone.

## Webhooks & signatures

`listener::verify_signature` HMAC-SHA256 with `x-hub-signature-256`; empty secret disables (dev). IP allowlist for Tailscale/GitHub ranges. Events: only `issues` accepted.

## Execution lifecycle

Reserve issue before launch; statuses completed/running/failed; retry up to max_attempts; concurrent delivery dedup; restart reconciles running->failed. Tests cover failed launch retry, duplicate delivery, concurrency, restart recovery.

## Tests / CI

14 tests in state/listener/agent. CI: fmt, clippy, build, test. No integration tests against real services.

## Dependencies & reuse

Rust deps: anyhow, regex, clap, colored, dirs, hex, hmac, reqwest, serde, sha2, tokio, axum, tracing. Reuse all. Gateway should reuse signature verification logic (port to wasm-compatible hmac/sha2, already compile to wasm).

## Open issues/PRs

Only #1 (recover failed launches) merged. No in-flight work. Safe to proceed.

## Cloudflare/Rust compatibility findings

- Rust on Workers is viable via `worker` crate (workers-rs), target `wasm32-unknown-unknown`.
- No Tokio/async_std runtime; use async/await of worker.
- Axum usable via `http` feature, but native axum server not; adapt to worker fetch handler.
- SQLx not usable; use D1 bindings (`worker::D1Database`) with SQL.
- D1 and Queues available via features. Queues optional for MVP.
