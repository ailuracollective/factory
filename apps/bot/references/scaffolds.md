# Scaffolds and Templates for Discord Bots in Rust

Research on existing templates and scaffolds for starting a bot in Rust.

## cargo-generate (standard method)

`cargo-generate` creates projects from Git template repositories with placeholders.

Install: `cargo install cargo-generate`

Typical usage: `cargo generate <repo-url>` or `cargo generate gh:<owner>/<repo>`.

Find templates with the `cargo-generate` topic on GitHub: https://github.com/topics/cargo-generate

## Recommended templates by stack

### Serenity (events + Gateway) - recommended for our MVP
- `twilight-rs/template` (cargo-generate, modular, Gateway session, shards, cache, tracing, config, graceful shutdown) - https://github.com/twilight-rs/template
- `elijah-potter/discord-bot-template` (Serenity, archivado) - https://github.com/chilipepperhott/discord-bot-template
- Serenity ejemplos oficiales (incluye `!ping`) - https://github.com/serenity-rs/serenity

### Poise (slash commands on Serenity)
- `noxcraftdev/discord-bot-template-rs` (poise + sqlx + SQLite + tracing + dotenvy, production-ready) - https://github.com/noxcraftdev/discord-bot-template-rs
- `jackskelt/poise-bot-template` (Poise + Serenity slash commands, archivado) - https://github.com/jackskelt/poise-bot-template
- `Coca162/rust-bot-template` (Poise + SQLx postgres, over-commented) - https://github.com/Coca162/rust-bot-template
- `varubogu/poise-postgres-bot-template` (Poise + Postgres + Docker multi-arch) - https://github.com/varubogu/poise-postgres-bot-template

### Feature-rich (serenity + poise + songbird)
- `cycle-five/bot-template-rs` (feature flags to enable capabilities: songbird, lavalink) - https://github.com/cycle-five/bot-template-rs

### Twilight (modular, scalable)
- `twilight-rs/twilight` (crate ecosystem: gateway, http, model, cache) - https://github.com/twilight-rs/twilight
- `twilight-rs/template` - https://github.com/twilight-rs/template

### Shuttle (easy deployment)
- `shuttle init --template serenity` (Hello World Bot example) - https://docs.shuttle.dev/examples/serenity

## Selection criteria for Mochi

Our current MVP uses Serenity 0.12 + Tokio with `!ping` and MESSAGE_CONTENT. If we want to migrate to a scaffold:

- To keep using simple events: stick with Serenity alone or `twilight-rs/template`.
- For slash commands: Poise (noxcraftdev or cycle-five).
- If we need audio/voice: cycle-five (songbird).
- For managed deployment: Shuttle.

Recommendation: do not migrate now. The current MVP is sufficient and minimal. Evaluate a scaffold when the first real feature appears (slash commands or persistence), and prefer Poise + sqlx if a database is needed.

## Note

Several templates are archived or maintained by a single contributor. Check the latest commit date and compatibility with the Rust/Serenity versions before adopting one.

## Ready-to-use "create" CLIs (create-react-app style)

### create-discord-bot (crates.io)
- Crate: https://crates.io/crates/create-discord-bot
- Repo: https://github.com/saumsy/create-discord-bot
- Install: `cargo install create-discord-bot`
- Generates boilerplate in seconds. ~4k downloads. Last updated 2025-03.

### RustDisco
- Repo: https://github.com/keiveulbugs/RustDisco
- Install: `cargo install --git https://github.com/keiveulbugs/RustDisco`
- Deploys templates needed to get started.

### discordbot_init (estilo create-react-app)
- Repo: https://github.com/daminals/discordbot_init
- CLI similar to create-react-app for initializing a Discord bot. Rust is among the supported languages.

## Recommendation

Our existing MVP does not need a CLI. If starting from scratch today, `create-discord-bot` (crates.io) is the most direct option for Rust; `cargo-generate` with `twilight-rs/template` is more flexible and better maintained.
