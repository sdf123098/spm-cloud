import assert from "node:assert/strict";

const origin = process.env.SPM_CLOUD_TEST_ORIGIN;
if (!origin) throw new Error("SPM_CLOUD_TEST_ORIGIN must point at an isolated local Wrangler instance");

const unique = crypto.randomUUID().replaceAll("-", "");
const owner = `owner_${unique}`;
const observer = `observer_${unique}`;
const scopeId = `scope_${unique}`;
const targetId = `target_${unique}`;

async function request(path, { token, method = "GET", body } = {}) {
  const response = await fetch(new URL(path, origin), {
    method,
    headers: {
      ...(token ? { authorization: `Bearer ${token}` } : {}),
      ...(body ? { "content-type": "application/json" } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await response.text();
  return { status: response.status, body: text ? JSON.parse(text) : null };
}

async function createAccount(accountId) {
  const created = await request("/v1/accounts", { method: "POST", body: { account_id: accountId, password: "test-password-123" } });
  assert.equal(created.status, 201, JSON.stringify(created.body));
  const login = await request("/v1/sessions", { method: "POST", body: { account_id: accountId, password: "test-password-123" } });
  assert.equal(login.status, 200, JSON.stringify(login.body));
  return login.body.access_token;
}

const ownerToken = await createAccount(owner);
const observerToken = await createAccount(observer);

assert.equal((await request("/v1/scopes", { token: ownerToken, method: "POST", body: { scope_id: scopeId, name: "Recovery smoke", world_epoch: "epoch-1" } })).status, 201);
assert.equal((await request("/v1/targets", { token: ownerToken, method: "POST", body: { scope_id: scopeId, target_id: targetId, kind: "PLAYER", display_name: "Recovery target" } })).status, 201);
assert.equal((await request(`/v1/scopes/${scopeId}/acl`, { token: ownerToken, method: "PUT", body: { account_id: observer, role: "viewer" } })).status, 200);

const appearance = await request(`/v1/targets/${targetId}/appearance`, {
  token: ownerToken,
  method: "PUT",
  body: { expected_revision: 0, texture_id: "texture-smoke", disabled: false },
});
assert.equal(appearance.status, 200, JSON.stringify(appearance.body));
const staleAppearance = await request(`/v1/targets/${targetId}/appearance`, {
  token: ownerToken,
  method: "PUT",
  body: { expected_revision: 0, texture_id: "must-not-commit", disabled: false },
});
assert.equal(staleAppearance.status, 409, "stale appearance revisions must not commit state or outbox rows");

const animation = await request(`/v1/targets/${targetId}/animation`, {
  token: ownerToken,
  method: "PUT",
  body: { expected_revision: 0, lease_id: "lease-smoke", lease_ttl_ms: 30_000, channel: "body", action: "PLAY", animation_key: "run" },
});
assert.equal(animation.status, 200, JSON.stringify(animation.body));

const firstPage = await request(`/v1/scopes/${scopeId}/events/recovery?after=0&limit=1`, { token: ownerToken });
assert.equal(firstPage.status, 200, JSON.stringify(firstPage.body));
assert.equal(firstPage.body.entries.length, 1);
assert.equal(firstPage.body.has_more, true);
assert.equal(firstPage.body.entries[0].kind, "APPEARANCE_UPDATED");
assert.equal(firstPage.body.entries[0].payload.revision, 1);

const secondPage = await request(`/v1/scopes/${scopeId}/events/recovery?after=${firstPage.body.to_cursor}&limit=1`, { token: ownerToken });
assert.equal(secondPage.status, 200, JSON.stringify(secondPage.body));
assert.equal(secondPage.body.entries.length, 1);
assert.equal(secondPage.body.entries[0].kind, "AnimationState");
assert.equal(secondPage.body.entries[0].payload.animation_key, "run");
assert.equal(secondPage.body.has_more, false);

const observerRecovery = await request(`/v1/scopes/${scopeId}/events/recovery`, { token: observerToken });
assert.equal(observerRecovery.status, 200, JSON.stringify(observerRecovery.body));
assert.equal(observerRecovery.body.entries.length, 0, "scope viewers must not recover targets they cannot view");

const leaseConflict = await request(`/v1/targets/${targetId}/animation`, {
  token: ownerToken,
  method: "PUT",
  body: { expected_revision: 1, lease_id: "wrong-lease", lease_ttl_ms: 30_000, channel: "body", action: "STOP", animation_key: "" },
});
assert.equal(leaseConflict.status, 409, "an active animation lease cannot be replaced by a different lease id");

console.log("Cloudflare D1 recovery smoke passed");
