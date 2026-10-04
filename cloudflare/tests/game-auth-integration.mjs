import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { generateKeyPairSync, sign } from "node:crypto";
import { Miniflare } from "miniflare";

const bundle = process.env.SPM_CLOUD_TEST_BUNDLE;
if (!bundle) throw new Error("Set SPM_CLOUD_TEST_BUNDLE to the Wrangler dry-run index.js bundle");
const bundlePath = resolve(bundle);
const canonicalOrigin = "https://micafic.xyz";
const legacyOrigin = "https://spm-cloud-official.robinson171.workers.dev";

const joined = new Map();
const forcedResponses = new Map();
const outboundPaths = [];
const certificateRoot = generateKeyPairSync("rsa", { modulusLength: 4096 });
const playerKey = generateKeyPairSync("rsa", { modulusLength: 2048 });
const publicRoot = certificateRoot.publicKey.export({ type: "spki", format: "der" }).toString("base64");
const miniflare = new Miniflare({
  workers: [{
    config: {
      name: "spm-cloud-integration",
      compatibilityDate: "2026-09-22",
      manifest: {
        mainModule: "index.js",
        modulesRoot: dirname(bundlePath),
        modules: { "index.js": { type: "esm", contents: await readFile(bundlePath, "utf8") } },
      },
      env: {
        SPM_CLOUD_ACCESS_TOKEN: { type: "text", value: "integration-operator-secret" },
        SPM_CLOUD_INSTANCE_ID: { type: "text", value: "official" },
        SPM_CLOUD_ORIGIN: { type: "text", value: canonicalOrigin },
        SPM_CLOUD_LEGACY_ORIGIN: { type: "text", value: legacyOrigin },
        DB: { type: "d1", name: "spm-cloud-integration" },
      },
    },
    dev: {
      outboundService: { type: "fetcher", handler: async request => {
        const url = new URL(request.url);
        outboundPaths.push(`${url.host}${url.pathname}`);
        if (url.href === "https://api.minecraftservices.com/publickeys") {
          return Response.json({ playerCertificateKeys: [{ publicKey: publicRoot }] });
        }
        const forced = forcedResponses.get(url.searchParams.get("serverId"));
        if (forced) return new Response(forced.body, { status: forced.status, headers: forced.headers });
        const profile = joined.get(url.searchParams.get("serverId"));
        return profile
          ? new Response(JSON.stringify(profile), { headers: { "content-type": "application/json" } })
          : new Response(null, { status: 204 });
      } },
    },
  }],
});

async function call(path, { method = "GET", token, body, requester = "192.0.2.1", origin = canonicalOrigin } = {}) {
  const response = await miniflare.dispatchFetch(`${origin}${path}`, {
    method,
    headers: {
      "cf-connecting-ip": requester,
      ...(token ? { authorization: `Bearer ${token}` } : {}),
      ...(body ? { "content-type": "application/json" } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  const payload = response.status === 204 ? null : await response.json();
  return { status: response.status, body: payload };
}

function profileKeyProof(challenge, uuid, {
  expiresAt = Date.now() + 3_600_000, root = certificateRoot,
  signingKey = playerKey.privateKey, payload = challenge.profile_key_payload,
} = {}) {
  const publicKey = playerKey.publicKey.export({ type: "spki", format: "der" });
  const certificate = Buffer.alloc(24 + publicKey.length);
  Buffer.from(uuid.replaceAll("-", ""), "hex").copy(certificate);
  certificate.writeBigInt64BE(BigInt(expiresAt), 16);
  publicKey.copy(certificate, 24);
  return {
    public_key: publicKey.toString("base64"), expires_at_ms: expiresAt,
    key_signature: sign("RSA-SHA1", certificate, root.privateKey).toString("base64"),
    challenge_signature: sign("RSA-SHA256", Buffer.from(payload), signingKey).toString("base64"),
  };
}

async function challengeAndComplete(profile, purpose, token) {
  const route = purpose === "link" ? "/v1/auth/challenges" : "/v1/auth/login-challenges";
  const challenge = await call(route, { method: "POST", token, body: profile });
  assert.equal(challenge.status, 200, JSON.stringify(challenge.body));
  joined.set(challenge.body.server_id, { id: profile.profile_uuid.replaceAll("-", ""), name: profile.username });
  const completed = await call(`${route}/${challenge.body.challenge_id}/complete`, {
    method: "POST", token, body: { challenge_id: challenge.body.challenge_id },
  });
  return { challenge, completed };
}

try {
  for (const origin of [canonicalOrigin, legacyOrigin]) {
    const instance = await call("/v1/instance", { origin });
    assert.equal(instance.status, 200);
    assert.equal(instance.body.origin, origin, "discovery must match the trusted origin used by the client");
    assert.equal(instance.body.websocket_origin, origin.replace(/^http/, "ws") + "/v1/realtime");
  }
  assert.equal((await call("/v1/instance", { origin: "https://untrusted.example" })).body.origin, canonicalOrigin,
    "untrusted request hosts must not become advertised or signed Cloud origins");
  const db = await miniflare.getD1Database("DB");
  for (const file of (await readdir("migrations")).filter(name => name.endsWith(".sql")).sort()) {
    const sql = await readFile(resolve("migrations", file), "utf8");
    for (const statement of sql.split(";").map(part => part.trim()).filter(Boolean)) {
      await db.prepare(statement).run();
    }
  }

  const providers = await call("/v1/identity-providers");
  assert.equal(providers.status, 200);
  for (const id of ["official", "littleskin", "elyby", "drasl_unmojang"]) {
    assert.ok(providers.body.some(provider => provider.provider_id === id), `${id} should be available`);
  }

  const accountId = `integration_${crypto.randomUUID().replaceAll("-", "")}`;
  assert.equal((await call("/v1/accounts", {
    method: "POST", body: { account_id: accountId, password: "test-password-123" },
  })).status, 201);
  const session = await call("/v1/sessions", {
    method: "POST", body: { account_id: accountId, password: "test-password-123" },
  });
  assert.equal(session.status, 200, JSON.stringify(session.body));
  const token = session.body.access_token;

  for (const [provider_id, expectedPath] of [
    ["official", "sessionserver.mojang.com/session/minecraft/hasJoined"],
    ["littleskin", "littleskin.cn/api/yggdrasil/sessionserver/session/minecraft/hasJoined"],
    ["elyby", "authserver.ely.by/session/hasJoined"],
    ["drasl_unmojang", "drasl.unmojang.org/session/minecraft/hasJoined"],
  ]) {
    const profile = { provider_id, username: `Player_${provider_id}`, profile_uuid: crypto.randomUUID() };
    const linked = await challengeAndComplete(profile, "link", token);
    assert.equal(linked.completed.status, 200, JSON.stringify(linked.completed.body));
    assert.equal(linked.completed.body.account_id, accountId);
    assert.equal(outboundPaths.at(-1), expectedPath);

    const login = await challengeAndComplete(profile, "login");
    assert.equal(login.completed.status, 200, JSON.stringify(login.completed.body));
    assert.equal(login.completed.body.account_id, accountId);
    assert.ok(login.completed.body.access_token);
    assert.equal(outboundPaths.at(-1), expectedPath);
    assert.equal((await call("/v1/identities", { token: login.completed.body.access_token })).status, 200);
    assert.equal((await call(`/v1/auth/login-challenges/${login.challenge.body.challenge_id}/complete`, {
      method: "POST", body: { challenge_id: login.challenge.body.challenge_id },
    })).status, 401, "login challenges must not be reusable");
  }

  const secondAccount = `integration_${crypto.randomUUID().replaceAll("-", "")}`;
  assert.equal((await call("/v1/accounts", {
    method: "POST", body: { account_id: secondAccount, password: "test-password-123" },
  })).status, 201);
  const secondSession = await call("/v1/sessions", {
    method: "POST", body: { account_id: secondAccount, password: "test-password-123" },
  });
  const taken = {
    provider_id: "official", username: "Player_official",
    profile_uuid: (await db.prepare("SELECT profile_uuid FROM identities WHERE identity_kind = 'official' LIMIT 1").first()).profile_uuid,
  };
  const conflict = await challengeAndComplete(taken, "link", secondSession.body.access_token);
  assert.equal(conflict.completed.status, 409, JSON.stringify(conflict.completed.body));

  const customProvider = {
    provider_id: "blessing-example", display_name: "Blessing Skin example",
    base_url: "https://skin.example.com/api/yggdrasil", enabled: true,
  };
  assert.equal((await call("/v1/identity-providers", {
    method: "POST", token, body: customProvider,
  })).status, 403, "Cloud users must not configure trusted identity providers");
  assert.equal((await call("/v1/identity-providers", {
    method: "POST", token: "integration-operator-secret", body: customProvider,
  })).status, 200, "the operator must be able to configure a trusted provider");
  const customProfile = {
    provider_id: "blessing-example", username: "Player_Blessing", profile_uuid: crypto.randomUUID(),
  };
  assert.equal((await challengeAndComplete(customProfile, "link", token)).completed.status, 200);
  assert.equal(outboundPaths.at(-1), "skin.example.com/api/yggdrasil/sessionserver/session/minecraft/hasJoined");

  const privateDrasl = {
    provider_id: "private-drasl", display_name: "Private Drasl",
    base_url: "https://drasl.example.com", session_path: "/session/minecraft/hasJoined", enabled: true,
  };
  assert.equal((await call("/v1/identity-providers", {
    method: "POST", token: "integration-operator-secret", body: privateDrasl,
  })).status, 200);
  assert.equal((await challengeAndComplete({
    provider_id: "private-drasl", username: "PrivatePlayer", profile_uuid: crypto.randomUUID(),
  }, "link", token)).completed.status, 200);
  assert.equal(outboundPaths.at(-1), "drasl.example.com/session/minecraft/hasJoined");

  assert.equal((await call("/v1/identity-providers", {
    method: "POST", token: "integration-operator-secret", body: {
      ...privateDrasl, provider_id: "invalid-url", base_url: "http://127.0.0.1:8080",
    },
  })).status, 400, "private or insecure provider URLs must be rejected");

  for (const [provider_id, upstream, expectedStatus, expectedCode] of [
    ["elyby", { status: 401, body: JSON.stringify({ error: "ForbiddenOperationException", errorMessage: "Invalid token." }), headers: { "content-type": "application/json" } }, 403, "IDENTITY_PROFILE_MISMATCH"],
    ["drasl_unmojang", { status: 403, body: null }, 403, "IDENTITY_PROFILE_MISMATCH"],
    ["official", { status: 403, body: "<html>Access denied</html>", headers: { "content-type": "text/html" } }, 502, "IDENTITY_PROVIDER_UNAVAILABLE"],
    ["drasl_unmojang", { status: 403, body: "<html>Access denied</html>", headers: { "content-type": "text/html" } }, 502, "IDENTITY_PROVIDER_UNAVAILABLE"],
  ]) {
    const challenge = await call("/v1/auth/login-challenges", {
      method: "POST", body: { provider_id, username: "UnjoinedPlayer", profile_uuid: crypto.randomUUID() },
    });
    assert.equal(challenge.status, 200);
    forcedResponses.set(challenge.body.server_id, upstream);
    const result = await call(`/v1/auth/login-challenges/${challenge.body.challenge_id}/complete`, {
      method: "POST", body: { challenge_id: challenge.body.challenge_id },
    });
    assert.equal(result.status, expectedStatus, JSON.stringify(result.body));
    assert.equal(result.body.code, expectedCode);
    assert.equal(result.body.access_token, undefined, "rejected upstream responses must never issue sessions");
  }
  const certifiedProfile = { provider_id: "official", username: "CertifiedPlayer", profile_uuid: crypto.randomUUID() };
  async function keyChallenge(purpose = "login", profile = certifiedProfile, origin = canonicalOrigin) {
    const route = purpose === "link" ? "/v1/auth/challenges" : "/v1/auth/login-challenges";
    const response = await call(route, {
      method: "POST", token: purpose === "link" ? token : undefined, body: profile, requester: "192.0.2.2", origin,
    });
    assert.equal(response.status, 200, JSON.stringify(response.body));
    assert.ok(response.body.profile_key_payload?.includes(profile.profile_uuid), "official challenges must offer a profile key proof");
    assert.equal(response.body.profile_key_payload.split("\n")[1], origin);
    return { route, body: response.body, token: purpose === "link" ? token : undefined, origin };
  }
  async function completeKey(challenge, proof, origin = challenge.origin) {
    return call(`${challenge.route}/${challenge.body.challenge_id}/complete`, {
      method: "POST", token: challenge.token, origin,
      body: { challenge_id: challenge.body.challenge_id, profile_key: proof },
    });
  }
  const beforeCertificates = outboundPaths.length;
  const certifiedLink = await keyChallenge("link");
  assert.equal((await completeKey(certifiedLink, profileKeyProof(certifiedLink.body, certifiedProfile.profile_uuid))).status, 200);
  const certifiedLogin = await keyChallenge();
  const validProof = profileKeyProof(certifiedLogin.body, certifiedProfile.profile_uuid);
  const recovered = await completeKey(certifiedLogin, validProof);
  assert.equal(recovered.status, 200, JSON.stringify(recovered.body));
  assert.equal(recovered.body.account_id, accountId);
  assert.ok(recovered.body.access_token);
  assert.equal((await completeKey(certifiedLogin, validProof)).status, 401, "signed challenges must also be one-use");
  assert.ok(!outboundPaths.slice(beforeCertificates).some(path => path.includes("hasJoined")), "certificate proof must not depend on hasJoined");

  const legacyLogin = await keyChallenge("login", certifiedProfile, legacyOrigin);
  const legacySession = await completeKey(legacyLogin, profileKeyProof(legacyLogin.body, certifiedProfile.profile_uuid));
  assert.equal(legacySession.status, 200, JSON.stringify(legacySession.body));
  assert.equal(legacySession.body.account_id, accountId, "both domains must recover the same existing account");
  const boundToCanonical = await keyChallenge();
  assert.equal((await completeKey(boundToCanonical,
    profileKeyProof(boundToCanonical.body, certifiedProfile.profile_uuid), legacyOrigin)).status, 403,
    "a signed proof must not be replayed across Cloud origins");

  const forgedRoot = generateKeyPairSync("rsa", { modulusLength: 2048 });
  for (const [label, options] of [
    ["signature copied to another challenge", { payload: certifiedLogin.body.profile_key_payload }],
    ["certificate without its private key", { signingKey: forgedRoot.privateKey }],
    ["expired certificate", { expiresAt: Date.now() - 1_000 }],
    ["certificate signed by a third party", { root: forgedRoot }],
    ["login using a link signature", { payload: certifiedLink.body.profile_key_payload }],
  ]) {
    const challenge = await keyChallenge();
    const rejected = await completeKey(challenge, profileKeyProof(challenge.body, certifiedProfile.profile_uuid, options));
    assert.equal(rejected.status, 403, `${label}: ${JSON.stringify(rejected.body)}`);
    assert.equal(rejected.body.code, "IDENTITY_PROFILE_MISMATCH");
    assert.equal(rejected.body.access_token, undefined);
  }
  const wrongUuid = await keyChallenge("login", { ...certifiedProfile, profile_uuid: crypto.randomUUID() });
  assert.equal((await completeKey(wrongUuid, profileKeyProof(wrongUuid.body, certifiedProfile.profile_uuid))).status, 403);
  const malformedProof = await keyChallenge();
  assert.equal((await completeKey(malformedProof, {})).status, 403, "invalid certificate must not fall back to session verification");
  console.log("Cloudflare game identity integration passed (session providers and certificate proof)");
} finally {
  await miniflare.dispose();
}
