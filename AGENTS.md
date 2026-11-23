# AGENTS.md

Guidance for coding agents working in this repository.

## Project

Ailura Factory is a monorepo for AI-assisted development automation.

- `apps/cli` — Rust CLI (`cli`), runs agents locally (T3 Code today).
- `apps/gateway` — Rust Cloudflare Worker (GitHub App gateway, D1 persistence).
- `apps/bot` — Rust bot for chat notifications.
- `internal/notifications` — Shared notification contract.
- `docs/` — architecture, audit, migration plan, roadmap.
- `Cargo.toml` (root) — Cargo workspace for Rust crates.

## Build / Test / Lint

```bash
cargo fmt --all -- --check
cargo clippy -p cli -- -D warnings
cargo build -p cli
cargo test -p cli
```

Gateway (wasm32):
```bash
cargo clippy -p gateway --target wasm32-unknown-unknown -- -D warnings
cargo build -p gateway --target wasm32-unknown-unknown --release
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
