# SPM Cloud — Cloudflare adapter prototype

This directory is the official Cloudflare deployment prototype for protocol
`spm.cloud.v1`. It demonstrates the intended binding boundaries:

- Worker: HTTPS routing, bearer gate and instance discovery.
- D1: asset metadata, idempotency and catalog rows.
- R2: immutable asset objects keyed by SHA-256.
- Durable Object: one realtime WebSocket room per `scope_id`.

It is not yet the production account/identity/ACL implementation. In
particular, the prototype uses one bootstrap bearer (`SPM_CLOUD_ACCESS_TOKEN`)
and buffers an upload to calculate SHA-256. Production Cloud must replace that
with the full session/identity protocol and a bounded streaming upload/lease
pipeline before public traffic is enabled.

## Setup

```bash
npm install
npm run types
npx wrangler d1 create spm-cloud
npx wrangler r2 bucket create spm-cloud-assets
# Put the returned D1 id into wrangler.jsonc, then:
npm run types
npm run d1:migrate:remote
npx wrangler secret put SPM_CLOUD_ACCESS_TOKEN
npm run deploy
```

For local development use `npx wrangler dev`; D1/R2/DO state is local unless a
binding is explicitly configured for remote access. Do not commit `.dev.vars`
or a real database id/secret.

The Axum service at the repository root remains the cross-platform community
self-hosting backend. This adapter is a separate official deployment target;
it does not make SQLite or the local object directory suitable for Cloudflare.
