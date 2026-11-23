# Common Errors

Configuration pitfalls that break a repository or Factory setup when not done
correctly. Environment/toolchain issues (pnpm PATH, Engram setup, etc.) are
intentionally excluded — see the app READMEs for those.

## 1. Git / hooks / worktrees

### Hooks do not run in linked worktrees

- **Symptom:** commits land without `cargo fmt` / `clippy` / `rumdl` checks even though `.husky/pre-commit` exists and is executable.

- **Cause:** `core.hooksPath=.husky` is a *relative* path. Its resolution differs between the primary checkout and linked worktrees, so the hook is not found in worktrees.

- **Fix:** set an absolute path in the *local* (untracked) `.git/config`:

  ```text
  git config core.hooksPath /absolute/path/to/repo/.husky
  ```

  Verify both the primary checkout and each worktree resolve `pre-commit` to the same absolute path. Note this is machine-specific and not tracked, so re-apply after cloning/moving the repo.

### Git identity mismatched with the active `gh` account

- **Symptom:** commits/PRs attributed to the wrong author; GitHub may reject pushes or mislabel contributions.

- **Cause:** global `user.name` / `user.email` do not match the authenticated `gh` account.

- **Fix:** align global git identity with the active `gh` account, using GitHub's noreply email when the profile has no public email:

  ```text
  git config --global user.name "AiluraCheetah"
  git config --global user.email "338790863+AiluraCheetah@users.noreply.github.com"
  ```

## 2. Webhooks / Factory listener

### Webhook returns 401

- **Symptom:** GitHub deliveries (ping, `issue_comment`, etc.) come back `401`.
- **Cause:** the webhook secret configured in GitHub does not match Factory's local webhook secret; the listener validates `x-hub-signature-256` against the locally loaded config.
- **Fix:** synchronize the secret between the GitHub hook and `~/.config/ailuracollective/factory/config.json` (never print either value), restart the listener, and confirm a ping delivery returns `202`.

### Webhook points at a stale Tailscale domain

- **Symptom:** webhook is active but deliveries fail after a Funnel/domain change.
- **Cause:** the hook still targets the old Tailscale host.
- **Fix:** update the GitHub webhook URL to the current Funnel host and verify the health endpoint returns `200`.

### Only the last-tested repository's webhook works after switching repos

- **Symptom:** switching between repositories leaves only the most recently tested repo's webhook functional.
- **Cause:** `repo_add` can overwrite hook IDs and the `load_config`/`save_config` sequencing around `repos` (which is `#[serde(skip)]` but synced to SQLite) is not fully verified.
- **Fix (pending):** inspect `handle_payload` routing/filtering and the exact `repo_add`/`save`/`load` sequencing, reproduce with tests before changing behavior. Avoid creating duplicate webhooks by not re-adding an already-present repo.

### Shared listener ignores repo-local config

- **Symptom:** repo-local settings (e.g. `usernames_extra` in `test_bots/factory.json`) have no effect.
- **Cause:** the shared listener runs from the factory root and does not load repo-local `factory.json`.
- **Fix:** put shared trigger identities in the **global** config (`~/.config/ailuracollective/factory/config.json`) instead of relying on repo-local files.

### Global mention handler disabled (masked by cwd)

- **Symptom:** webhook delivery returns `202` but mentions are ignored.
- **Cause:** global `mentions.enabled` reverted to `false`; running `factory config get` from a repo cwd can apply local overrides and hide the underlying global value.
- **Fix:** verify the persisted **global** JSON directly, set `mentions.enabled=true` globally, restart the daemon, and confirm the effective config includes it.
