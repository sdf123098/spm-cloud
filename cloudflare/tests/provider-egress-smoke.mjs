import assert from "node:assert/strict";

const origin = process.env.SPM_CLOUD_TEST_ORIGIN;
if (!origin) throw new Error("SPM_CLOUD_TEST_ORIGIN must name the Worker to check");
const providers = process.argv.slice(2);
// Official authentication uses its certificate proof (test:official-key-egress).
if (!providers.length) providers.push("littleskin", "elyby", "drasl_unmojang");
let failures = 0;
for (const provider_id of providers) {
  try {
    const challengeResponse = await fetch(new URL("/v1/auth/login-challenges", origin), {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ provider_id, username: "SPM_EgressSmoke", profile_uuid: crypto.randomUUID() }),
    });
    const challenge = await challengeResponse.json();
    assert.equal(challengeResponse.status, 200, JSON.stringify(challenge));
    const response = await fetch(new URL(`/v1/auth/login-challenges/${challenge.challenge_id}/complete`, origin), {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ challenge_id: challenge.challenge_id }),
    });
    const result = await response.json();
    assert.equal(result.access_token, undefined, "an unjoined test profile must never receive a session");
    assert.equal(response.status, 403, JSON.stringify(result));
    assert.equal(result.code, "IDENTITY_PROFILE_MISMATCH");
    console.log(`${provider_id}: expected unjoined response`);
  } catch (error) {
    failures++;
    console.error(`${provider_id}: ${error.message}`);
  }
}
if (failures) process.exitCode = 1;
