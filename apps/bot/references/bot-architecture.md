# Discord Bot Architecture: One vs. Multiple Bots

Research on whether to have several small bots with defined responsibilities or one super-bot with many features.

## Short answer

By default, use a single bot with a modular monolith architecture. Split into multiple bots only when there is a real boundary in permissions, trust, branding, or operational ownership.

## Our context

- Initial bot: notify a server (under construction) when something happens in a coding agent.
- More features are planned for the future.
- One server for now; MVP in Rust with Serenity.

## Option A - One modular bot (recommended to start)

A single bot identity and codebase, with well-separated modules (notifications, commands, moderation, etc.).

Advantages:
- One token, one invitation, and one set of permissions to manage.
- Centralized configuration, deployment, and troubleshooting.
- Simple shared state (one Gateway connection, one database).
- Easier to iterate on an MVP.

Disadvantages:
- A failure or heavy workload can affect everything.
- All modules share the same Gateway rate limits.
- All features have the same permissions.

## Option B - Multiple specialized bots

Each major feature has its own bot (application, token, and identity).

Advantages:
- Clear permission and trust boundaries (e.g., an admin bot vs. a notification bot).
- Independent releases, uptime, and ownership.
- One bot going down does not take other features with it.
- Different teams can own different bots.

Disadvantages:
- Multiple tokens, invitations, and per-server configurations.
- Duplicated infrastructure, monitoring, and command management.
- Bots may need coordination or shared data.
- Users see multiple bot identities in the server.

## When to split into multiple bots

Only when there is a concrete operational reason:

1. Different permissions or trust boundaries (e.g., one bot with admin permissions and another read-only).
2. Different branding or audiences (e.g., a public support bot vs. an internal CI bot).
3. Independent releases or uptime (one feature must not affect others).
4. Separate ownership (different teams maintain each bot).
5. Isolated workloads (heavy processing must not block the Gateway).

## Scaling: do not confuse more bots with sharding

- Discord requires sharding at 2,500+ servers. Sharding applies to the same bot; it does not mean creating new bots.
- For heavy workloads (AI, media, scheduled jobs), use a background worker queue rather than another bot.
- If you only need slash commands/components (without Gateway events), Discord allows an HTTP interactions endpoint, which can simplify things.

## Recommended evolution order

1. One process, with modular code by feature.
2. Move slow work to background workers (queue).
3. Shared database and idempotent handlers.
4. Gateway sharding when the number of servers requires it.
5. Split into a new bot only if a real permissions, ownership, or scaling boundary emerges.

## Low-cost alternative: webhooks

If a feature only needs to send messages to a channel, a webhook is simpler than a full bot. Ideal for CI/CD notifications.

## Rule of thumb

Start with a modular bot. Split only when you can point to a concrete operational or security benefit, not just because the feature list is growing.

## Sources

- Discord Gateway docs (sharding): https://github.com/discord/discord-api-docs
- Discord Interactions overview (HTTP endpoint): https://github.com/discord/discord-api-docs
- Discord Bot users / OAuth2: https://github.com/discord/discord-api-docs
- Comunidad: r/Discord_Bots, r/Discordjs (consenso: modular monolith primero)

## Decision for Mochi

For the current MVP (notifications and future features on one server, with one owner), a single modular bot is recommended. Extract a second bot only if a future feature requires different permissions or separate ownership (e.g., an administration bot with elevated permissions). Simple CI notifications could use a webhook instead of a bot.
