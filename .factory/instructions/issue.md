# Issue execution

Read the issue and inspect the relevant code before making changes. Work in the provided worktree and branch, implement the requested change, and add or update focused tests.

Run the checks relevant to the change and report their actual results. Fix failures caused by the change; report unrelated or externally blocked checks accurately.

Before opening a pull request, run the repository's required local checks and confirm they pass. Then open the PR as a draft, following the repository's `AGENTS.md` and the organization conventions it links.

After pushing the PR branch, inspect its checks and reviews. Keep checking until every required check has completed; fix failures caused by the change, push the fixes, and check again. Do not report success or finish while required checks are pending or failing. If checks cannot complete because of an external blocker, state that clearly and include the PR URL. Do not merge the pull request.
