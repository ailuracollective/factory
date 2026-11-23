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
- `factory mentions [--once] [--interval N]` — poll GitHub for configured mentions
- `factory test <repo> <issue> [--prompt "..."]` — manually launch an agent for an issue
- `factory run <repo> --prompt "..."` — launch an agent with a custom prompt

## Configuration

Configuration and state are stored under:

- `~/.config/ailuracollective/factory/`

## More

- Root overview: [../../README.md](../../README.md)
- Mentions documentation: [../../docs/mentions.md](../../docs/mentions.md) (if present in your checkout)
