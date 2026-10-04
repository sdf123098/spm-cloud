import assert from "node:assert/strict";

const origin = process.env.SPM_CLOUD_TEST_ORIGIN;
if (!origin) throw new Error("SPM_CLOUD_TEST_ORIGIN must point at an isolated local Wrangler instance");

async function request(path, { token, method = "GET", body } = {}) {
  const response = await fetch(new URL(path, origin), {
    method,
    headers: {
      ...(token ? { authorization: `Bearer ${token}` } : {}),
      ...(body ? { "content-type": "application/json" } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  return { status: response.status, body: await response.json() };
}

const providers = await request("/v1/identity-providers");
assert.equal(providers.status, 200);
assert.ok(providers.body.some(provider => provider.provider_id === "official" && provider.enabled));

const id = crypto.randomUUID().replaceAll("-", "");
const account = `game_auth_${id}`;
assert.equal((await request("/v1/accounts", {
  method: "POST", body: { account_id: account, password: "test-password-123" },
})).status, 201);
const login = await request("/v1/sessions", {
  method: "POST", body: { account_id: account, password: "test-password-123" },
});
assert.equal(login.status, 200);
const token = login.body.access_token;
const profile = { provider_id: "official", username: "Player", profile_uuid: "123456781234123412341234567890ab" };

assert.equal((await request("/v1/auth/challenges", { method: "POST", body: profile })).status, 401);
const link = await request("/v1/auth/challenges", { token, method: "POST", body: profile });
assert.equal(link.status, 200, JSON.stringify(link.body));
assert.ok(link.body.challenge_id.startsWith("challenge_"));
assert.equal(link.body.server_id.length, 32);
const crossPurpose = await request(`/v1/auth/login-challenges/${link.body.challenge_id}/complete`, {
  method: "POST", body: { challenge_id: link.body.challenge_id },
});
assert.equal(crossPurpose.status, 401, "a link challenge must never issue an anonymous session");

const gameLogin = await request("/v1/auth/login-challenges", { method: "POST", body: profile });
assert.equal(gameLogin.status, 200, JSON.stringify(gameLogin.body));
assert.ok(gameLogin.body.challenge_id !== link.body.challenge_id);
const mismatched = await request(`/v1/auth/login-challenges/${gameLogin.body.challenge_id}/complete`, {
  method: "POST", body: { challenge_id: link.body.challenge_id },
});
assert.equal(mismatched.status, 400);
const unknownProvider = await request("/v1/auth/login-challenges", {
  method: "POST", body: { ...profile, provider_id: "untrusted-provider" },
});
assert.equal(unknownProvider.status, 403);

console.log("Cloudflare game identity challenge smoke passed");
