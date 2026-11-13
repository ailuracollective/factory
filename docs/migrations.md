# D1 Migrations (Gateway)

Location: `apps/gateway/migrations/*.sql`

Conventions:
- Files are named `NNNN_description.sql` with zero-padded 4-digit version.
- Each migration is registered in `MIGRATIONS` in `apps/gateway/src/lib.rs` as `(version, name, include_str!(...))`.
- Versions must be strictly increasing and unique.
- `apply_migrations` creates `schema_migrations` and applies only missing versions.
- Migrations are idempotent at file level only when written with `IF NOT EXISTS`; the runner also skips applied versions.

Adding a migration:
1. Create `apps/gateway/migrations/NNNN_description.sql`.
2. Append an entry to `MIGRATIONS`.
3. Run `pnpm --filter gateway test` (validates ordering/uniqueness).
4. Run `pnpm --filter gateway build`.
