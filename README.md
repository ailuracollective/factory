# AI Factory

A console application that listens to GitHub issues and launches [T3 Code](https://t3.codes) agents to resolve them.
Runs locally on your machine — no cloud hosting required.

## Installation

```bash
git clone https://github.com/ailuracollective/factory.git
cd factory
pip install -e .
```

## Quick start

```bash
factory init                    # create config, generate secret
factory doctor                  # verify dependencies
factory repo add ailuracollective/test_bots
factory secret rotate           # (if needed) update secret everywhere
tailscale funnel --bg 8787      # expose webhook
factory start                   # start daemon
```

Then create a webhook in each repo (Settings → Webhooks) pointing to your funnel URL
with the secret from `factory init`.

## Commands

| Command | Description |
|---|---|
| `factory init [--force]` | Initialize config |
| `factory repo add <path-or-name>` | Register an existing repo (labels, T3, webhook) |
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
| `factory test <repo> <issue#>` | Manually launch an agent for an issue |

## How it works

1. GitHub sends an `issues` webhook (opened / reopened / labeled).
2. The factory verifies the HMAC signature and filters by `status/ready` (Ailura Collective standard).
3. For each qualifying issue, it launches a T3 Code agent in an isolated git worktree/branch.
4. The agent implements, tests, and opens a pull request (does not merge).

## Requirements

- Python 3.12+
- `gh` CLI authenticated
- T3 Code service running
- Tailscale Funnel (or any public tunnel)

## Configuration

Config lives at `~/.config/factory/config.json`. See `factory init`.

## Tests

```bash
PYTHONPATH=src pytest -q
```

## License

MIT
