# Ailura Factory Gateway

## Overview

The gateway is the cloud-facing entrypoint for Ailura Factory. It exists because GitHub webhooks must reach a reliable HTTPS URL and because coordinating runners/jobs is easier with **durable, centrally hosted state** than with a laptop that may be offline.

Implemented as a Cloudflare Worker (Rust, WebAssembly) with D1, it receives GitHub webhooks, manages runner registration and job flow, and exposes admin endpoints. This separation lets the local CLI focus on developer ergonomics while the gateway owns event intake and coordination contracts.

## Requirements

- Rust toolchain with the `wasm32-unknown-unknown` target
- [`worker-build`](https://github.com/cloudflare/workers-rs) (`cargo install worker-build`)
- [Wrangler](https://developers.cloudflare.com/workers/wrangler/) and Node.js (for local dev/deploy)

## Local development

1. Create a local env file:

   ```bash
   cp .dev.vars.example .dev.vars
   # fill values (do not commit .dev.vars)
   ```

2. Start the dev server:

   ```bash
   make dev
   # or: npx wrangler dev
   ```

## Build and deploy

- Build (wasm release): `make build`
- Deploy: `make deploy` (or `npx wrangler deploy`)

D1 bindings and secrets are configured in `wrangler.toml` and the Cloudflare dashboard. See comments in `wrangler.toml` and `.dev.vars.example` for required variables.

## More

- Root overview: [../../README.md](../../README.md)
