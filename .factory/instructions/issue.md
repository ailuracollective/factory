# Issue execution

Read the issue, the applicable `AGENTS.md`, and the organization conventions linked from it; inspect the relevant code before making changes. Work in the provided worktree and branch, implement the requested change, and add or update focused tests.

Issue and fresh mention/comment launches must receive the repository's applicable `AGENTS.md` and linked organization conventions in their prompt. On a same-thread follow-up, read and follow those base instructions and conventions before continuing, even if the follow-up prompt omits their text.

Run the checks relevant to the change and report their actual results. Fix failures caused by the change; report unrelated or externally blocked checks accurately.

Before opening a pull request, validate its Conventional Commit title, exactly one allowed `type/*` label, closing link to a `status/ready` issue, and required headings from the matching PR template. Run the repository's required local checks and confirm they pass. Open the PR as a draft with the matching template, then mark it ready for review before awaiting CI.

After marking the PR ready, inspect its checks and reviews using GitHub's check runs/statuses and branch protection or ruleset configuration to determine the applicable required checks. `gh pr checks` reporting no checks is unresolved, not success: keep observing until checks appear. A draft-skipped check is not a successful check. Pending, failed, cancelled, or blocked required checks are non-success. Fix failures caused by the change, push fixes, and check again. If an external blocker prevents checks from completing, report the blocker and PR URL, but leave the issue/task open and unresolved. Only declare completion once all applicable required checks have succeeded; do not infer required checks or branch protection without verifying repository configuration. Do not make a successful `t3 thread start/send --wait` result alone complete an issue-triggered execution: it only confirms the agent thread ended, not that GitHub checks passed.

If you start a PR observer, let it stop itself when all required checks pass. Do not restart it after its green-check notification. Do not merge the pull request.
