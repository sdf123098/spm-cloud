# SPM Cloud — official Cloudflare service

This directory contains the official Cloudflare deployment for protocol
`spm.cloud.v1`. It demonstrates the intended binding boundaries:

- Worker: HTTPS routing, bearer gate and instance discovery.
- D1: asset metadata, idempotency and catalog rows.
- R2: immutable asset objects keyed by SHA-256.
- Durable Object: one realtime WebSocket room per `scope_id`.

Implemented API areas include account/session lifecycle, scopes and targets,
ACLs, appearance/animation compare-and-swap, entity bindings/observations,
offline approval and claim codes, and asset upload/catalog/download/visibility
with ACL checks. The official deployment currently uses a `workers.dev` origin.

This is deployed and suitable for controlled testing, but it is not yet a
completed production release. Account provisioning still requires an operator
using the bootstrap bearer (`SPM_CLOUD_ACCESS_TOKEN`); never give that secret to
players. The official deployment currently has no trusted Minecraft/Yggdrasil
identity provider configured. Durable Object realtime behavior has basic
authenticated WebSocket and size/backpressure guards, but full protocol event
recovery, lease/revocation semantics, and dual-client game validation remain
open. Upload hashing currently buffers a bounded request body. Custom domain,
production monitoring/alerting, load/cost validation, and recovery drills are
also release gates. Do not treat this status as a claim of production
readiness.

## Setup

```bash
npm ci
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

For an existing deployment, validate with `npm run typecheck` and
`npm run deploy:dry-run` before deployment. Apply reviewed D1 migrations before
releasing code that depends on them. Set secrets with Wrangler; do not put
secret values in this repository or operator manuals.

The Axum service at the repository root remains the cross-platform community
self-hosting backend. This adapter is a separate official deployment target;
it does not make SQLite or the local object directory suitable for Cloudflare.
