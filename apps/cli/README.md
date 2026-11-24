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
- `factory execution list|show|cancel|logs` — inspect and manage executions
- `factory execution logs <id> [--follow] [--lines <n>]` — show logs for an execution
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

### Execution logs

`factory execution logs <id> [--follow] [--lines <n>]` shows captured stdout/stderr
for a local execution record. Each execution writes to its own log file under the
Factory state directory (`logs/<execution_id>.log`), so concurrent executions have
distinguishable log streams.

- `--lines <n>` limits the history shown (default: 200).
- `--follow` emits subsequent records without repeating history. Interrupting
  follow (Ctrl-C) stops the log stream and does not cancel the execution.
- Unknown executions, unavailable logs, and empty logs return distinguishable
  messages.

Gateway-origin executions are covered when their logs were captured on the runner
that executed them. Logs are not centrally replicated to the Gateway; remote
availability is limited to runners/local machines that have the execution log
files. Log retention is currently file-based and local: logs remain until removed
by the operator or state directory cleanup. Credentials in captured output are
redacted on a best-effort basis before being written to log files.

## More

- Root overview: [../../README.md](../../README.md)
- Mentions documentation: [../../docs/mentions.md](../../docs/mentions.md) (if present in your checkout)
