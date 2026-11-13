# AI Factory

A console application that listens to GitHub issues and launches [T3 Code](https://t3.codes) agents to resolve them.
Written in Rust. Runs locally on your machine — no cloud hosting required.

## Build

```bash
pnpm install
pnpm --filter cli build
# binary: apps/cli/target/release/factory
cp apps/cli/target/release/factory ~/.local/bin/factory
```

## Quick start

```bash
factory init                    # create config, generate secret
factory doctor                  # verify dependencies
factory repo add /path/to/repo  # register a repo
tailscale funnel --bg 8787      # expose webhook
factory start                   # start daemon
```

Then create a webhook in each repo (Settings → Webhooks) pointing to your funnel URL
with the secret from `factory init`.

## Commands

| Command | Description |
|---|---|
| `factory init [--force]` | Initialize factory config |
| `factory repo add <path>` | Register an existing repo (labels, T3, webhook) |
| `factory repo list` | List repos |
| `factory repo remove <name>` | Unregister a repo |
| `factory repo sync <name>` | Re-apply labels from standards |
| `factory start` | Start daemon |
| `factory stop` | Stop daemon |
| `factory restart` | Restart daemon |
| `factory status` | Show status |
| `factory secret rotate` | Rotate webhook secret |
| `factory logs [-f] [-n N]` | Show/follow daemon logs |
| `factory doctor` | Run diagnostics |
| `factory test <repo> <issue#> [--prompt "..."]` | Manually launch an agent for an issue (optional custom prompt) |
| `factory run <repo> --prompt "..."` | Launch an agent with a custom prompt (no issue) |
| `factory listen [--port N]` | Run webhook listener in foreground |
| `factory mentions [--once] [--interval N]` | Poll GitHub for configured mentions and dispatch work |

## How it works

1. GitHub sends an `issues` webhook (opened / reopened / labeled).
2. The factory verifies the HMAC signature and filters by `status/ready` (Ailura Collective standard).
3. For each qualifying issue, it launches a T3 Code agent in an isolated git worktree/branch.
4. The agent implements, tests, and opens a pull request (does not merge).

Alternatively, `factory mentions` polls allowlisted repositories for mentions of
your configured GitHub username and dispatches authorized requests without the
Gateway or GitHub notifications. See [docs/mentions.md](docs/mentions.md).

## Requirements

- Rust toolchain (rustup)
- `gh` CLI authenticated
- T3 Code service running
- Tailscale Funnel (or any public tunnel)

## Configuration

Config lives at `~/.config/ailuracollective/factory/config.json`. See `factory init`.
Mention triggers are documented in [docs/mentions.md](docs/mentions.md).

## License

MIT
