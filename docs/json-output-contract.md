# Factory CLI JSON Output Contract (schema v1)

## Envelope

Success:

```json
{"schema_version":1,"data":...,"meta":{"count":0,"limit":20}}
```

- `schema_version`: integer, currently 1.
- `data`: command-specific array or object.
- `meta`: optional, only applicable metadata (count, limit, next_cursor).

## Naming, timestamps, nullability

- Keys are `snake_case`.
- Timestamps RFC 3339 UTC.
- Nullable fields explicit (Option) and omitted when None where applicable.
- Stable identifiers and canonical status values.

## Streams and exit codes

- One JSON document on stdout per invocation.
- No progress/ANSI/diagnostics mixed into JSON stdout.
- Human output independent.
- Failures: nonzero exit, actionable stderr, stable machine-readable error codes.
- For `--json`, errors emitted as JSON error shape on stderr? Consistently: error JSON to stderr, exit nonzero. (Documented.)

Error shape:

```json
{"schema_version":1,"error":{"code":"NOT_FOUND","message":"..."}}
```

## Compatibility

- Additive optional fields in v1 allowed.
- Incompatible structural/semantic changes require version bump.

## Redaction

Never expose secrets, tokens, prompts, credentials, sensitive env values. Centralized `redact`.

## Pagination

Design supports lists, single resources, paginated results via meta.next_cursor.

```text
```

Build/test.<|end|><|start|><|start|>assistant to=shell<|channel|>analysis <|constrain|>xml<|message|><rfl:parameter name="command">
cd /projects/ailuracollective/factory && cargo test -p cli 2>&1 | tail -30
