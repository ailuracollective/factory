# Architecture

## Monorepo

- Cargo workspace at root (`Cargo.toml`) with Rust crates under `apps/*` and `internal/*`.
- Each app builds/tests independently via cargo.

## Components

### Factory CLI (`apps/cli`)

Local daemon + manual commands. Three modes:

- Local mode (existing): webhooks direct to machine, T3 agent launch with the
  configured harness + model (`defaults.harness`, `defaults.model`).
- Mentions: `factory daemon run --mode listener` handles mention webhooks in real time
  (`issue_comment`, `issues`, `pull_request`, `pull_request_review_comment`).
  Parsing, authorization, dedup and dispatch live in `apps/cli/src/mentions.rs`
  behind `Engine`, reused by GitHub webhooks and runner job pushes.
- Connected mode (future): outbound connection to Gateway, claim jobs, report status.

### Factory Gateway (`apps/gateway`)

Cloudflare Worker (Rust, wasm32-unknown-unknown). Responsibilities:

- GitHub App webhook receiver (signature verification).
- Authorization policies per installation/repo/user.
- Job persistence in D1.
- Runner-facing HTTP API for claim/heartbeat/result (outbound from CLI).
- Admin API for dashboard.

No Tokio/SQLx. Use `worker` crate, D1 bindings, async/await. Axum may be used via `http` feature for routing if needed; prefer `worker::Router` for simplicity.

Execution logs are captured locally by the CLI runner for executions it runs,
including Gateway-origin jobs dispatched to that runner. The Gateway does not
currently store or serve execution log streams; remote log availability is
limited to the runner/local machine holding the execution log files.

## Data flow (MVP)

GitHub -> Gateway webhook -> authorize -> persist job (D1) -> Gateway pushes job to runner webhook -> runs agent -> reports result -> Gateway updates GitHub (comment/PR) with minimal permissions.

## Security

- Gateway fails closed on missing or invalid authentication configuration; see [gateway-security.md](gateway-security.md).
- Webhook signatures mandatory in gateway.
- Runner auth via CSPRNG tokens stored as hashes, with revocation and rotation.
- No auto-merge/deploy in MVP.
- Treat issue/PR content as untrusted.
