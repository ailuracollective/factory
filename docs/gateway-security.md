# Gateway Security

The Gateway fails closed. If required authentication or authorization
configuration is missing or invalid, requests are rejected and no jobs are
created.

## Required secrets

Set these with `wrangler secret put <NAME>`. Do not put them in
`wrangler.toml` or any tracked file.

| Secret | Purpose |
| --- | --- |
| `GITHUB_WEBHOOK_SECRET` | HMAC key for `x-hub-signature-256`. An empty value rejects every webhook. |
| `GITHUB_TOKEN` | GitHub App installation token (preferred) or fine-grained PAT used to verify repository access and actor permissions. Missing value rejects every webhook. |
| `ADMIN_TOKEN` | Bearer value for every `/admin/*` route. Missing value returns `503`; wrong/missing token returns `401`. |
| `RUNNER_BOOTSTRAP_SECRET` | Required `x-bootstrap-secret` value for `/runners/register`. Missing value rejects registration. |

Optional configuration:

| Variable | Purpose |
| --- | --- |
| `GITHUB_ALLOWED_REPOS` | Comma-separated `owner/repo` allowlist. When empty, repository scope is limited by the GitHub token. |
| `GITHUB_INSTALLATION_ID` | GitHub App installation id. When set, webhooks from any other installation are rejected. |

## Fail-closed behavior

- Webhooks require a valid signature and a configured webhook secret.
- Webhooks require `GITHUB_TOKEN`; repository and actor permission lookups are
  mandatory. API failures return `503`; missing/insufficient permission returns
  `403`. An empty `allowed_users` policy never bypasses permission checks.
- Webhook repository scope is validated against `GITHUB_ALLOWED_REPOS` (when
  set), the optional installation id, and a live GitHub repository lookup.
- Every admin route, including cancellation, goes through the same token guard.
- Runner registration requires the bootstrap secret; tokens are generated with
  the platform CSPRNG (`crypto.getRandomValues`) and only their SHA-256 hash is
  stored in D1.

## Runner tokens

- Format: `frt_` followed by 64 hex characters from 32 random bytes.
- Tokens are returned once on registration or rotation. They are never logged
  or stored in plaintext.
- Rotate: `POST /admin/runners/:id/rotate` with `x-admin-token`. Returns a new
  token and invalidates the previous one.
- Revoke: `POST /admin/runners/:id/revoke` with `x-admin-token`. Revoked tokens
  cannot claim, heartbeat, complete, or fail jobs.
- Migration `0003_runner_tokens` replaces the old plaintext `token` column and
  invalidates existing tokens. Re-register runners after applying it.

## Local development

Copy [apps/gateway/.dev.vars.example](../apps/gateway/.dev.vars.example) to
`apps/gateway/.dev.vars` and replace the placeholder values. `.dev.vars` is
git-ignored. For deployed environments, always use Cloudflare secrets.
