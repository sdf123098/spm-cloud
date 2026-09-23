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
        websocket_origin: `${env.SPM_CLOUD_ORIGIN.replace(/^http/, "ws")}/v1/realtime`,
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

    try {
      if (url.pathname === "/v1/accounts" && request.method === "POST") return await createAccount(request, env);
      if (url.pathname === "/v1/sessions" && request.method === "POST") return await login(request, env);
      if (url.pathname === "/v1/sessions/refresh" && request.method === "POST") return await refreshSession(request, env);

      const accountId = await authenticate(request, env);
      if (!accountId) return json({ code: "UNAUTHENTICATED", message: "Cloud bearer is required" }, 401);

      if (url.pathname === "/v1/sessions/current" && request.method === "DELETE") {
        await revokeSession(request, env);
        return new Response(null, { status: 204, headers: corsHeaders() });
      }
      if (url.pathname === "/v1/realtime" && request.method === "GET") {
        const scopeId = url.searchParams.get("scope_id");
        if (!scopeId) return json({ code: "INVALID_METADATA", message: "scope_id is required" }, 400);
        const id = env.SCOPE_ROOMS.idFromName(scopeId);
        return env.SCOPE_ROOMS.get(id).fetch(request);
      }
      if (url.pathname === "/v1/assets" && request.method === "GET") return listAssets(env, accountId);
      if (url.pathname === "/v1/assets" && request.method === "POST") return uploadAsset(request, env, accountId);
      const contentMatch = url.pathname.match(/^\/v1\/assets\/([^/]+)\/revisions\/(\d+)\/content$/);
      if (contentMatch && request.method === "GET") return downloadAsset(request, env, accountId, contentMatch[1], Number(contentMatch[2]));
      if (url.pathname === "/v1/identity-providers" && request.method === "GET") return json([], 200);
      if (url.pathname === "/v1/identities" && request.method === "GET") return listIdentities(env, accountId);
      if (url.pathname === "/v1/identities" && request.method === "POST") return createIdentity(request, env, accountId);
      const identityBindings = url.pathname.match(/^\/v1\/identities\/([^/]+)\/offline-bindings$/);
      if (identityBindings && request.method === "POST") return createOfflineBinding(request, env, accountId, identityBindings[1]);
      const scopeOfflineBindings = url.pathname.match(/^\/v1\/scopes\/([^/]+)\/offline-bindings$/);
      if (scopeOfflineBindings && request.method === "GET") return listOfflineBindings(env, accountId, scopeOfflineBindings[1]);
      const bindingApproval = url.pathname.match(/^\/v1\/scoped-identity-bindings\/([^/]+)$/);
      if (bindingApproval && request.method === "PUT") return approveOfflineBinding(request, env, accountId, bindingApproval[1]);
      if (url.pathname === "/v1/claim-codes/redeem" && request.method === "POST") return redeemClaimCode(request, env, accountId);
      if (url.pathname === "/v1/claim-codes/revoke" && request.method === "POST") return revokeClaimCode(request, env, accountId);
      const targetClaimCodes = url.pathname.match(/^\/v1\/targets\/([^/]+)\/claim-codes$/);
      if (targetClaimCodes && request.method === "POST") return createClaimCode(request, env, accountId, targetClaimCodes[1]);
      if (url.pathname === "/v1/auth/challenges" && request.method === "POST") return identityChallengeUnavailable();
      const challengeComplete = url.pathname.match(/^\/v1\/auth\/challenges\/([^/]+)\/(?:complete|verify)$/);
      if (challengeComplete && request.method === "POST") return identityChallengeUnavailable();
      if (url.pathname === "/v1/scopes" && request.method === "GET") return listScopes(env, accountId);
      if (url.pathname === "/v1/scopes" && request.method === "POST") return createScope(request, env, accountId);
      const scopeAcl = url.pathname.match(/^\/v1\/scopes\/([^/]+)\/acl$/);
      if (scopeAcl && request.method === "GET") return listScopeAcl(env, accountId, scopeAcl[1]);
      if (scopeAcl && request.method === "PUT") return setScopeAcl(request, env, accountId, scopeAcl[1]);
      const scopeTargets = url.pathname.match(/^\/v1\/scopes\/([^/]+)\/targets$/);
      if (scopeTargets && request.method === "GET") return listTargets(env, accountId, scopeTargets[1]);
      if (url.pathname === "/v1/targets" && request.method === "POST") return createTarget(request, env, accountId);
      const targetAcl = url.pathname.match(/^\/v1\/targets\/([^/]+)\/acl$/);
      if (targetAcl && request.method === "GET") return listTargetAcl(env, accountId, targetAcl[1]);
      if (targetAcl && request.method === "PUT") return setTargetAcl(request, env, accountId, targetAcl[1]);
      const appearance = url.pathname.match(/^\/v1\/targets\/([^/]+)\/appearance$/);
      if (appearance && request.method === "GET") return getAppearance(env, accountId, appearance[1]);
      if (appearance && request.method === "PUT") return updateAppearance(request, env, accountId, appearance[1]);
      const animation = url.pathname.match(/^\/v1\/targets\/([^/]+)\/animation$/);
      if (animation && request.method === "GET") return getAnimation(env, accountId, animation[1]);
      if (animation && request.method === "PUT") return updateAnimation(request, env, accountId, animation[1]);
      const bindings = url.pathname.match(/^\/v1\/scopes\/([^/]+)\/bindings$/);
      if (bindings && request.method === "GET") return listBindings(env, accountId, bindings[1]);
      if (bindings && request.method === "POST") return registerBinding(request, env, accountId, bindings[1]);
      const observe = url.pathname.match(/^\/v1\/bindings\/([^/]+)\/observation$/);
      if (observe && request.method === "PUT") return observeBinding(request, env, accountId, observe[1]);
      const recovery = url.pathname.match(/^\/v1\/scopes\/([^/]+)\/events\/recovery$/);
      if (recovery && request.method === "GET") return json({ from_cursor: 0, to_cursor: 0, has_more: false, entries: [] }, 200);
      return json({ code: "NOT_FOUND", message: "Cloud route not found" }, 404);
    } catch (error) {
      if (error instanceof Response) return error;
      console.error("Cloud request failed", error);
      return json({ code: "INTERNAL", message: "Cloud request failed" }, 500);
    }
  },
};

type AccountRow = { account_id: string };

const ACCESS_TTL_SECONDS = 60 * 60;
const REFRESH_TTL_SECONDS = 30 * 24 * 60 * 60;
// Cloudflare Workers WebCrypto currently caps PBKDF2 at 100,000 iterations.
const PASSWORD_ITERATIONS = 100_000;

async function createAccount(request: Request, env: RuntimeEnv): Promise<Response> {
  const body = await readJson(request);
  const accountId = slug(body.account_id, "account_id");
  const password = text(body.password, "password", 8, 1024);
  const exists = await env.DB.prepare("SELECT account_id FROM accounts WHERE account_id = ?1").bind(accountId).first<AccountRow>();
  if (exists) return json({ code: "ACCOUNT_EXISTS", message: "account already exists" }, 409);
  const passwordHash = await hashPassword(password);
  await env.DB.batch([
    env.DB.prepare("INSERT INTO accounts(account_id) VALUES (?1)").bind(accountId),
    env.DB.prepare("INSERT INTO account_credentials(account_id, password_hash) VALUES (?1, ?2)").bind(accountId, passwordHash),
  ]);
  return json({ account_id: accountId }, 201);
}

async function login(request: Request, env: RuntimeEnv): Promise<Response> {
  const body = await readJson(request);
  const accountId = slug(body.account_id, "account_id");
  const password = text(body.password, "password", 1, 1024);
  const row = await env.DB.prepare("SELECT password_hash FROM account_credentials WHERE account_id = ?1").bind(accountId).first<{ password_hash: string }>();
  if (!row || !(await verifyPassword(password, row.password_hash))) return json({ code: "UNAUTHENTICATED", message: "invalid credentials" }, 401);
  return json(await issueSession(env, accountId), 200);
}

async function refreshSession(request: Request, env: RuntimeEnv): Promise<Response> {
  const body = await readJson(request);
  const refreshToken = text(body.refresh_token, "refresh_token", 1, 512);
  const refreshHash = await sha256(new TextEncoder().encode(refreshToken));
  const now = Math.floor(Date.now() / 1000);
  const row = await env.DB.prepare(
    "SELECT account_id FROM sessions WHERE refresh_hash = ?1 AND revoked = 0 AND refresh_expires_at > ?2",
  ).bind(refreshHash, now).first<AccountRow>();
  if (!row) return json({ code: "SESSION_EXPIRED", message: "refresh session is invalid" }, 401);
  return json(await issueSession(env, row.account_id), 200);
}

async function issueSession(env: RuntimeEnv, accountId: string): Promise<Record<string, unknown>> {
  const accessToken = `spm_access_${crypto.randomUUID().replaceAll("-", "")}`;
  const refreshToken = `spm_refresh_${crypto.randomUUID().replaceAll("-", "")}`;
  const now = Math.floor(Date.now() / 1000);
  await env.DB.prepare(
    `INSERT INTO sessions(session_id, account_id, access_hash, refresh_hash, access_expires_at, refresh_expires_at)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)`,
  ).bind(
    crypto.randomUUID(), accountId,
    await sha256(new TextEncoder().encode(accessToken)),
    await sha256(new TextEncoder().encode(refreshToken)),
    now + ACCESS_TTL_SECONDS,
    now + REFRESH_TTL_SECONDS,
  ).run();
  return {
    access_token: accessToken,
    refresh_token: refreshToken,
    access_expires_in_seconds: ACCESS_TTL_SECONDS,
    refresh_expires_in_seconds: REFRESH_TTL_SECONDS,
  };
}

async function revokeSession(request: Request, env: RuntimeEnv): Promise<void> {
  const token = bearer(request);
  if (!token) return;
  await env.DB.prepare("UPDATE sessions SET revoked = 1 WHERE access_hash = ?1")
    .bind(await sha256(new TextEncoder().encode(token))).run();
}

async function authenticate(request: Request, env: RuntimeEnv): Promise<string | null> {
  const token = bearer(request);
  if (!token) return null;
  if (constantTimeEqual(token, env.SPM_CLOUD_ACCESS_TOKEN)) {
    await env.DB.prepare("INSERT OR IGNORE INTO accounts(account_id) VALUES ('account_local')").run();
    return "account_local";
  }
  const now = Math.floor(Date.now() / 1000);
  const row = await env.DB.prepare(
    "SELECT account_id FROM sessions WHERE access_hash = ?1 AND revoked = 0 AND access_expires_at > ?2",
  ).bind(await sha256(new TextEncoder().encode(token)), now).first<AccountRow>();
  return row?.account_id ?? null;
}

async function listIdentities(env: Env, accountId: string): Promise<Response> {
  const result = await env.DB.prepare(
    `SELECT identity_id, account_id, identity_kind, provider_id, scope_id, profile_uuid,
            display_name, CASE WHEN verified = 1 THEN 'VERIFIED' ELSE 'PENDING_VERIFICATION' END AS verification_status
     FROM identities WHERE account_id = ?1 ORDER BY identity_id`,
  ).bind(accountId).all();
  return json(result.results.map(row => ({
    identity_id: row.identity_id,
    account_id: row.account_id,
    identity: `${row.identity_kind}:${row.scope_id ?? row.provider_id ?? ""}:${row.profile_uuid}`,
    display_name: row.display_name,
    verification_status: row.verification_status,
  })), 200);
}

async function createIdentity(request: Request, env: Env, accountId: string): Promise<Response> {
  const body = await readJson(request);
  const wire = text(body.identity, "identity", 3, 512);
  const parts = wire.split(":");
  if (parts.length !== 3 || parts[0] !== "offline") return json({ code: "INVALID_METADATA", message: "only offline identities can be registered here" }, 400);
  const scopeId = slug(parts[1], "scope_id");
  const profileUuid = text(parts[2], "profile_uuid", 1, 128);
  await requireScope(env, accountId, scopeId, "editor");
  const identityId = `identity_${crypto.randomUUID().replaceAll("-", "")}`;
  const displayName = text(body.display_name, "display_name", 1, 256);
  await env.DB.prepare(
    `INSERT INTO identities(identity_id, account_id, identity_kind, provider_id, scope_id, profile_uuid, display_name, verified)
     VALUES (?1, ?2, 'offline', NULL, ?3, ?4, ?5, 0)`,
  ).bind(identityId, accountId, scopeId, profileUuid, displayName).run();
  return json({ identity_id: identityId, account_id: accountId, identity: wire, display_name: displayName, verification_status: "PENDING_VERIFICATION" }, 201);
}

async function createOfflineBinding(request: Request, env: Env, accountId: string, identityId: string): Promise<Response> {
  const body = await readJson(request);
  const scopeId = slug(body.scope_id, "scope_id");
  const worldEpoch = slug(body.world_epoch, "world_epoch");
  const targetId = slug(body.target_id, "target_id");
  await requireScope(env, accountId, scopeId, "editor");
  const identity = await env.DB.prepare(
    "SELECT identity_kind, scope_id, profile_uuid FROM identities WHERE identity_id = ?1 AND account_id = ?2",
  ).bind(identityId, accountId).first<{ identity_kind: string; scope_id: string | null; profile_uuid: string }>();
  if (!identity) return json({ code: "NOT_FOUND", message: "identity not found" }, 404);
  if (identity.identity_kind !== "offline" || identity.scope_id !== scopeId) return json({ code: "ACCESS_DENIED", message: "identity is outside this scope" }, 403);
  const scope = await env.DB.prepare("SELECT offline_policy FROM scopes WHERE scope_id = ?1 AND world_epoch = ?2").bind(scopeId, worldEpoch).first<{ offline_policy: string }>();
  if (!scope) return json({ code: "NOT_FOUND", message: "scope/world_epoch not found" }, 404);
  if (scope.offline_policy !== "STRICT_APPROVAL") return json({ code: "ACCESS_DENIED", message: "scope does not allow strict approval" }, 403);
  const target = await env.DB.prepare("SELECT scope_id, target_kind FROM targets WHERE target_id = ?1").bind(targetId).first<{ scope_id: string; target_kind: string }>();
  if (!target || target.scope_id !== scopeId || target.target_kind !== "PLAYER") return json({ code: "NOT_FOUND", message: "PLAYER target not found" }, 404);
  const duplicate = await env.DB.prepare(
    "SELECT binding_id FROM scoped_identity_bindings WHERE scope_id = ?1 AND world_epoch = ?2 AND entity_uuid = ?3 AND status = 'APPROVED'",
  ).bind(scopeId, worldEpoch, identity.profile_uuid).first();
  if (duplicate) return json({ code: "REVISION_CONFLICT", message: "identity is already approved in this scope" }, 409);
  const bindingId = `offline_binding_${crypto.randomUUID().replaceAll("-", "")}`;
  await env.DB.prepare(
    `INSERT INTO scoped_identity_bindings(binding_id, account_id, identity_id, target_id, scope_id, world_epoch, entity_uuid, verification_method, status, revision)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'STRICT_APPROVAL', 'PENDING_APPROVAL', 0)`,
  ).bind(bindingId, accountId, identityId, targetId, scopeId, worldEpoch, identity.profile_uuid).run();
  return json({ binding_id: bindingId, account_id: accountId, identity_id: identityId, target_id: targetId, scope_id: scopeId, world_epoch: worldEpoch, entity_uuid: identity.profile_uuid, verification_method: "STRICT_APPROVAL", status: "PENDING_APPROVAL", approved_by: null, revision: 0 }, 201);
}

async function listOfflineBindings(env: Env, accountId: string, scopeId: string): Promise<Response> {
  await requireScope(env, accountId, scopeId, "viewer");
  const result = await env.DB.prepare(
    `SELECT binding_id, account_id, identity_id, target_id, scope_id, world_epoch, entity_uuid, verification_method, status, approved_by, revision
     FROM scoped_identity_bindings WHERE scope_id = ?1 ORDER BY binding_id`,
  ).bind(scopeId).all();
  return json(result.results, 200);
}

async function approveOfflineBinding(request: Request, env: Env, accountId: string, bindingId: string): Promise<Response> {
  const body = await readJson(request);
  const status = text(body.status, "status", 1, 32).toUpperCase();
  if (status !== "APPROVED" && status !== "REJECTED") return json({ code: "INVALID_METADATA", message: "status is invalid" }, 400);
  const expected = integer(body.expected_revision, "expected_revision", 0);
  const binding = await env.DB.prepare("SELECT * FROM scoped_identity_bindings WHERE binding_id = ?1").bind(bindingId).first<Record<string, any>>();
  if (!binding) return json({ code: "NOT_FOUND", message: "binding not found" }, 404);
  await requireScope(env, accountId, String(binding.scope_id), "manage");
  const scope = await env.DB.prepare("SELECT offline_policy FROM scopes WHERE scope_id = ?1").bind(binding.scope_id).first<{ offline_policy: string }>();
  if (scope?.offline_policy !== "STRICT_APPROVAL") return json({ code: "ACCESS_DENIED", message: "scope does not allow strict approval" }, 403);
  if (Number(binding.revision) !== expected) return json({ code: "REVISION_CONFLICT", message: "binding revision conflict" }, 409);
  if (status === "APPROVED") {
    const duplicate = await env.DB.prepare(
      "SELECT binding_id FROM scoped_identity_bindings WHERE scope_id = ?1 AND world_epoch = ?2 AND entity_uuid = ?3 AND status = 'APPROVED' AND binding_id <> ?4",
    ).bind(binding.scope_id, binding.world_epoch, binding.entity_uuid, bindingId).first();
    if (duplicate) return json({ code: "REVISION_CONFLICT", message: "identity is already approved in this scope" }, 409);
  }
  await env.DB.prepare("UPDATE scoped_identity_bindings SET status = ?1, approved_by = ?2, revision = revision + 1, updated_at = CURRENT_TIMESTAMP WHERE binding_id = ?3 AND revision = ?4")
    .bind(status, accountId, bindingId, expected).run();
  const updated = await env.DB.prepare("SELECT binding_id, account_id, identity_id, target_id, scope_id, world_epoch, entity_uuid, verification_method, status, approved_by, revision FROM scoped_identity_bindings WHERE binding_id = ?1").bind(bindingId).first();
  return json(updated, 200);
}

async function createClaimCode(request: Request, env: Env, accountId: string, targetId: string): Promise<Response> {
  const body = await readJson(request);
  const worldEpoch = slug(body.world_epoch, "world_epoch");
  const entityUuid = uuidText(body.entity_uuid, "entity_uuid");
  const expiresIn = Math.max(60, Math.min(86_400, integer(body.expires_in_seconds ?? 600, "expires_in_seconds", 1)));
  const target = await env.DB.prepare("SELECT scope_id, target_kind FROM targets WHERE target_id = ?1").bind(targetId).first<{ scope_id: string; target_kind: string }>();
  if (!target) return json({ code: "NOT_FOUND", message: "target not found" }, 404);
  if (target.target_kind !== "PLAYER") return json({ code: "INVALID_METADATA", message: "claim codes require PLAYER targets" }, 400);
  await requireScope(env, accountId, target.scope_id, "manage");
  const scope = await env.DB.prepare("SELECT world_epoch, offline_policy FROM scopes WHERE scope_id = ?1").bind(target.scope_id).first<{ world_epoch: string; offline_policy: string }>();
  if (!scope || scope.world_epoch !== worldEpoch) return json({ code: "REVISION_CONFLICT", message: "world_epoch does not match scope" }, 409);
  if (scope.offline_policy !== "CLAIM_CODE" && scope.offline_policy !== "FIRST_CLAIM") return json({ code: "ACCESS_DENIED", message: "scope does not allow claim codes" }, 403);
  if (scope.offline_policy === "FIRST_CLAIM") {
    const approved = await env.DB.prepare("SELECT binding_id FROM scoped_identity_bindings WHERE scope_id = ?1 AND world_epoch = ?2 AND entity_uuid = ?3 AND status = 'APPROVED'").bind(target.scope_id, worldEpoch, entityUuid).first();
    if (approved) return json({ code: "REVISION_CONFLICT", message: "entity already claimed" }, 409);
  }
  const code = `spm_claim_${crypto.randomUUID().replaceAll("-", "")}`;
  await env.DB.prepare(
    "INSERT INTO claim_codes(code_hash, scope_id, world_epoch, target_id, entity_uuid, issued_by, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
  ).bind(await tokenHash(code), target.scope_id, worldEpoch, targetId, entityUuid, accountId, Math.floor(Date.now() / 1000) + expiresIn).run();
  return json({ code, scope_id: target.scope_id, world_epoch: worldEpoch, target_id: targetId, entity_uuid: entityUuid, expires_in_seconds: expiresIn }, 201);
}

async function redeemClaimCode(request: Request, env: Env, accountId: string): Promise<Response> {
  const body = await readJson(request);
  const code = text(body.code, "code", 1, 256);
  const identityId = slug(body.identity_id, "identity_id");
  const claim = await env.DB.prepare("SELECT * FROM claim_codes WHERE code_hash = ?1 AND revoked = 0").bind(await tokenHash(code)).first<Record<string, any>>();
  if (!claim) return json({ code: "NOT_FOUND", message: "claim code not found" }, 404);
  if (Number(claim.consumed) !== 0 || Number(claim.expires_at) <= Math.floor(Date.now() / 1000) || Number(claim.attempts) >= Number(claim.max_attempts)) return json({ code: "ACCESS_DENIED", message: "claim code is expired or exhausted" }, 403);
  const scope = await env.DB.prepare("SELECT offline_policy FROM scopes WHERE scope_id = ?1").bind(claim.scope_id).first<{ offline_policy: string }>();
  if (!scope || (scope.offline_policy !== "CLAIM_CODE" && scope.offline_policy !== "FIRST_CLAIM")) return json({ code: "ACCESS_DENIED", message: "scope does not allow claim codes" }, 403);
  await env.DB.prepare("UPDATE claim_codes SET attempts = attempts + 1 WHERE code_hash = ?1").bind(await tokenHash(code)).run();
  const identity = await env.DB.prepare("SELECT identity_kind, scope_id, profile_uuid FROM identities WHERE identity_id = ?1 AND account_id = ?2").bind(identityId, accountId).first<{ identity_kind: string; scope_id: string | null; profile_uuid: string }>();
  if (!identity) return json({ code: "NOT_FOUND", message: "identity not found" }, 404);
  if (identity.identity_kind !== "offline" || identity.scope_id !== claim.scope_id || identity.profile_uuid !== claim.entity_uuid) return json({ code: "ACCESS_DENIED", message: "identity does not match claim" }, 403);
  const duplicate = await env.DB.prepare("SELECT binding_id FROM scoped_identity_bindings WHERE scope_id = ?1 AND world_epoch = ?2 AND entity_uuid = ?3 AND status = 'APPROVED'").bind(claim.scope_id, claim.world_epoch, claim.entity_uuid).first();
  if (duplicate) return json({ code: "REVISION_CONFLICT", message: "entity already claimed" }, 409);
  const bindingId = `offline_binding_${crypto.randomUUID().replaceAll("-", "")}`;
  await env.DB.batch([
    env.DB.prepare("INSERT INTO scoped_identity_bindings(binding_id, account_id, identity_id, target_id, scope_id, world_epoch, entity_uuid, verification_method, status, revision) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'CLAIM_CODE', 'APPROVED', 1)").bind(bindingId, accountId, identityId, claim.target_id, claim.scope_id, claim.world_epoch, claim.entity_uuid),
    env.DB.prepare("UPDATE claim_codes SET consumed = 1 WHERE code_hash = ?1").bind(await tokenHash(code)),
  ]);
  return json({ binding_id: bindingId, account_id: accountId, identity_id: identityId, target_id: claim.target_id, scope_id: claim.scope_id, world_epoch: claim.world_epoch, entity_uuid: claim.entity_uuid, verification_method: "CLAIM_CODE", status: "APPROVED", approved_by: null, revision: 1 }, 200);
}

async function revokeClaimCode(request: Request, env: Env, accountId: string): Promise<Response> {
  const body = await readJson(request);
  const code = text(body.code, "code", 1, 256);
  const hash = await tokenHash(code);
  const claim = await env.DB.prepare("SELECT scope_id FROM claim_codes WHERE code_hash = ?1").bind(hash).first<{ scope_id: string }>();
  if (!claim) return json({ code: "NOT_FOUND", message: "claim code not found" }, 404);
  await requireScope(env, accountId, claim.scope_id, "manage");
  await env.DB.prepare("UPDATE claim_codes SET revoked = 1 WHERE code_hash = ?1 AND consumed = 0").bind(hash).run();
  return new Response(null, { status: 204, headers: corsHeaders() });
}

function identityChallengeUnavailable(): Response {
  return json({ code: "IDENTITY_PROVIDER_UNTRUSTED", message: "No official or third-party identity provider is enabled on this Cloud instance" }, 503);
}

async function listScopes(env: Env, accountId: string): Promise<Response> {
  const result = await env.DB.prepare(
    `SELECT s.scope_id, s.tenant_id, s.name, s.world_epoch, s.offline_policy
     FROM scopes s JOIN scope_acl a ON a.scope_id = s.scope_id
     WHERE a.account_id = ?1 ORDER BY s.scope_id`,
  ).bind(accountId).all();
  return json(result.results, 200);
}

async function createScope(request: Request, env: Env, accountId: string): Promise<Response> {
  const body = await readJson(request);
  const scopeId = slug(body.scope_id, "scope_id");
  const name = text(body.name, "name", 1, 256);
  const worldEpoch = slug(body.world_epoch, "world_epoch");
  const offlinePolicy = body.offline_policy == null ? "STRICT_APPROVAL" : text(body.offline_policy, "offline_policy", 1, 64);
  const existing = await env.DB.prepare("SELECT scope_id FROM scopes WHERE scope_id = ?1").bind(scopeId).first();
  if (existing) return json({ code: "SCOPE_EXISTS", message: "scope already exists" }, 409);
  await env.DB.batch([
    env.DB.prepare("INSERT INTO scopes(scope_id, tenant_id, name, world_epoch, offline_policy) VALUES (?1, ?2, ?3, ?4, ?5)").bind(scopeId, accountId, name, worldEpoch, offlinePolicy),
    env.DB.prepare("INSERT INTO scope_acl(scope_id, account_id, role) VALUES (?1, ?2, 'manage')").bind(scopeId, accountId),
  ]);
  return json({ scope_id: scopeId, tenant_id: accountId, name, world_epoch: worldEpoch, offline_policy: offlinePolicy }, 201);
}

async function listScopeAcl(env: Env, accountId: string, scopeId: string): Promise<Response> {
  await requireScope(env, accountId, scopeId, "viewer");
  const result = await env.DB.prepare("SELECT account_id, role FROM scope_acl WHERE scope_id = ?1 ORDER BY account_id").bind(scopeId).all();
  return json(result.results, 200);
}

async function setScopeAcl(request: Request, env: Env, accountId: string, scopeId: string): Promise<Response> {
  await requireScope(env, accountId, scopeId, "manage");
  const body = await readJson(request);
  const targetAccount = slug(body.account_id, "account_id");
  const role = roleText(body.role);
  await env.DB.batch([
    env.DB.prepare("INSERT OR IGNORE INTO accounts(account_id) VALUES (?1)").bind(targetAccount),
    env.DB.prepare("INSERT INTO scope_acl(scope_id, account_id, role) VALUES (?1, ?2, ?3) ON CONFLICT(scope_id, account_id) DO UPDATE SET role = excluded.role").bind(scopeId, targetAccount, role),
  ]);
  return json({ account_id: targetAccount, role }, 200);
}

async function listTargets(env: Env, accountId: string, scopeId: string): Promise<Response> {
  await requireScope(env, accountId, scopeId, "viewer");
  const result = await env.DB.prepare("SELECT target_id, scope_id, target_kind AS kind, display_name, revision FROM targets WHERE scope_id = ?1 ORDER BY target_id").bind(scopeId).all();
  return json(result.results, 200);
}

async function createTarget(request: Request, env: Env, accountId: string): Promise<Response> {
  const body = await readJson(request);
  const scopeId = slug(body.scope_id, "scope_id");
  await requireScope(env, accountId, scopeId, "editor");
  const targetId = body.target_id == null ? `target_${crypto.randomUUID().replaceAll("-", "")}` : slug(body.target_id, "target_id");
  const kind = text(body.kind, "kind", 1, 64);
  const displayName = text(body.display_name, "display_name", 1, 256);
  await env.DB.batch([
    env.DB.prepare("INSERT INTO targets(target_id, scope_id, target_kind, display_name, owner_account_id) VALUES (?1, ?2, ?3, ?4, ?5)").bind(targetId, scopeId, kind, displayName, accountId),
    env.DB.prepare("INSERT INTO target_acl(target_id, account_id, role) VALUES (?1, ?2, 'manage')").bind(targetId, accountId),
    env.DB.prepare("INSERT INTO appearances(target_id) VALUES (?1)").bind(targetId),
    env.DB.prepare("INSERT INTO animation_states(target_id, owner_account_id, lease_id, expires_at_unix_ms, channel, action, animation_key) VALUES (?1, ?2, ?3, ?4, 'base', 'idle', '')").bind(targetId, accountId, crypto.randomUUID(), 0),
  ]);
  return json({ target_id: targetId, scope_id: scopeId, kind, display_name: displayName, revision: 0 }, 201);
}

async function listTargetAcl(env: Env, accountId: string, targetId: string): Promise<Response> {
  await requireTarget(env, accountId, targetId, "viewer");
  const result = await env.DB.prepare("SELECT account_id, role FROM target_acl WHERE target_id = ?1 ORDER BY account_id").bind(targetId).all();
  return json(result.results, 200);
}

async function setTargetAcl(request: Request, env: Env, accountId: string, targetId: string): Promise<Response> {
  await requireTarget(env, accountId, targetId, "manage");
  const body = await readJson(request);
  const targetAccount = slug(body.account_id, "account_id");
  const role = roleText(body.role);
  await env.DB.batch([
    env.DB.prepare("INSERT OR IGNORE INTO accounts(account_id) VALUES (?1)").bind(targetAccount),
    env.DB.prepare("INSERT INTO target_acl(target_id, account_id, role) VALUES (?1, ?2, ?3) ON CONFLICT(target_id, account_id) DO UPDATE SET role = excluded.role").bind(targetId, targetAccount, role),
  ]);
  return json({ account_id: targetAccount, role }, 200);
}

async function getAppearance(env: Env, accountId: string, targetId: string): Promise<Response> {
  await requireTarget(env, accountId, targetId, "viewer");
  const row = await env.DB.prepare("SELECT target_id, revision, asset_id, asset_revision, raw_sha256, texture_id, scale, disabled FROM appearances WHERE target_id = ?1").bind(targetId).first();
  return json(row ?? { target_id: targetId, revision: 0, asset_id: null, asset_revision: null, raw_sha256: null, texture_id: null, scale: null, disabled: false }, 200);
}

async function updateAppearance(request: Request, env: Env, accountId: string, targetId: string): Promise<Response> {
  await requireTarget(env, accountId, targetId, "editor");
  const body = await readJson(request);
  const expected = integer(body.expected_revision, "expected_revision", 0);
  const current = await env.DB.prepare("SELECT revision FROM appearances WHERE target_id = ?1").bind(targetId).first<{ revision: number }>();
  if ((current?.revision ?? 0) !== expected) return json({ code: "REVISION_CONFLICT", message: "appearance revision conflict" }, 409);
  const next = expected + 1;
  await env.DB.prepare(
    `INSERT INTO appearances(target_id, revision, asset_id, asset_revision, raw_sha256, texture_id, scale, disabled)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
     ON CONFLICT(target_id) DO UPDATE SET revision = excluded.revision, asset_id = excluded.asset_id, asset_revision = excluded.asset_revision, raw_sha256 = excluded.raw_sha256, texture_id = excluded.texture_id, scale = excluded.scale, disabled = excluded.disabled`,
  ).bind(targetId, next, nullableText(body.asset_id), nullableInteger(body.asset_revision), nullableText(body.raw_sha256), nullableText(body.texture_id), body.scale == null ? null : Number(body.scale), body.disabled === true ? 1 : 0).run();
  return getAppearance(env, accountId, targetId);
}

async function getAnimation(env: Env, accountId: string, targetId: string): Promise<Response> {
  await requireTarget(env, accountId, targetId, "viewer");
  const row = await env.DB.prepare("SELECT target_id, revision, owner_account_id, lease_id, expires_at_unix_ms, channel, action, animation_key FROM animation_states WHERE target_id = ?1").bind(targetId).first();
  return json(row ?? { target_id: targetId, revision: 0, owner_account_id: accountId, lease_id: "", expires_at_unix_ms: 0, channel: "base", action: "idle", animation_key: "" }, 200);
}

async function updateAnimation(request: Request, env: Env, accountId: string, targetId: string): Promise<Response> {
  await requireTarget(env, accountId, targetId, "editor");
  const body = await readJson(request);
  const current = await env.DB.prepare("SELECT revision, owner_account_id, lease_id, expires_at_unix_ms FROM animation_states WHERE target_id = ?1").bind(targetId).first<{ revision: number; owner_account_id: string; lease_id: string; expires_at_unix_ms: number }>();
  const expected = integer(body.expected_revision, "expected_revision", 0);
  if ((current?.revision ?? 0) !== expected) return json({ code: "REVISION_CONFLICT", message: "animation revision conflict" }, 409);
  const leaseId = nullableText(body.lease_id) ?? crypto.randomUUID();
  const ttl = Math.max(250, Math.min(60_000, integer(body.lease_ttl_ms ?? body.ttl_ms ?? body.ttl, "lease_ttl_ms", 250)));
  const row = { target_id: targetId, revision: expected + 1, owner_account_id: accountId, lease_id: leaseId, expires_at_unix_ms: Date.now() + ttl, channel: text(body.channel, "channel", 1, 64), action: text(body.action, "action", 1, 128), animation_key: text(body.animation_key, "animation_key", 0, 256) };
  await env.DB.prepare(
    `INSERT INTO animation_states(target_id, revision, owner_account_id, lease_id, expires_at_unix_ms, channel, action, animation_key)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
     ON CONFLICT(target_id) DO UPDATE SET revision = excluded.revision, owner_account_id = excluded.owner_account_id, lease_id = excluded.lease_id, expires_at_unix_ms = excluded.expires_at_unix_ms, channel = excluded.channel, action = excluded.action, animation_key = excluded.animation_key`,
  ).bind(row.target_id, row.revision, row.owner_account_id, row.lease_id, row.expires_at_unix_ms, row.channel, row.action, row.animation_key).run();
  return json(row, 200);
}

async function listBindings(env: Env, accountId: string, scopeId: string): Promise<Response> {
  await requireScope(env, accountId, scopeId, "viewer");
  const result = await env.DB.prepare("SELECT binding_id, scope_id, world_epoch, entity_uuid, entity_kind, target_id, observation_state, last_seen_at, revision FROM entity_bindings WHERE scope_id = ?1 ORDER BY binding_id").bind(scopeId).all();
  return json(result.results, 200);
}

async function registerBinding(request: Request, env: Env, accountId: string, scopeId: string): Promise<Response> {
  await requireScope(env, accountId, scopeId, "editor");
  const body = await readJson(request);
  const worldEpoch = slug(body.world_epoch, "world_epoch");
  const entityUuid = text(body.entity_uuid, "entity_uuid", 1, 128);
  const entityKind = text(body.entity_kind, "entity_kind", 1, 64);
  const targetId = slug(body.target_id, "target_id");
  await requireTarget(env, accountId, targetId, "editor");
  const bindingId = `binding_${crypto.randomUUID().replaceAll("-", "")}`;
  await env.DB.prepare("INSERT INTO entity_bindings(binding_id, scope_id, world_epoch, entity_uuid, entity_kind, target_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)").bind(bindingId, scopeId, worldEpoch, entityUuid, entityKind, targetId).run();
  return json({ binding_id: bindingId, scope_id: scopeId, world_epoch: worldEpoch, entity_uuid: entityUuid, entity_kind: entityKind, target_id: targetId, observation_state: "REGISTERED", last_seen_at: null, revision: 0 }, 201);
}

async function observeBinding(request: Request, env: Env, accountId: string, bindingId: string): Promise<Response> {
  const body = await readJson(request);
  const binding = await env.DB.prepare("SELECT * FROM entity_bindings WHERE binding_id = ?1").bind(bindingId).first<Record<string, unknown>>();
  if (!binding) return json({ code: "NOT_FOUND", message: "binding not found" }, 404);
  await requireScope(env, accountId, String(binding.scope_id), "editor");
  const worldEpoch = slug(body.world_epoch, "world_epoch");
  const state = text(body.observation_state, "observation_state", 1, 64);
  const revision = Number(binding.revision ?? 0) + 1;
  await env.DB.prepare("UPDATE entity_bindings SET world_epoch = ?1, observation_state = ?2, last_seen_at = CURRENT_TIMESTAMP, revision = ?3 WHERE binding_id = ?4").bind(worldEpoch, state, revision, bindingId).run();
  return json({ ...binding, world_epoch: worldEpoch, observation_state: state, last_seen_at: new Date().toISOString(), revision }, 200);
}

async function requireScope(env: Env, accountId: string, scopeId: string, minimum: "viewer" | "editor" | "manage"): Promise<Record<string, unknown>> {
  const row = await env.DB.prepare("SELECT s.*, a.role FROM scopes s JOIN scope_acl a ON a.scope_id = s.scope_id WHERE s.scope_id = ?1 AND a.account_id = ?2").bind(scopeId, accountId).first<Record<string, unknown>>();
  if (!row || !roleAllows(String(row.role), minimum)) throw new Response(JSON.stringify({ code: "SCOPE_ACCESS_DENIED", message: "scope access denied" }), { status: 403, headers: { ...corsHeaders(), ...JSON_HEADERS } });
  return row;
}

async function requireTarget(env: Env, accountId: string, targetId: string, minimum: "viewer" | "editor" | "manage"): Promise<Record<string, unknown>> {
  const row = await env.DB.prepare("SELECT t.*, a.role FROM targets t JOIN target_acl a ON a.target_id = t.target_id WHERE t.target_id = ?1 AND a.account_id = ?2").bind(targetId, accountId).first<Record<string, unknown>>();
  if (!row || !roleAllows(String(row.role), minimum)) throw new Response(JSON.stringify({ code: "TARGET_ACCESS_DENIED", message: "target access denied" }), { status: 403, headers: { ...corsHeaders(), ...JSON_HEADERS } });
  return row;
}

function roleAllows(role: string, minimum: "viewer" | "editor" | "manage"): boolean {
  const rank: Record<string, number> = { viewer: 1, editor: 2, manage: 3, owner: 3 };
  return (rank[role] ?? 0) >= rank[minimum];
}

function roleText(value: unknown): string {
  const role = text(value, "role", 1, 32).toLowerCase();
  if (!["viewer", "editor", "manage"].includes(role)) throw bad("role is invalid");
  return role;
}

async function readJson(request: Request): Promise<Record<string, any>> {
  const value = await request.json() as unknown;
  if (!value || typeof value !== "object" || Array.isArray(value)) throw bad("JSON object is required");
  return value as Record<string, any>;
}

function text(value: unknown, name: string, min: number, max: number): string {
  if (typeof value !== "string" || value.length < min || value.length > max || /[\r\n]/.test(value)) throw bad(`${name} is invalid`);
  return value;
}

function slug(value: unknown, name: string): string {
  const result = text(value, name, 1, 128).trim();
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(result)) throw bad(`${name} is invalid`);
  return result;
}

function integer(value: unknown, name: string, min: number): number {
  const result = typeof value === "number" ? value : Number(value);
  if (!Number.isInteger(result) || result < min) throw bad(`${name} is invalid`);
  return result;
}

function nullableText(value: unknown): string | null {
  return value == null ? null : text(value, "value", 1, 512);
}

function nullableInteger(value: unknown): number | null {
  return value == null ? null : integer(value, "value", 0);
}

function uuidText(value: unknown, name: string): string {
  const result = text(value, name, 32, 64).toLowerCase();
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(result)) throw bad(`${name} must be a UUID`);
  return result;
}

function bad(message: string): Response {
  return new Response(JSON.stringify({ code: "INVALID_METADATA", message }), { status: 400, headers: { ...corsHeaders(), ...JSON_HEADERS } });
}

function bearer(request: Request): string | null {
  const value = request.headers.get("authorization") ?? "";
  const token = value.replace(/^Bearer\s+/i, "");
  return token && token.length <= 1024 ? token : null;
}

async function hashPassword(password: string): Promise<string> {
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(password), "PBKDF2", false, ["deriveBits"]);
  const bits = await crypto.subtle.deriveBits({ name: "PBKDF2", salt: salt.buffer as ArrayBuffer, iterations: PASSWORD_ITERATIONS, hash: "SHA-256" }, key, 256);
  return `${hex(salt)}:${hex(new Uint8Array(bits))}`;
}

async function verifyPassword(password: string, stored: string): Promise<boolean> {
  const [saltText, expected] = stored.split(":");
  if (!saltText || !expected) return false;
  const salt = fromHex(saltText);
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(password), "PBKDF2", false, ["deriveBits"]);
  const bits = await crypto.subtle.deriveBits({ name: "PBKDF2", salt: salt.buffer as ArrayBuffer, iterations: PASSWORD_ITERATIONS, hash: "SHA-256" }, key, 256);
  return constantTimeEqual(hex(new Uint8Array(bits)), expected);
}

function hex(bytes: Uint8Array): string {
  return [...bytes].map(byte => byte.toString(16).padStart(2, "0")).join("");
}

function fromHex(value: string): Uint8Array {
  if (!/^[0-9a-f]+$/i.test(value) || value.length % 2 !== 0) throw new Error("invalid hex");
  const result = new Uint8Array(value.length / 2);
  for (let i = 0; i < result.length; i++) result[i] = Number.parseInt(value.slice(i * 2, i * 2 + 2), 16);
  return result;
}

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

async function downloadAsset(request: Request, env: Env, accountId: string, assetId: string, revision: number): Promise<Response> {
  const acl = await env.DB.prepare("SELECT permission FROM asset_acl WHERE asset_id = ?1 AND account_id = ?2").bind(assetId, accountId).first<{ permission: string }>();
  if (!acl || !["manage", "use", "render_read", "discover"].includes(acl.permission)) return json({ code: "ASSET_ACCESS_DENIED", message: "asset access denied" }, 403);
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

async function tokenHash(value: string): Promise<string> {
  return sha256(new TextEncoder().encode(value));
}
