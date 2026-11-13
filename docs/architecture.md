# Architecture

## Monorepo

- pnpm workspace (`pnpm-workspace.yaml`) with `apps/*` (and future `packages/*`), including Rust apps `apps/cli`, `apps/gateway` and `apps/dashboard`.
- Rust apps invoke cargo through pnpm scripts; no Cargo workspace at root.
- Each app builds/deploy independently.

## Components

### Factory CLI (`apps/cli`)
Local daemon + manual commands. Two modes:
- Local mode (existing): webhooks direct to machine, T3 agent launch.
- Connected mode (future): outbound connection to Gateway, claim jobs, report status.

### Factory Gateway (`apps/gateway`)
Cloudflare Worker (Rust, wasm32-unknown-unknown). Responsibilities:
- GitHub App webhook receiver (signature verification).
- Authorization policies per installation/repo/user.
- Job persistence in D1.
- Runner-facing HTTP API for claim/heartbeat/result (outbound from CLI).
- Admin API for dashboard.

No Tokio/SQLx. Use `worker` crate, D1 bindings, async/await. Axum may be used via `http` feature for routing if needed; prefer `worker::Router` for simplicity.

### Factory Dashboard (`apps/dashboard`)
TanStack (React) + TanStack Router + TanStack Query. Admin auth via GitHub. Consumes Gateway admin API.

## Data flow (MVP)

GitHub -> Gateway webhook -> authorize -> persist job (D1) -> CLI polls/claims job -> runs agent -> reports result -> Gateway updates GitHub (comment/PR) with minimal permissions.

## Security

- Gateway fails closed on missing or invalid authentication configuration; see [gateway-security.md](gateway-security.md).
- Webhook signatures mandatory in gateway.
- Runner auth via CSPRNG tokens stored as hashes, with revocation and rotation.
- No auto-merge/deploy in MVP.
- Treat issue/PR content as untrusted.
