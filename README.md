# AI Factory

A console application that listens to GitHub issues and launches [T3 Code](https://t3.codes) agents to resolve them.
Written in Rust. Runs locally on your machine — no cloud hosting required.

## Build

```bash
cargo build -p cli --release
# binary: target/release/cli
cp target/release/cli ~/.local/bin/cli
```

## Quick start

```bash
cli init                    # create config, generate secret
cli doctor                  # verify dependencies
cli repo add /path/to/repo  # register a repo
tailscale funnel --bg 8787      # expose webhook
cli start                   # start daemon
```

Then create a webhook in each repo (Settings → Webhooks) pointing to your funnel URL
with the secret from `cli init`.

## Commands

| Command | Description |
|---|---|
| `cli init [--force]` | Initialize factory config |
| `cli repo add <path>` | Register an existing repo (labels, T3, webhook) |
| `cli repo list` | List repos |
| `cli repo remove <name>` | Unregister a repo |
| `cli repo sync <name>` | Re-apply labels from standards |
| `cli start` | Start daemon |
| `cli stop` | Stop daemon |
| `cli restart` | Restart daemon |
| `cli status` | Show status |
| `cli secret rotate` | Rotate webhook secret |
| `cli logs [-f] [-n N]` | Show/follow daemon logs |
| `cli doctor` | Run diagnostics |
| `cli test <repo> <issue#> [--prompt "..."]` | Manually launch an agent for an issue (optional custom prompt) |
| `cli run <repo> --prompt "..."` | Launch an agent with a custom prompt (no issue) |
| `cli listen [--port N]` | Run webhook listener in foreground |
| `cli mentions [--once] [--interval N]` | Poll GitHub for configured mentions and dispatch work |

## How it works

1. GitHub sends an `issues` webhook (opened / reopened / labeled).
2. The factory verifies the HMAC signature and filters by `status/ready` (Ailura Collective standard).
3. For each qualifying issue, it launches a T3 Code agent in an isolated git worktree/branch.
4. The agent implements, tests, and opens a pull request (does not merge).

Alternatively, mentions can be handled in real time via webhooks through
`cli listen` (recommended when webhooks are configured) or by polling with
`cli mentions` (no public endpoint needed). See
[docs/mentions.md](docs/mentions.md).

## Requirements

- Rust toolchain (rustup)
- `gh` CLI authenticated
- T3 Code service running
- Tailscale Funnel (or any public tunnel)

## Configuration

Config lives at `~/.config/ailuracollective/factory/config.json`. See `cli init`.
Mention triggers are documented in [docs/mentions.md](docs/mentions.md).

## License

MIT
