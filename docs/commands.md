# Mention Commands

Factory exposes seven commands through GitHub mentions. This document defines
their syntax, context restrictions, capabilities, execution states and
limitations. Command dispatch is transport-independent: `apps/cli/src/commands.rs`
holds the catalog, policy and state machine; `apps/cli/src/mentions.rs` supplies
the GitHub transport.

## Invocation

```text
@<configured-github-login> [<trigger-word>] <command> [validated-options]
```

With the default `mentions.command` of `factory`, all of these are valid:

```text
@your-login factory plan focus on rollback
@your-login implement add tests for the parser
@your-login review check the error paths
@your-login test
@your-login status
@your-login help
@your-login cancel
```

The trigger word is optional. A direct catalog command such as `@your-login plan`
also works. One command per invocation; ordinary mentions with no command are
ignored. The containing issue or pull request supplies the context.

`fix` is accepted as a deprecated alias for `implement` (the original mention
flow). Unsupported options, wrong contexts, ambiguous cancel targets and unknown
commands are rejected without executing anything.

## Command catalog

| Command | Context | Options | Capability | Behavior |
| --- | --- | --- | --- | --- |
| `help` | Issue / PR | `[command]` | `commands:read` | Lists only the commands the author's capabilities permit, with syntax and examples. Read-only. |
| `status` | Issue / PR | `[execution-id]` | `executions:read` | Shows execution ids, states, timestamps and agent thread ids for the current issue/PR. Details are redacted. |
| `plan` | Issue | `[guidance]` | `tasks:plan` | Reads the issue and conventions, publishes a technical plan. Never modifies source code. |
| `implement` | Issue | `[guidance]` | `tasks:implement` | Works in an isolated branch/worktree, implements the scope, runs configured format/lint/tests and opens or updates a PR. Never merges or deploys. |
| `review` | PR | `[guidance]` | `tasks:review` | Inspects the diff, checks and conventions and publishes findings. Never changes code, approves or merges. |
| `test` | Issue / PR | none | `tasks:test` | Runs only `mentions.test_allowlist` programs in the project directory and publishes the results. Rejects ambiguous targets by requiring the issue/PR context. |
| `cancel` | Issue / PR | `[execution-id]` | `executions:cancel` | Cancels a queued execution or requests cancellation of a running one. Only the requester or an administrator. |

Options are validated:

- Any `--flag` style option is rejected.
- `help` accepts at most one known command name.
- `status` and `cancel` accept at most one execution id (letters, digits, `-`, `_`).
- `test` accepts no arguments.
- `plan`, `implement` and `review` accept free-form guidance. It is treated as
  untrusted text and fenced inside the agent prompt; it is never executed as a
  shell command.

## Configuration

```json
{
  "mentions": {
    "enabled": true,
    "usernames": ["your-github-login"],
    "command": "factory",
    "allowed_authors": [],
    "admin_users": [],
    "poll_interval_secs": 60,
    "initial_lookback_secs": 300,
    "daily_quota": 20,
    "test_allowlist": ["pnpm test", "cargo clippy"],
    "execution_timeout_secs": 3600,
    "command_policy": {
      "write": [
        "commands:read",
        "executions:read",
        "tasks:plan",
        "tasks:implement",
        "tasks:review",
        "tasks:test"
      ],
      "maintain": [
        "commands:read",
        "executions:read",
        "tasks:plan",
        "tasks:implement",
        "tasks:review",
        "tasks:test",
        "executions:cancel"
      ],
      "admin": [
        "commands:read",
        "executions:read",
        "tasks:plan",
        "tasks:implement",
        "tasks:review",
        "tasks:test",
        "executions:cancel"
      ]
    },
    "report": true
  }
}
```

| Key | Meaning |
| --- | --- |
| `test_allowlist` | Programs the `test` command may run, split on whitespace. Empty disables `test` (fail closed). |
| `execution_timeout_secs` | Stale queued/running executions are recovered on the next poll. `0` disables recovery. |
| `admin_users` | Logins treated as Factory administrators. They may cancel any execution. |
| `command_policy` | Explicit mapping from repository role to capabilities. Unknown capability names are ignored. |

Author roles are resolved as follows, fail closed:

1. `admin_users` → `admin` (all configured admin capabilities).
2. `usernames` or `allowed_authors` → `owner` (all capabilities).
3. Otherwise the GitHub repository role from `gh api
   repos/{repo}/collaborators/{user}/permission`: `admin`, `maintain` or `write`
   map to the same-named policy; `read`, `triage`, missing users and lookup
   failures get no role.

Being mentioned is not permission to execute: the author is checked, not the
mentioned account. GitHub roles are never trusted implicitly; they only feed the
explicit `command_policy`.

## Execution state machine

```text
received -> validating -> queued -> running -> completed | failed
validating -> rejected
queued -> cancelled
running -> cancelling -> cancelled | failed

Terminal: completed, failed, rejected, cancelled
```

Every command invocation that reaches `validating` creates an execution record
with a UUIDv7 id, event identity/revision, actor, role, target issue/PR, command,
agent (`provider/model`), created/updated timestamps and an append-only
transition log (`executions`, `execution_transitions` in `factory.db`).

- Invalid options, missing capabilities and wrong contexts transition
  `validating -> rejected` and are reported on the issue/PR.
- Duplicate delivery is deduplicated by the event audit key
  (`repo:kind:id:body-hash`) and the `executions.event_key` unique constraint.
  A replay does not create a second run.
- `queued` cancellations complete immediately. `running` cancellations are
  best-effort: the runner either confirms the cancellation or the execution is
  marked `failed` and the outcome is reported. Cancellation of another author's
  execution is rejected unless the requester is an administrator.
- On each poll, executions older than `execution_timeout_secs` are recovered:
  `queued -> cancelled` and `running`/`cancelling -> failed`.
- `status` reports at most the last 10 executions for the current issue/PR and
  never echoes stored detail text.

## Security

- Repository allowlist, author role and per-command capability are checked
  before queueing.
- Bot-authored events, Factory's own report comments (`<!--
  factory:mention-report -->`), quoted (`>`) mentions and fenced code blocks are
  ignored, so report loops and copy-pasted examples cannot trigger work.
- Issue content is never interpolated into a shell. `test` runs only configured
  allowlisted programs directly (no shell), in the project directory.
- The test report redacts the configured webhook secret and gateway token and is
  truncated. Agent commands cannot merge or deploy.
- The daily quota applies to `plan`, `implement`, `review` and `test`; it never
  blocks `help`, `status`, `cancel` or admin recovery.

## Limitations

- The local `t3` CLI has no thread interrupt/stop command, so cancelling a
  running execution currently transitions it to `failed` with an honest report.
  Queued cancellations are definitive.
- `test` isolation is a project directory and a clean process invocation, not a
  container or cgroup sandbox.
- Agent commands wait for the agent to finish (`t3 ... --wait`), so polling and
  commands are processed sequentially.
- `plan` and `review` rely on the agent following the published constraints; the
  transport never runs its own code modifications for those commands.
- Agent tool restrictions and the no-merge/no-deploy rule are enforced by the
  agent prompt and the T3 runner configuration, not by a separate sandbox.
- Status links are execution and thread ids, not GitHub deep links per
  execution.
