# References: Discord Bots in Rust

Brief research for this repository's MVP. Consulted November 22, 2026.

## How a bot works

A bot is an application created in the [Discord Developer Portal](https://discord.com/developers/applications), with its own credentials and configuration. Its process authenticates with the bot token and communicates with Discord through:

- **Gateway** (WebSocket): a persistent connection for receiving real-time events and, where applicable, sending events. A library maintains the connection, heartbeats, and recovery after disconnections.
- **HTTP API** (REST): operations such as sending messages and managing resources. Its usage limits must be respected; libraries usually handle rate limiting.
- **Interactions**: application commands, buttons, and menus. They can be received through the Gateway or, in another architecture, through a public HTTP endpoint with signature verification.

For a small bot that listens for events, the Gateway avoids exposing a web server.

## What to configure and comply with

1. Create the application and bot in the Developer Portal, obtain the token, and keep it secret. Do not commit it to the repository or print it in logs.
2. Install it on a test server using the installation/OAuth2 link, granting only the required scopes and permissions.
3. Choose the minimum **Gateway intents** needed for its features. Server message events require `GUILD_MESSAGES`; DMs require `DIRECT_MESSAGES`.
4. Reading message content requires the privileged `MESSAGE_CONTENT` intent, enabled in the portal. Discord may close the connection if an unauthorized or unconfigured privileged intent is requested. To avoid this requirement, prefer slash commands when appropriate.
5. Follow Discord's policies, permissions, API limits, and rate limits. Do not automate a regular user account (self-bot).

This repository's MVP uses `!ping`, so it needs `MESSAGE_CONTENT`; enable it under **Developer Portal → Application → Bot → Privileged Gateway Intents** before running it.

## Existing libraries and templates

| Option | Approach | Suitable for |
| --- | --- | --- |
| [Serenity](https://github.com/serenity-rs/serenity) | Rust library with a client, events, models, and Gateway; abstracts shards and reconnection. Official examples include a `!ping` bot. | A straightforward event-based MVP like this repository's. |
| [Poise](https://github.com/serenity-rs/poise) | Command framework built on Serenity; defines slash and text commands as Rust functions. | Bots where commands are central to the application. |
| [Twilight](https://github.com/twilight-rs/twilight) | Modular and flexible ecosystem of crates for the Gateway, HTTP, models, and cache. Its repository provides a `cargo-generate` template. | Projects that need to select components and control more of the architecture. |
| [discord-example-app](https://github.com/discord/discord-example-app) | Discord's official example, written in JavaScript and focused on HTTP interactions. | A reference for application and interaction flows; not a Rust template. |

## Recommended starting architecture

A small Rust binary:

- `main`: reads the token from the environment, creates the client, and starts the Gateway.
- An `EventHandler`: receives messages, ignores bots, and processes only the minimal command.
- Serenity handles the protocol, Gateway, and HTTP response delivery.

This MVP does not need a database, custom cache, command framework, HTTP server, sharding, or distributed deployment. Split into modules as the logic grows; add persistence only if the bot needs to retain state.

## Best practices

- Read secrets from environment variables; use a local `.env` only if it is excluded from Git.
- Request the minimum intents and permissions. For new commands, consider slash commands to avoid needing to read general message content.
- Ignore messages from other bots to prevent reply loops.
- Handle and log send/connection errors without revealing the token.
- Let the library handle rate limiting; do not hard-code limits as custom constants.
- Test first in a private server with the minimum permissions.

## Rust scaffolds and templates

See [`scaffolds.md`](scaffolds.md) for existing templates (cargo-generate, Serenity, Poise, Twilight, Shuttle) and selection criteria.

## Architecture: one bot or several

See [`bot-architecture.md`](bot-architecture.md) for the decision between a single modular bot and multiple specialized bots. Recommendation for Mochi: one modular bot; split only when there is a real permissions or ownership boundary.

## References

- Discord, [Building your first Discord Bot](https://discord.com/developers/docs/getting-started).
- Discord, [Overview of Discord Apps](https://docs.discord.com/developers/quick-start/overview-of-apps).
- Discord, [Gateway and Gateway Intents](https://docs.discord.com/developers/events/gateway).
- Discord, [Rate Limits](https://docs.discord.com/developers/topics/rate-limits).
- Serenity, [repository, README, and examples](https://github.com/serenity-rs/serenity).
- Poise, [repository and examples](https://github.com/serenity-rs/poise).
- Twilight, [repository and getting started guide](https://github.com/twilight-rs/twilight).
- Twilight, [official template](https://github.com/twilight-rs/template).
- Discord, [official application example](https://github.com/discord/discord-example-app).

## MVP decision

Use Serenity 0.12 and Tokio, with a single handler and `!ping` → `Pong!`. This is the shortest path from an official Rust example demonstrating the Gateway, intents, and replies; it avoids adopting a command framework until slash commands are needed.
