# Mochi Discord Bot

Minimal Rust MVP: listens for `!ping` and replies with `Pong!`.

## Requirements

- Rust and Cargo installed.
- An application/bot created in the [Discord Developer Portal](https://discord.com/developers/applications).
- **Message Content Intent** enabled on the portal's **Bot** page.
- Bot invited to a test server with permission to view and send messages.

## Run

Set `DISCORD_TOKEN` to the bot token. Do not share it or commit it to Git.

```sh
cp .env.example .env
# Edit .env and add the token, then export the variable:
set -a
. ./.env
set +a
cargo run
```

You can also export `DISCORD_TOKEN` directly in your environment and run `cargo run`.

Research on how bots work, requirements, Rust options, and architecture is in [`references/README.md`](references/README.md).
