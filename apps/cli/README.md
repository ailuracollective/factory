# Ailura Factory CLI

## Overview

The CLI is the operator surface for Ailura Factory on your own machine. It exists because most teams want **local control and transparency** first: you decide which repositories are enrolled, when the daemon runs, and whether work is triggered by GitHub webhooks (with a tunnel) or by polling for mentions.

It manages repositories, runs webhook listener or mention polling flows, and launches agents (via T3 Code) to work on GitHub issues. Keeping this in a single CLI reduces setup drift (config, secrets, labels, health checks) compared to scripting the same steps ad hoc.

Binaries (built from this workspace):

- `factory`
- `afc` (shortcut)

## Build

From the repository root:

```bash
cargo build -p cli --release
# binaries: target/release/factory, target/release/afc
```

## Usage

Run `factory --help` for the full command list. Common commands:

- `factory init [--force]` — initialize config
- `factory doctor` — verify dependencies
- `factory repo add <repo>` / `factory repo list` / `factory repo remove <name>` — manage repositories
- `factory daemon start|stop|restart|status|logs|run` — daemon lifecycle
- `factory daemon run --mode listener [--port N]` — run webhook listener in the foreground
- `factory runner` — run as a connected runner (receives job pushes)
- `factory daemon run --mode listener` — run webhook listener (processes GitHub events and runner jobs)
- `factory agent harnesses [--json]` — list harnesses installed in T3 Code
- `factory agent models <harness> [--json]` — list models of a harness

## Configuration

Configuration and state are stored under:

- `~/.config/ailuracollective/factory/`

`defaults.harness` is the **agent harness** installed in T3 Code
(`opencode`, `pi`, `codex`, `claudeAgent`, …). `defaults.model` is a model slug
that belongs to that harness and is passed unchanged to T3, so it can name any
model the harness exposes. Both are what the CLI hands to `t3 thread start` for
the agent.

```bash
factory config set -g harness pi
factory config set -g model opencode-go/beam-free
```

Values come from T3 Code's own catalog (`<T3CODE_HOME>/caches`, default
`~/.t3/caches`), not from `opencode models`, so no harness or model names are
hardcoded in the CLI:

- `factory agent harnesses` lists the harnesses T3 reports as installed, marking the
  configured one.
- `factory agent models <harness>` lists the models that harness exposes, e.g.
  `factory agent models opencode`.
- An unknown harness prints the harnesses T3 reports as installed.

`factory config set` validates `harness` against installed harnesses and
`model` against the configured harness when T3's catalog is available; if it
is not, values are stored as given.

`ISSUE_FACTORY_HARNESS` and `ISSUE_FACTORY_MODEL` override the configured
values for a single process (for example the systemd unit in
`service.example`).

## Logs

The daemon emits compact warning-and-error logs by default. Set `RUST_LOG=info`
in the systemd unit temporarily when diagnosing normal webhook activity.
Systemd's journal rate-limits each service; the example unit allows 200 messages
per 30 seconds before journald suppresses excess output. Journal retention stays
under the system-wide journald policy. Inspect Factory logs with
`journalctl --user -u factory --since today`; do not vacuum the shared journal
to clean only Factory logs.

## More

- Root overview: [../../README.md](../../README.md)
- Mentions documentation: [../../docs/mentions.md](../../docs/mentions.md) (if present in your checkout)
