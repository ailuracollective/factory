# AGENTS.md

Guidance for coding agents working in this repository.

## Project

AI Factory is a Rust CLI (`factory`) that listens to GitHub issue webhooks and launches
T3 Code agents to resolve them. Entry point: `src/main.rs` (clap). Modules live in `src/`.

## Build / Test / Lint

```bash
cargo build            # dev build
cargo build --release  # release build (binary: target/release/factory)
cargo test             # run tests (none yet)
cargo fmt              # format
cargo fmt --check      # verify formatting
cargo clippy           # lint
```

Install locally after release build:

```bash
cp target/release/factory ~/.local/bin/factory
```

## Run

```bash
cargo run -- doctor
cargo run -- listen --port 8787
# or installed binary:
factory doctor
```

## Architecture

- `src/main.rs` — clap CLI dispatch
- `src/cli.rs` — command implementations
- `src/config.rs` — `~/.config/factory/config.json` load/save
- `src/state.rs` — processed issues / thread ids (`~/.config/factory/state.json`)
- `src/listener.rs` — axum webhook server (HMAC verify, IP allowlist, background tasks)
- `src/agent.rs` — builds prompt, invokes `t3 thread start`
- `src/repo_manager.rs` — repo resolution, T3 registration, label sync
- `src/webhook_manager.rs` — GitHub webhook CRUD via `gh`
- `src/doctor.rs` — dependency diagnostics

## Conventions

- Rust 2021 edition; errors via `anyhow::Result`.
- Config/state are JSON on disk under `~/.config/factory/`.
- External tools (`gh`, `t3`, `curl`, `git`) are invoked with `std::process::Command`; prefer
  the existing `run(...)` helpers.
- Environment overrides: `ISSUE_FACTORY_*` (see README).
- Keep changes minimal; no tests exist yet — add a small test or assertion for new non-trivial logic.

## Notes

- `cargo clippy`/`cargo fmt --check` currently report warnings/formatting debt; don't
  introduce new ones.
- The listener binds `0.0.0.0`; IP allowlist in `listener.rs` restricts access.
- Webhook secret empty disables signature verification (dev only).