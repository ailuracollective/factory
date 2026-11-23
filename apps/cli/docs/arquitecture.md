# Architecture — CLI

## Role

Ailura Factory local/daemon CLI: receives GitHub events, manages repos/config, launches T3 agents, and operates in local, mentions, or connected-runner mode.

## Style

Modular monolith by layers (transitioned from a flat layout).

- **Interface/CLI**: `src/main.rs` (clap parser/dispatch), `src/interface/cli.rs` (commands), `src/interface/doctor.rs` (diagnostics).
- **Application/core**: `src/application/agent.rs` (T3 launch), `src/application/execution.rs` (lifecycle/cancel), `src/application/repo_manager.rs` (repo management).
- **Inbound/adapters**: `src/adapters/listener.rs` (Axum webhook + HMAC), `src/adapters/mentions.rs` (polling/webhook, `EventSource`/`Engine`), `src/adapters/webhook_manager.rs` (GitHub webhook integration).
- **Infra/persistence**: `src/infra/config.rs` (JSON config), `src/infra/state.rs` (SQLite/SeaORM, issues/projects state), `src/infra/t3.rs` (T3 Code harness/model catalog read from `<T3CODE_HOME>/caches`).

## Flow

```text
CLI args / GitHub webhook / mention poll
        -> interface (main/cli)
        -> application (agent/execution/repo_manager)
        -> adapters (listener/mentions/webhook_manager)
        -> infra (config/state SQLite+JSON, t3 harness catalog)
        -> external processes (gh, T3, git)
```

## Rules

- `main.rs` only parses and dispatches; logic lives in modules.
- State/config paths are controlled by env (`ISSUE_FACTORY_*`).
- Changes must preserve existing CLI commands and behavior.
- External dependencies go through `std::process::Command`/HTTP; errors use `anyhow::Result`.
