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
- `factory start` / `factory stop` / `factory restart` / `factory status` — daemon lifecycle
- `factory listen [--port N]` — run webhook listener in the foreground
- `factory runner` — run as a connected runner (receives job pushes)
- `factory listen` — run webhook listener (processes GitHub events and runner jobs)
- `factory test <repo> <issue> [--prompt "..."]` — manually launch an agent for an issue
- `factory run <repo> --prompt "..."` — launch an agent with a custom prompt
- `factory harnesses` — list harnesses installed in T3 Code
- `factory providers <harness>` — list model providers inside a harness
- `factory models <harness>` — list models of a harness

## Configuration

Configuration and state are stored under:

- `~/.config/ailuracollective/factory/`

`defaults.harness` is the **agent harness**, i.e. the T3 Code provider instance
(`opencode`, `pi`, `codex`, `claudeAgent`, …). `defaults.model` is a model slug
that belongs to that harness and is passed unchanged to T3, so it can name a
model from any provider the harness exposes. Both are what the CLI hands to
`t3 thread start` for the agent.

```bash
factory config set -g harness pi
factory config set -g model opencode-go/beam-free
```

Values come from T3 Code's own catalog (`<T3CODE_HOME>/caches`, default
`~/.t3/caches`), not from `opencode models`, so no harness or model names are
hardcoded in the CLI:

- `factory harnesses` lists the harnesses T3 reports as installed, marking the
  configured one.
- `factory models <harness>` lists the models that harness exposes, e.g.
  `factory models opencode`.
- `factory providers <harness>` lists that harness's model providers (slug
  prefix, e.g. `openai`, `opencode-go`).
- An unknown harness prints the harnesses T3 reports as installed.

`factory config set` validates `harness` against installed harnesses and
`model` against the configured harness when T3's catalog is available; if it
is not, values are stored as given.

## More

- Root overview: [../../README.md](../../README.md)
- Mentions documentation: [../../docs/mentions.md](../../docs/mentions.md) (if present in your checkout)
