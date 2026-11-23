# Ailura Factory

Ailura Factory exists to reduce the manual work around “issue becomes a coded change”: it connects GitHub activity to AI-assisted execution (agents) while keeping the workflow predictable for teams.

This repository is a **monorepo** so the moving parts stay versioned together: a local operator CLI, a cloud gateway for webhooks and job coordination, and notification adapters. Splitting them keeps responsibilities clear (run work locally vs. receive events reliably vs. fan out messages), while sharing contracts and docs in one place.

## Apps

| App | Why it exists | README |
|---|---|---|
| `apps/cli` | The day-to-day tool on a maintainer machine: configure repos, run the listener or mention polling, manage the daemon, and launch agent runs when you want local control. | [apps/cli/README.md](apps/cli/README.md) |
| `apps/gateway` | A Cloudflare Worker edge entrypoint for GitHub webhooks and runner/job orchestration. It exists so events are received reliably without exposing your laptop, and so remote runners/jobs can be coordinated with durable state (D1). | [apps/gateway/README.md](apps/gateway/README.md) |
| `apps/bot` | A small service that turns internal notifications into chat messages (for example Discord). It exists to decouple “something happened” from “how it is announced”, so notification delivery can evolve independently. | [apps/bot/README.md](apps/bot/README.md) |

Other packages:

- `internal/notifications` — shared notification types/contracts so producers and consumers agree on payloads.

## Workspace commands

From the repository root (Rust workspace):

```bash
cargo fmt --all -- --check
cargo clippy -p cli -- -D warnings
cargo build -p cli
cargo test -p cli
```

Gateway (wasm32) checks/build:

```bash
cargo clippy -p gateway --target wasm32-unknown-unknown -- -D warnings
cargo build -p gateway --target wasm32-unknown-unknown --release
```

## Documentation

- `docs/` — architecture, audits, migration notes, roadmap, and feature docs (for example mentions flow).

## License

MIT
