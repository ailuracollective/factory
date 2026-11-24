# Ailura Factory Gateway

## Overview

The gateway is the cloud-facing entrypoint for Ailura Factory. It exists because GitHub webhooks must reach a reliable HTTPS URL and because coordinating runners/jobs is easier with **durable, centrally hosted state** than with a laptop that may be offline.

Implemented as a Cloudflare Worker (Rust, WebAssembly) with D1, it receives GitHub webhooks, manages runner registration and job flow, and exposes admin endpoints. This separation lets the local CLI focus on developer ergonomics while the gateway owns event intake and coordination contracts.

## Requirements

- Rust toolchain with the `wasm32-unknown-unknown` target
- Node.js and pnpm

## Commands

Run these from `apps/gateway`:

| Command | Purpose |
| --- | --- |
| `pnpm install` | Install Wrangler and project dependencies |
| `pnpm dev` | Start Wrangler's local server at `http://localhost:8787` |
| `pnpm start` | Alias for `pnpm dev` |
| `pnpm test` | Run Gateway Rust tests |
| `pnpm lint` | Run Clippy for the WASM target |
| `pnpm format` | Format Rust code |
| `pnpm format:check` | Check Rust formatting without changing files |
| `pnpm build` | Build the release WASM package |
| `pnpm clean` | Clean Cargo build artifacts |

## Local development

1. (Optional) Create a local env file if you need local secrets:

   ```bash
   cp .dev.vars.example .dev.vars
   # fill values (do not commit .dev.vars)
   ```

2. Start the dev server:

   ```bash
   pnpm dev
   ```

   The Gateway is available at `http://localhost:8787`. Wrangler uses a local D1 simulation, isolated from the remote database.

`.dev.vars` is ignored by Git; never commit secrets.

## Execution logs

Execution logs are captured by the CLI runner on the machine that executes a job.
The Gateway stores job metadata and status, but it does not currently store or
serve execution log streams. Remote log availability is therefore limited to the
runner/local machine that holds the log files; retention is local and file-based
until removed by the operator or state directory cleanup.

## More

- Root overview: [../../README.md](../../README.md)
