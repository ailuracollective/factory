# Cancellation

Factory supports safe, per-execution cancellation for local CLI, GitHub mentions, and connected runners.

## Command contract

```text
factory cancel <execution-id> [--force]
factory cancel --repo <owner/repo> --issue <number> [--force]
factory cancel --repo <owner/repo> --pr <number> [--force]

@<configured-github-login> cancel [execution-id]
```

- Prefer an explicit execution ID. Context-only selection succeeds only when exactly one cancellable execution matches; otherwise Factory returns an ambiguity error listing candidate IDs.
- Default cancellation targets one execution. Bulk/cascading cancellation is out of scope.
- `--force` first requests normal termination, then escalates after 10 seconds (SIGTERM/SIGKILL for Unix process groups; `taskkill /T` then `/F` for Windows process trees). Other platforms can terminate only the directly owned child process. It does not reset/clean worktrees, delete branches, or delete/close PRs.
- Repeated requests are idempotent and report existing cancellation state.
- `factory stop` remains daemon control; it does not cancel individual executions.

## Behavior

- **Queued**: marked cancelled before agent launch.
- **Running**: transitions to `cancelling`; the supervised process is signalled and confirmed as `cancelled` after exit. If confirmation cannot be obtained, Factory reports an unconfirmed/unknown outcome instead of false success.
- **Terminal** (`completed`/`failed`/`cancelled`): immutable; cancel requests report existing state.

## Authorization

- CLI: local operator invoking `factory cancel`.
- GitHub mentions: original execution requester or explicitly authorized mention admins (`mentions.usernames` / `mentions.allowed_authors`). Repository write permission alone is not sufficient.
- Gateway admin routes: existing `x-admin-token` guard; fail-closed behavior preserved.

## Safety and limitations

- Cancellation stops only the owned agent process group/tree on Unix and Windows; on other platforms it stops only the direct child. It does not kill unrelated processes.
- Worktrees, uncommitted changes, commits, branches, existing PRs, and audit records are preserved by default.
- Already-published GitHub effects are not rolled back automatically.
- Connected runners observe gateway cancellation via heartbeat; on network loss, runners do not report false cancelled success.
- Mention polling processes cancel mentions without waiting for running launch jobs to finish.
