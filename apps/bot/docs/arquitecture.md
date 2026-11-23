# Architecture — Bot

## Role

Notification receiver bot: receives `notifications::Notification`, validates it, and renders it to a chat channel.

## Style

Hexagonal (ports & adapters), small and modular.

- **Composition root**: `src/main.rs` selects adapters by env and wires the Axum server.
- **Inbound adapter / transport**: `src/transport/http.rs` exposes `/health` and `/notify`.
- **Domain**: `src/domain/handler.rs` orchestrates the use case; `src/domain/ports.rs` defines outbound ports (`ChatSink`, `ChatError`).
- **Outbound adapters / chat**: `src/chat/` implements `ChatSink` (`stdout`, `discord`; extensible to Slack/Telegram/webhook).

## Flow

```text
HTTP/Firebase -> transport::http -> domain::handler
                                  -> validate(Notification)
                                  -> ChatSink (port)
                                  -> chat adapter (stdout/discord/...)
```

## Rules

- The domain does not depend on Axum, Discord, or transport details.
- Adapters implement domain ports.
- `main.rs` is the only concrete wiring point.
- The shared contract lives in `internal/notifications`.
