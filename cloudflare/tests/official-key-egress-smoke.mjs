import assert from "node:assert/strict";
import { generateKeyPairSync, sign } from "node:crypto";

const origin = process.env.SPM_CLOUD_TEST_ORIGIN;
if (!origin) throw new Error("SPM_CLOUD_TEST_ORIGIN must name the Worker to check");
const uuid = crypto.randomUUID();
const challengeResponse = await fetch(new URL("/v1/auth/login-challenges", origin), {
  method: "POST", headers: { "content-type": "application/json" },
  body: JSON.stringify({ provider_id: "official", username: "SPM_KeySmoke", profile_uuid: uuid }),
});
const challenge = await challengeResponse.json();
assert.equal(challengeResponse.status, 200, JSON.stringify(challenge));
assert.ok(challenge.profile_key_payload?.includes(uuid));
const key = generateKeyPairSync("rsa", { modulusLength: 2048 });
const publicKey = key.publicKey.export({ type: "spki", format: "der" });
const expiresAt = Date.now() + 60_000;
const certificate = Buffer.alloc(24 + publicKey.length);
Buffer.from(uuid.replaceAll("-", ""), "hex").copy(certificate);
certificate.writeBigInt64BE(BigInt(expiresAt), 16);
publicKey.copy(certificate, 24);
const response = await fetch(new URL(`/v1/auth/login-challenges/${challenge.challenge_id}/complete`, origin), {
  method: "POST", headers: { "content-type": "application/json" },
  body: JSON.stringify({ challenge_id: challenge.challenge_id, profile_key: {
    public_key: publicKey.toString("base64"), expires_at_ms: expiresAt,
    key_signature: sign("RSA-SHA1", certificate, key.privateKey).toString("base64"),
    challenge_signature: sign("RSA-SHA256", Buffer.from(challenge.profile_key_payload), key.privateKey).toString("base64"),
  } }),
});
const result = await response.json();
assert.equal(response.status, 403, JSON.stringify(result));
assert.equal(result.code, "IDENTITY_PROFILE_MISMATCH");
assert.equal(result.access_token, undefined);
console.log("Official player certificate path reached; self-signed certificate rejected without issuing a session");
