# Ailura Factory Bot

## Overview

The bot is a small notification adapter for Ailura Factory. It exists to keep **delivery concerns out of core automation**: producers emit notifications through a shared contract, and this service decides how to present them (for example posting to Discord or logging locally during development).

It runs an HTTP service (default port `8790`) and forwards notifications to a chat sink. If `DISCORD_WEBHOOK_URL` is set, messages are sent via Discord webhooks; otherwise output is written to stdout so you can iterate without external dependencies.

## Requirements

- Rust toolchain (Cargo)

## Run

From this directory or the repository root:

```bash
cargo run -p bot
```

Optional environment variables:

- `DISCORD_WEBHOOK_URL` — if set, send messages to this Discord webhook URL.
- `RUST_LOG` — standard `tracing` filter (optional).

The service listens on `0.0.0.0:8790` by default.

## More

- Root overview: [../../README.md](../../README.md)
- Additional notes: [`references/README.md`](references/README.md) (if present)
