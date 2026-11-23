# Daemon Drain, Resume, and Reload Semantics

Status: specification only. These commands are **not implemented yet**; runtime
support must land first. This document defines the contract for future
`factory daemon drain`, `factory daemon resume`, and `factory daemon reload`
commands so later implementation has explicit acceptance criteria.

## Scope

- `drain`: stop accepting new jobs; allow in-flight work to complete within a
  bounded deadline.
- `resume`: accept new jobs again after a drain.
- `reload`: apply safe configuration changes atomically; reject changes that
  require a full restart.

These are daemon-control operations. They do not cancel individual executions;
see `docs/cancellation.md`.

## Commands (future)

```text
factory daemon drain [--timeout <seconds>] [--json]
factory daemon resume [--json]
factory daemon reload [--config <path>] [--json]
```

All three follow the shared CLI JSON output contract in
`docs/json-output-contract.md`.

## Operational states

The daemon reports one of:

- `running` — accepting new jobs.
- `draining` — not accepting new jobs; in-flight work completing.
- `drained` — no in-flight work; ready to resume or stop.
- `reloading` — applying a validated config transaction.

State transitions:

- `running` -> `draining` (drain)
- `draining` -> `drained` (all in-flight complete or deadline)
- `drained` -> `running` (resume)
- `running` -> `reloading` -> `running` (reload success)
- `running` -> `reloading` -> `running` (reload rejected, state restored)

## Drain

Behavior:

1. Set state to `draining`; reject new job submissions with a stable error code
   (`DAEMON_DRAINING`).
2. Allow currently in-flight jobs to complete.
3. Bound total wait by `--timeout` (default 300s). When the deadline elapses,
   report remaining in-flight jobs; do **not** force-cancel them unless the
   operator separately invokes `factory cancel`.
4. Transition to `drained` when no in-flight jobs remain or the deadline
   expires.

Persistence/recovery:

- Drain state is persisted to the daemon state file so a crash/restart does not
  silently resume accepting jobs. On startup after an unclean shutdown while
  draining, the daemon starts in `drained` and requires explicit `resume`.

Concurrency:

- Drain is idempotent: repeated calls report current state.
- Concurrent drain/resume/reload requests are serialized by a daemon-side lock;
  the second request waits or returns `DAEMON_BUSY`.

Signal handling:

- SIGTERM during drain waits for the drain deadline, then exits; remaining jobs
  are left to be recovered on next start.
- SIGINT cancels the drain wait and exits promptly (jobs continue under
  supervisor).

Health/operational state:

- Health endpoint reports `state` and `in_flight_count`.
- `factory daemon status --json` includes `state`, `in_flight_count`,
  `drain_deadline`.

IPC security:

- Control socket/pipe requires the same user ownership as the daemon; no
  network exposure. Requests are authenticated by OS-level peer credentials
  where available.

## Resume

Behavior:

1. Only valid from `drained` (or `draining` with no in-flight jobs, which
   transitions directly).
2. Set state to `running`; accept new jobs.

Idempotent: calling resume while `running` reports current state.

## Reload

Behavior:

1. Validate the candidate config against the schema and runtime constraints.
2. Classify each changed key as `safe` (hot-reloadable) or `restart-required`.
3. If any `restart-required` key changed, reject with `RELOAD_REQUIRES_RESTART`
   and list the keys; no partial apply.
4. Otherwise apply all safe changes atomically under the daemon lock; on
   failure, roll back to the previous config and report `RELOAD_FAILED`.
5. Preserve in-flight jobs throughout.

Persistence/recovery:

- Persist both previous and candidate config versions for rollback. On startup,
  if a reload was interrupted, restore the last known-good config.

IPC security: same as drain.

## Acceptance criteria (for later implementation)

- [ ] `drain`, `resume`, `reload` subcommands exist with `--json`.
- [ ] State machine implemented and persisted.
- [ ] New job submission rejected while draining with stable error code.
- [ ] In-flight jobs complete or deadline reported; no forced cancellation.
- [ ] Reload rejects restart-required changes without partial apply.
- [ ] Reload rolls back on failure.
- [ ] Health/status expose state and in-flight count.
- [ ] IPC access restricted to owning user.
- [ ] Tests cover transitions, idempotency, concurrency, and recovery.
- [ ] Existing Factory workflows are not regressed.
