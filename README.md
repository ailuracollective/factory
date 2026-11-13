# AI Factory

Listens to GitHub issues and launches [T3 Code](https://t3.codes) agents to resolve them.
Runs locally on your machine, free of cloud hosting. Works with any repo that has a webhook configured.

## How it works

1. GitHub sends an `issues` webhook (opened / reopened / labeled) to your machine.
2. The factory verifies the HMAC signature and filters by `status/ready` (Ailura Collective standard).
3. For each qualifying issue, it launches a T3 Code agent in an isolated git worktree/branch.
4. The agent implements, tests, and opens a pull request (does not merge).

## Requirements

- Python 3.12+
- `gh` CLI authenticated (`gh auth login`)
- T3 Code service running (`t3 service status`)
- Tailscale Funnel (or any public tunnel) pointing to the webhook port

## Setup

```bash
git clone https://github.com/ailuracollective/factory.git
cd factory
uv sync  # or: pip install -e .
```

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `ISSUE_FACTORY_PROJECTS_BASE` | `~/projects/ailuracollective` | Base directory for repo clones |
| `ISSUE_FACTORY_REPO` | (empty = multi-repo) | Restrict to a single repo |
| `ISSUE_FACTORY_PROJECT` | (empty = auto) | Override project path for single repo |
| `ISSUE_FACTORY_LABEL` | `status/ready` | Only process issues with this label (empty = all) |
| `ISSUE_FACTORY_WEBHOOK_SECRET` | (empty) | HMAC secret (required in production) |
| `ISSUE_FACTORY_WEBHOOK_PORT` | `8787` | Webhook listen port |
| `ISSUE_FACTORY_PROVIDER` | `opencode` | T3 provider instance |
| `ISSUE_FACTORY_MODEL` | `opencode-go/beam-free` | Model slug |
| `ISSUE_FACTORY_BASE_BRANCH` | `main` | Base branch for worktrees |

## Run

```bash
# Webhook mode (real-time)
factory-listen --port 8787
```

## Run as a systemd service

```bash
mkdir -p ~/.config/systemd/user
cp deploy/factory.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now factory
```

## Expose via Tailscale Funnel

```bash
tailscale funnel --bg 8787
```

Then create a webhook in each target repo (Settings → Webhooks):
- Payload URL: your funnel URL + `/`
- Content type: `application/json`
- Secret: your `ISSUE_FACTORY_WEBHOOK_SECRET`
- Events: `Issues`

## Prepare a new repo

1. Apply labels from [ailuracollective/standards](https://github.com/ailuracollective/standards) (`.github/labels.yml`).
2. Register in T3: `t3 project add /path/to/repo`.
3. Create a webhook pointing to the funnel URL.

## Tests

```bash
PYTHONPATH=src pytest -q
```

## License

MIT
