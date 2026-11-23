# Architecture — Gateway

## Role

Cloudflare Worker gateway (Rust, `wasm32-unknown-unknown`) that receives GitHub webhooks, authorizes requests, persists jobs in D1, and exposes HTTP APIs for runners/admin.

## Style

Layered architecture, aligned with Ohkami:

- **Entry/composition root**: `src/lib.rs` defines `#[ohkami::worker]` and composes routes.
- **HTTP services/handlers**: `src/services/` (`webhook`, `runner`, `admin`) plus response/auth-guard helpers.
- **Domain**: `src/domain/` pure auth, GitHub, and validation rules (no Worker HTTP/D1 runtime I/O).
- **Repositories**: `src/repos/` D1 access by aggregate/use case.
- **Infra**: `src/infra/` technical adapters (db, sea, env, time, audit, http).
- **Entities**: `src/entities/` SeaORM models.
- **Migrations**: `src/migration/` D1 schema migrations.

## Flow

```text
GitHub webhook / Runner / Admin
        -> Ohkami route (lib.rs)
        -> services/* handler
        -> domain/* (validation/authorization)
        -> repos/* + infra/* (D1/SeaORM/worker bindings)
        -> JSON response
```

## Rules

- Dependencies point inward: `services -> domain/repos/infra`; `domain` has no HTTP/D1 runtime dependencies.
- No Tokio/SQLx; use `worker` bindings + async/await.
- Missing/invalid auth configuration fails closed (`domain::auth`).
- Keep domain logic testable in native builds where possible (`cfg` wasm32 for runtime).
