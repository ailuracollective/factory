# Standalone Mentions (Gateway-free)

Factory can watch GitHub for mentions of your own account and launch an agent
locally. This works without the Gateway and without GitHub notifications, so a
maintainer can trigger work by mentioning their own username (which GitHub does
not notify).

## Configuration

Add a `mentions` block to `~/.config/ailuracollective/factory/config.json`
(the file created by `factory init`):

```json
{
  "mentions": {
    "enabled": true,
    "usernames": ["your-github-login"],
    "command": "factory",
    "allowed_authors": [],
    "daily_quota": 20,
    "report": true
  }
}
```

| Key | Meaning |
|---|---|
| `enabled` | Turn mention polling on. Existing configs default to `false`. |
| `usernames` | Trigger identities, without `@`. Case-insensitive. Your own login is enough for self-mentions. |
| `command` | Explicit command required after the mention. Empty means any mention triggers. |
| `allowed_authors` | Extra logins allowed to trigger. Others are checked for repository write permission. |
| `daily_quota` | Maximum launches per rolling 24 hours. `0` disables the quota. |
| `report` | Comment the outcome back on the issue/PR. |

Invocation syntax (with the default command):

```text
@your-github-login factory implement this
```

Ordinary mentions such as `@your-github-login can you look at this later?` do
**not** launch work because the command token is missing. The text after the
command is untrusted GitHub content and is passed to the agent as guidance, not
executed as a shell command.

## Usage

Webhook mode: `factory daemon run --mode listener` processes `issue_comment`, `issues`,
`pull_request`, and `pull_request_review_comment` mention events in real time,
reusing the same parser, authorization, dedup, and dispatch logic. No polling
delay. A public webhook endpoint (tunnel/GitHub App/gateway) is required.

## Permissions

An event is accepted only when all of these hold:

1. The repository is registered and enabled (`factory repo add`, `factory repo
   list`). Repositories outside the allowlist are rejected.
2. The event author is authorized: they are a configured trigger username, an
   `allowed_authors` entry, or have `admin`, `maintain` or `write` permission on
   the repository as reported by `gh api .../collaborators/{user}/permission`.
   Permission lookups fail closed.
3. The only supported operation is `fix` (launch an agent to resolve the issue
   or pull request). Text that does not match the invocation syntax is ignored;
   rejected requests are audited.

Being mentioned is not permission to execute: the author is checked, not the
mentioned account.

## Deduplication and restarts

- Duplicate events are filtered by audit key `repo:kind:id:body-hash`, persisted
  in `mention_events`, so a webhook replay does not execute the same revision
  twice. The body hash is used because an issue's `updated_at` also changes when
  other people comment.
- Editing an issue or comment body changes its hash, so the edited revision is
  evaluated again. Title-only edits are not re-evaluated.
- Bot-authored events (`user.type == "Bot"` or logins ending in `[bot]`) are
  ignored. Factory's own status comments carry `<!-- factory:mention-report -->`
  and never trigger work, so bot loops cannot form.
- Accepted, rejected, completed and failed requests are audited in
  `mention_events` with the reason and, on success, the agent thread id.

## Reporting

With `report: true`, Factory posts a comment on the issue/PR after a launch
succeeds or fails. It uses the existing authenticated `gh` session, so the
credential only needs issue-write access on the allowlisted repositories. No
new GitHub App or bot account is required.

## Limitations

- Webhook mode needs a reachable endpoint (tunnel/GitHub App/gateway).
- Inline PR review comments (`pull_request_review_comment`) are handled by
  webhooks.
- Requests from pull request conversation comments are dispatched as an
  issue-style agent run with a note to inspect the PR; there is no special
  "update the existing PR branch" flow yet.
- The authenticated `gh` user authors the report comments, so they look like
  the maintainer's own comments (they are marked and ignored as triggers).
- A GitHub outage or missing `gh` auth skips that repository for the poll; the
  cursor is not advanced past events that were never fetched.

## Migration path to Gateway

Mention events are processed from webhooks in real time via `factory daemon run --mode listener`.
Parsing, authorization, deduplication, auditing and dispatch live in
`mentions::Engine`. A Gateway-backed deployment can forward webhook events to the
runner over a push channel instead of polling.
