# Migration Plan

## Phase 0 — Restructure (done)

- Move `src/` to `apps/cli/src/`.
- Move Cargo manifest to `apps/cli/Cargo.toml`.
- Add Cargo workspace root (`Cargo.toml`) with Rust crates.
- Remove pnpm workspace and dashboard scaffold.
- Update CI.
- Add docs.

## Phase 1 — Gateway MVP core

- D1 schema for installations, repos, policies, jobs, audit.
- Webhook verification + event parsing (issues).
- Authorization checks (basic allowlist).
- Job creation + dedup.
- Runner API: register, claim (lease), heartbeat, complete/fail.
- Admin API: list jobs, cancel.

## Phase 2 — CLI connected mode

- `factory connect` to register runner.
- Poll/claim jobs via outbound HTTPS.
- Report status/result.
- Reconcile on restart.

## Phase 3 — Dashboard (removed)

- Dashboard project removed from the monorepo; admin flows go through Gateway/admin APIs for now.

## Phase 4 — PR events & hardening

- PR comment handling.
- Rate limits/quotas.
- Retry/idempotency refinements.

## Compatibility

- Keep local listener and commands.
- Execution harness/model selection is configured through named `profiles` in repository `factory.json` (`default_profile` + `profiles.<name>.{harness,model}`). Legacy global/repo `defaults.harness`/`defaults.model` keys and `ISSUE_FACTORY_HARNESS`/`ISSUE_FACTORY_MODEL` overrides are not supported; migrate configs to profiles. Non-model defaults remain additive.
