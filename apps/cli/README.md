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

- `~/.config/ailuracollective/factory/` (global config)
- `factory.json` in the repository root (repo config)

### Execution profiles

`factory.json` profiles are the exclusive source of harness and model selection.
Each named profile selects a T3 harness and a model slug that belongs to that
harness. There is no implicit fallback and no global/repository harness/model
default.

```json
{
  "profiles": {
    "default": { "harness": "pi", "model": "opencode-go/beam-free" }
  },
  "default_profile": "default",
  "defaults": {
    "base_branch": "main",
    "label": "status/ready",
    "max_attempts": 3
  }
}
```

- `profiles.<name>.harness` is the T3 harness instance/driver name (`pi`,
  `opencode`, `codex`, `claudeAgent`, etc.).
- `profiles.<name>.model` is a model slug exposed by that harness.
- `default_profile` selects the profile used when `--profile` is not passed.
- `factory agent run --repo <repo> --issue <n> --profile <name>` selects a
  named profile explicitly.
- `factory agent check --repo <repo> [--profile <name>]` validates the selected
  profile using the same resolver as execution.

Profile validation uses existing T3 discovery (`<T3CODE_HOME>/caches`, default
`~/.t3/caches`). Temporary discovery failures (unreadable/missing T3 cache) are
reported as temporary failures; invalid profile references or incompatible
harness/model pairs are rejected as configuration errors.

Legacy `defaults.harness`/`defaults.model` keys and legacy
`ISSUE_FACTORY_HARNESS`/`ISSUE_FACTORY_MODEL` environment overrides are not
supported. Remove them and configure named profiles in `factory.json`.

Non-model defaults (`base_branch`, `label`, `max_attempts`), mentions, and
labels remain supported.

### T3 catalog

- `factory agent harnesses` lists harnesses T3 reports as installed.
- `factory agent models <harness>` lists models that harness exposes.

### Supported runtime options

Execution runtime is selected per repository profile, not globally. Each agent
launch passes the selected profile's `harness` as T3 `--provider` and `model`
as `--model`, so per-session/runtime selection does not mutate global T3 or
Gentle Shell settings.

- **Pi**: use a profile with `"harness": "pi"` (or the installed T3 Pi harness
  instance id) and a model slug exposed by that harness. Verify the model with
  `factory agent models pi`.
- **Gentle Shell**: use a profile with the Gentle Shell harness instance id as
  reported by T3 (`factory agent harnesses`) and a compatible model slug exposed
  by that harness. Gentle Shell runtime options are configured by the installed
  Gentle Shell/T3 integration; Factory passes only the selected harness/model
  per session.

For per-session isolation, run separate agent executions with the desired
profile; Factory does not change global harness/model defaults.

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
