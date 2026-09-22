import { ScopeRoom } from "./scope-room";

export { ScopeRoom };

type RuntimeEnv = Env & { SPM_CLOUD_ACCESS_TOKEN: string };

const JSON_HEADERS = {
  "content-type": "application/json; charset=utf-8",
  "cache-control": "no-store",
};

export default {
  async fetch(request: Request, env: RuntimeEnv, _ctx: ExecutionContext): Promise<Response> {
    const url = new URL(request.url);
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: corsHeaders() });
    }
    if (url.pathname === "/health" && request.method === "GET") {
      return json({ ok: true }, 200);
    }
    if (url.pathname === "/v1/instance" && request.method === "GET") {
      return json({
        instance_id: env.SPM_CLOUD_INSTANCE_ID,
        origin: env.SPM_CLOUD_ORIGIN,
        websocket_origin: env.SPM_CLOUD_ORIGIN.replace(/^http/, "ws"),
        protocol: "spm.cloud.v1",
        limits: {
          max_message_bytes: 64 * 1024,
          max_snapshot_bytes: 16 * 1024 * 1024,
          max_snapshot_chunk_bytes: 256 * 1024,
          max_asset_bytes: 128 * 1024 * 1024,
          max_subscriptions: 128,
          heartbeat_interval_seconds: 15,
          heartbeat_ttl_seconds: 45,
        },
      }, 200);
    }

    if (!authorized(request, env.SPM_CLOUD_ACCESS_TOKEN)) {
      return json({ code: "UNAUTHENTICATED", message: "Cloud bearer is required" }, 401);
    }

    if (url.pathname === "/v1/realtime" && request.method === "GET") {
      const scopeId = url.searchParams.get("scope_id");
      if (!scopeId) return json({ code: "INVALID_METADATA", message: "scope_id is required" }, 400);
      const id = env.SCOPE_ROOMS.idFromName(scopeId);
      return env.SCOPE_ROOMS.get(id).fetch(request);
    }
    if (url.pathname === "/v1/assets" && request.method === "GET") {
      return listAssets(env, "account_local");
    }
    if (url.pathname === "/v1/assets" && request.method === "POST") {
      try {
        return await uploadAsset(request, env, "account_local");
      } catch (error) {
        if (error instanceof Response) return error;
        throw error;
      }
    }
    const contentMatch = url.pathname.match(/^\/v1\/assets\/([^/]+)\/revisions\/(\d+)\/content$/);
    if (contentMatch && request.method === "GET") {
      return downloadAsset(request, env, contentMatch[1], Number(contentMatch[2]));
    }
    return json({ code: "NOT_FOUND", message: "Cloud route not found" }, 404);
  },
};

async function listAssets(env: Env, accountId: string): Promise<Response> {
  const result = await env.DB.prepare(
    `SELECT r.asset_id, r.revision, r.name, r.format, r.raw_sha256, r.byte_length
     FROM asset_revisions r JOIN asset_acl a ON a.asset_id = r.asset_id
     WHERE a.account_id = ?1 AND a.permission IN ('manage', 'use', 'discover')
     ORDER BY r.asset_id, r.revision`,
  ).bind(accountId).all();
  return json(result.results, 200);
}

async function uploadAsset(request: Request, env: Env, accountId: string): Promise<Response> {
  const requestId = requiredHeader(request, "idempotency-key");
  const assetId = request.headers.get("x-asset-id") || `asset_${crypto.randomUUID().replaceAll("-", "")}`;
  const name = request.headers.get("x-asset-name") || assetId;
  const format = request.headers.get("x-asset-format") || "application/octet-stream";
  const expectedSha = request.headers.get("x-asset-sha256");
  const body = await request.arrayBuffer();
  if (body.byteLength === 0 || body.byteLength > 128 * 1024 * 1024) {
    return json({ code: "ASSET_TOO_LARGE", message: "asset size is outside the prototype limit" }, 413);
  }
  const sha = await sha256(body);
  if (expectedSha && !constantTimeEqual(expectedSha.toLowerCase(), sha)) {
    return json({ code: "ASSET_HASH_MISMATCH", message: "asset SHA-256 mismatch" }, 422);
  }
  const requestHash = await sha256(new TextEncoder().encode(JSON.stringify({ assetId, name, format, sha, length: body.byteLength })));
  const existing = await env.DB.prepare(
    "SELECT request_hash, response_json FROM idempotency WHERE account_id = ?1 AND request_id = ?2",
  ).bind(accountId, requestId).first<{ request_hash: string; response_json: string }>();
  if (existing) {
    if (existing.request_hash !== requestHash) return json({ code: "IDEMPOTENCY_CONFLICT", message: "request key was reused" }, 409);
    return json(JSON.parse(existing.response_json), 201);
  }

  await env.ASSETS.put(`assets/${sha}`, body, { httpMetadata: { contentType: format }, customMetadata: { sha256: sha } });
  const current = await env.DB.prepare("SELECT current_revision FROM assets WHERE asset_id = ?1").bind(assetId).first<{ current_revision: number }>();
  const revision = (current?.current_revision ?? 0) + 1;
  const summary = { asset_id: assetId, revision, name, format, raw_sha256: sha, byte_length: body.byteLength };
  await env.DB.batch([
    env.DB.prepare("INSERT INTO assets(asset_id, owner_account_id, current_revision) VALUES (?1, ?2, ?3) ON CONFLICT(asset_id) DO UPDATE SET current_revision = excluded.current_revision").bind(assetId, accountId, revision),
    env.DB.prepare("INSERT INTO asset_revisions(asset_id, revision, name, format, raw_sha256, byte_length, object_key) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)").bind(assetId, revision, name, format, sha, body.byteLength, `assets/${sha}`),
    env.DB.prepare("INSERT INTO asset_acl(asset_id, account_id, permission) VALUES (?1, ?2, 'manage') ON CONFLICT(asset_id, account_id) DO UPDATE SET permission = 'manage'").bind(assetId, accountId),
    env.DB.prepare("INSERT INTO catalog_events(tenant_id, asset_id, revision) VALUES (?1, ?2, ?3)").bind(accountId, assetId, revision),
    env.DB.prepare("INSERT INTO idempotency(account_id, request_id, request_hash, response_json) VALUES (?1, ?2, ?3, ?4)").bind(accountId, requestId, requestHash, JSON.stringify(summary)),
  ]);
  return json(summary, 201);
}

async function downloadAsset(request: Request, env: Env, assetId: string, revision: number): Promise<Response> {
  const row = await env.DB.prepare("SELECT object_key, raw_sha256, byte_length, format FROM asset_revisions WHERE asset_id = ?1 AND revision = ?2").bind(assetId, revision).first<{ object_key: string; raw_sha256: string; byte_length: number; format: string }>();
  if (!row) return json({ code: "NOT_FOUND", message: "asset revision not found" }, 404);
  const object = await env.ASSETS.get(row.object_key);
  if (!object) return json({ code: "NOT_FOUND", message: "asset object not found" }, 404);
  const headers = new Headers(corsHeaders());
  headers.set("etag", `"${row.raw_sha256}"`);
  headers.set("accept-ranges", "bytes");
  headers.set("content-type", row.format);
  headers.set("content-length", String(object.size));
  return new Response(object.body, { status: 200, headers });
}

function json(value: unknown, status: number): Response {
  const headers = new Headers(JSON_HEADERS);
  Object.entries(corsHeaders()).forEach(([key, value]) => headers.set(key, value));
  return new Response(JSON.stringify(value), { status, headers });
}

function corsHeaders(): Record<string, string> {
  return { "access-control-allow-origin": "*", "access-control-allow-headers": "authorization,content-type,idempotency-key,x-asset-id,x-asset-name,x-asset-format,x-asset-sha256", "access-control-allow-methods": "GET,POST,OPTIONS" };
}

function authorized(request: Request, expected: string): boolean {
  const actual = request.headers.get("authorization")?.replace(/^Bearer\s+/i, "") ?? "";
  return Boolean(expected) && constantTimeEqual(actual, expected);
}

function constantTimeEqual(left: string, right: string): boolean {
  const a = new TextEncoder().encode(left);
  const b = new TextEncoder().encode(right);
  let result = a.length ^ b.length;
  for (let i = 0; i < Math.max(a.length, b.length); i++) result |= (a[i % Math.max(a.length, 1)] ?? 0) ^ (b[i % Math.max(b.length, 1)] ?? 0);
  return result === 0;
}

function requiredHeader(request: Request, name: string): string {
  const value = request.headers.get(name);
  if (!value || value.length > 256 || /[\r\n]/.test(value)) throw new Response(JSON.stringify({ code: "INVALID_METADATA", message: `${name} is required` }), { status: 400 });
  return value;
}

async function sha256(value: ArrayBuffer | Uint8Array): Promise<string> {
  const bytes = value instanceof Uint8Array ? value : new Uint8Array(value);
  const copy = new ArrayBuffer(bytes.byteLength);
  new Uint8Array(copy).set(bytes);
  const digest = await crypto.subtle.digest("SHA-256", copy);
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}
