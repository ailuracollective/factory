# AGENTS.md

Guidance for contributors and coding agents working in Ailura Factory.

## Organization standards

The mandatory organization conventions are in
[.agents/organization-conventions.md](.agents/organization-conventions.md).
Read them before working; they govern issues, pull requests, labels, reviews,
commits, and workflows.

For AI-assisted adoption of the Factory execution label catalog, see
[docs/factory-execution-label-adoption.md](docs/factory-execution-label-adoption.md).

## Project

Ailura Factory is a monorepo for AI-assisted development automation.

- `apps/cli` — Rust CLI (`factory`, shortcut `afc`), runs agents locally (T3 Code today).
- `apps/gateway` — Rust Cloudflare Worker (GitHub App gateway, D1 persistence).
- `apps/bot` — Rust bot for chat notifications.
- `internal/notifications` — Shared notification contract.
- `docs/` — architecture, audit, migration plan, roadmap.
- `Cargo.toml` (root) — Cargo workspace for Rust crates.

Each app directory contains a `README.md` with app-specific setup and usage.

## Build / Test / Lint

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Conventions

- Rust 2021 edition; errors via `anyhow::Result` in CLI.
- Gateway targets `wasm32-unknown-unknown` via Cloudflare Workers (`worker` crate). No Tokio/SQLx in gateway; use D1 bindings and async/await.
- Config/state are JSON on disk under `~/.config/ailuracollective/factory/` (CLI).
- External tools invoked with `std::process::Command`; prefer existing `run(...)` helpers.
- Environment overrides: `ISSUE_FACTORY_*`.
- Keep changes minimal; add small tests for new non-trivial logic.
- Preserve existing CLI commands and behavior.

## Notes

- Gateway does not replace the local listener; keep both during transition.
- Do not couple gateway to a specific AI provider.
- See app READMEs (`apps/*/README.md`) for per-app details.

## Repository-specific conventions

- Do not configure automated release or deployment workflows without a separate
  maintainer decision. Release and deployment are not part of standards adoption.

## Pull requests

- Use Conventional Commit titles and a matching
  [.github/PULL_REQUEST_TEMPLATE/](.github/PULL_REQUEST_TEMPLATE/).
- Branch names are not validated by the policy workflow.
- Link and close a `status/ready` issue, and apply exactly one `type/*` label.
- The `## Verification` section records evidence; CI commands are documented
  here, not copied into the shared templates.
- Policy status comments are disabled until maintainers approve a comment
  identity and configure its credentials.
