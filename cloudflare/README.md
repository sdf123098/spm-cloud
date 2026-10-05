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
with ACL checks. The official deployment uses `https://micafic.xyz` as its
canonical origin, with a Workers Custom Domain managed by Wrangler. The existing
`workers.dev` endpoint stays enabled for older clients. Instance discovery and
Minecraft certificate challenges retain the explicitly configured legacy origin
when called through that endpoint; arbitrary request hosts cannot change signing
origins. Updated clients migrate saved official instance metadata automatically.

This is deployed and suitable for controlled testing, but it is not yet a
completed production release. This Worker permits public account registration;
the bootstrap bearer (`SPM_CLOUD_ACCESS_TOKEN`) remains an operator secret and
must never be given to players. Game identity providers are configured by D1
migrations and operator settings; check the enabled provider list on the running
instance. Durable Object realtime behavior has basic
authenticated WebSocket and size/backpressure guards, but full protocol event
recovery, lease/revocation semantics, and dual-client game validation remain
open. Upload hashing currently buffers a bounded request body.
Production monitoring/alerting, load/cost validation, and recovery drills are
also release gates. Do not treat this status as a claim of production
readiness.

## External identity providers / 外置认证

Both the Rust backend and this Worker accept a complete `has_joined_url` in the
operator-authenticated `POST /v1/identity-providers` request:

```json
{
  "provider_id": "my-auth",
  "display_name": "My external login",
  "has_joined_url": "https://auth.example.com/all-in-one/hasJoined",
  "enabled": true
}
```

An all-in-one gateway can be configured once. `base_url` with an optional
`session_path` remains supported; do not combine that form with `has_joined_url`.
Keep existing provider IDs when updating their endpoint. Only the operator can
add trusted services; players must verify and bind their game identities before
automatic game-account login.

Rust startup variables `SPM_CLOUD_HAS_JOINED_URL` and
`SPM_CLOUD_IDENTITY_PROVIDERS` configure the Rust process, not this Worker.
Configure the Worker through its operator API, then update the running service
with the deployment workflow below. Pushing source to GitHub does not deploy a
Worker. Self-hosted configuration does not add a provider to the official
instance. Rust setup: [English](../README.md) / [中文](../README_zh.md).

Rust 与官方 Worker 的管理员接口均支持完整 `has_joined_url`，all-in-one 网关
只需配置一次。运营者启用认证服务、玩家完成首次身份验证与绑定后才能自动登录。
两个环境变量用于 Rust 启动配置，Worker 通过管理员接口配置；源码推送与线上
Worker 发布是独立步骤。自建实例的配置不会自动写入官方实例。

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

## Player animation synchronization and existing deployments

Player wheel animations, explicit stop events, synchronized Molang variables and
expressions, and automatic idle controller states use the player appearance API.
They require both an updated client and a backend advertising
`player_motion_v1` in `GET /v1/instance`. This works for the official deployment
and independently configured community Cloud instances. A successful local
build or dry run does not update an existing online service.

For an existing Cloudflare deployment, apply all pending D1 migrations through
`0009_player_motion.sql` before deploying the updated Worker. The migration adds
the identity-scoped `player_motion` table; it preserves existing appearance data.
For your own Worker, use your own Worker configuration, database, R2 bucket,
instance ID and origin. The checked-in configuration targets the official
service, so pass your configuration explicitly to every command:

```bash
npm ci
npx wrangler deploy --dry-run --config <your-worker-config>
npx wrangler d1 migrations apply <your-database-name> --remote --config <your-worker-config>
npx wrangler deploy --config <your-worker-config>
```

After deployment, request your own origin's `/v1/instance` and verify that
`capabilities` contains `player_motion_v1`. Then check with two updated clients
that wheel playback, stop, model settings and idle states are visible to the
other player. Private models remain local. Older clients that omit `motion`
clear the previous action on their next appearance update.

Community servers running the Rust backend must update and restart that backend
instead. Its SQLite motion table is created during startup; Wrangler D1
migrations apply only to this Cloudflare adapter. Keep each instance's existing
identity and storage configuration when upgrading.

## Account login and game identity binding

Instance discovery advertises `game_identity_auth_v1` and an `auth` object with
`password_login`, `game_identity_login`, `game_identity_link`, and
`self_registration`. All four are true for this Worker. The Rust backend defaults
to open registration on new installations and reports its configured policy;
an explicit disabled setting remains valid.
Clients should use these capabilities on each selected instance, including a
self-hosted instance. The `official` identity provider refers to a Minecraft
identity proof; it does not restrict the Cloud instance to the official service.

Password login authenticates a Cloud account but does not bind a game identity.
After login, obtain and complete an authenticated `/v1/auth/challenges` proof for
the current game profile, then refresh `/v1/identities` and verify its `VERIFIED`
status before enabling player synchronization. Subsequent game identity login
uses `/v1/auth/login-challenges`. Proof completion must include the matching
`challenge_id`; a verified identity already belonging to another account cannot
be claimed. Proof payloads remain bound to the configured Cloud origin.

Refresh tokens rotate atomically. Refreshing invalidates the previous access
and refresh tokens; replay returns `401 REFRESH_REUSED`. Expired sessions return
`401 SESSION_EXPIRED`. Logging out the current session invalidates its two
tokens without changing another account's sessions. A replacement insertion
failure rolls back revocation, so the original session remains usable.

Run `npm run test:login-parity` to check both official and self-hosted instance
IDs against real local Worker routing, sessions and D1, with fixture provider
network responses. This test covers password login followed by binding, game
identity login, replay prevention, account isolation and discovery capabilities.
