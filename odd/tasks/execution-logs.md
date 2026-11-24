# Execution logs

Feature: GitHub issue #65 - `factory execution logs <id> [--follow] [--lines <n>]`
Status: done

## Specs

- S1: "Users can inspect available logs for an individual execution."
  Scope: "`factory execution logs <id> [--follow] [--lines <n>]`, covering local and Gateway-origin executions where logs are available."
- S2: "Concurrent executions have distinguishable log streams."
- S3: "History respects the requested line limit; follow emits subsequent records without repeating history."
- S4: "Interrupting follow does not cancel the execution."
- S5: "Unknown executions, unavailable logs and empty logs are distinguishable."
- S6: "Credentials are not exposed and log access respects applicable authorization."
- S7: "Retention and remote availability limitations are documented."

## Tasks

| ID | Specs | Route | Commit | Status |
|---|---|---|---|---|
| T1 | S1,S2,S3,S6 | inline | bdbac2e | done |
| T2 | S1,S3,S4,S5 | inline | bdbac2e | done |
| T3 | S1-S6 | inline | bdbac2e | done |
| T4 | S7 | inline | bdbac2e | done |

## Log

### L1 - User request (verbatim)

```text
Resolve GitHub issue #65: https://api.github.com/repos/ailuracollective/factory/issues/65

# Issue execution

Read the issue, the applicable `AGENTS.md`, and the organization conventions linked from it; inspect the relevant code before making changes. Work in the provided worktree and branch, implement the requested change, and add or update focused tests.

Issue and fresh mention/comment launches must receive the repository's applicable `AGENTS.md` and linked organization conventions in their prompt. On a same-thread follow-up, read and follow those base instructions and conventions before continuing, even if the follow-up prompt omits their text.

Run the checks relevant to the change and report their actual results. Fix failures caused by the change; report unrelated or externally blocked checks accurately.

Before opening a pull request, validate its Conventional Commit title, exactly one allowed `type/*` label, closing link to a `status/ready` issue, and required headings from the matching PR template. Run the repository's required local checks and confirm they pass. Open the PR as a draft with the matching template, then mark it ready for review before awaiting CI.

After marking the PR ready, inspect its checks and reviews using GitHub's check runs/statuses and branch protection or ruleset configuration to determine the applicable required checks. `gh pr checks` reporting no checks is unresolved, not success: keep observing until checks appear. A draft-skipped check is not a successful check. Pending, failed, cancelled, or blocked required checks are non-success. Fix failures caused by the change, push fixes, and check again. If an external blocker prevents checks from completing, report the blocker and PR URL, but leave the issue/task open and unresolved. Only declare completion once all applicable required checks have succeeded; do not infer required checks or branch protection without verifying repository configuration. Do not make a successful `t3 thread start/send --wait` result alone complete an issue-triggered execution: it only confirms the agent thread ended, not that GitHub checks passed.

If you start a PR observer, let it stop itself when all required checks pass. Do not restart it after its green-check notification. Do not merge the pull request.

Issue body:

### Context

Service-level daemon logs do not provide an execution-specific history.

### Goal

Users can inspect available logs for an individual execution.

### Scope

`factory execution logs <id> [--follow] [--lines <n>]`, covering local and Gateway-origin executions where logs are available.

### Acceptance criteria

- [ ] Concurrent executions have distinguishable log streams.
- [ ] History respects the requested line limit; follow emits subsequent records without repeating history.
- [ ] Interrupting follow does not cancel the execution.
- [ ] Unknown executions, unavailable logs and empty logs are distinguishable.
- [ ] Credentials are not exposed and log access respects applicable authorization.
- [ ] Retention and remote availability limitations are documented.

### Non-goals

Daemon-wide log management.

### References

#63.
```

### L2 - Implementation note

- Implemented execution log capture in CLI agent supervisor, per-execution log files under state logs dir, and `factory execution logs` command.
- Verified with `cargo fmt --all -- --check` (clean after fmt), `cargo clippy -p cli --all-targets -- -D warnings`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (cli 80 passed; gateway 11 passed; notifications 5 passed; doc-tests passed).
- Commit slice is 817 authored changed lines (single coherent feature); recorded for PR review.
