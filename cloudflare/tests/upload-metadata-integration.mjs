import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { resolve } from "node:path";
import { createHash } from "node:crypto";
import { build } from "esbuild";
import { Miniflare } from "miniflare";

const bundle = await build({
  entryPoints: ["src/index.ts"], bundle: true, format: "esm", target: "es2022", write: false,
  external: ["cloudflare:workers"],
});
const miniflare = new Miniflare({ workers: [{ config: {
  name: "upload-metadata-integration", compatibilityDate: "2026-09-22",
  manifest: { mainModule: "index.js", modulesRoot: process.cwd(), modules: {
    "index.js": { type: "esm", contents: bundle.outputFiles[0].text },
  } },
  env: {
    SPM_CLOUD_ACCESS_TOKEN: { type: "text", value: "isolated-upload-secret" },
    SPM_CLOUD_INSTANCE_ID: { type: "text", value: "upload-test" },
    SPM_CLOUD_ORIGIN: { type: "text", value: "https://cloud.example.test" },
    DB: { type: "d1", name: "upload-metadata-integration" },
    ASSETS: { type: "r2", name: "upload-metadata-assets" },
  },
} }] });

async function upload(id, name, body, encoding = "utf-8-percent", visibility = "PRIVATE", requestId = crypto.randomUUID()) {
  const sha = createHash("sha256").update(body).digest("hex");
  const encoded = value => encoding ? encodeURIComponent(value) : value;
  const response = await miniflare.dispatchFetch("https://cloud.example.test/v1/assets", {
    method: "POST", body,
    headers: {
      authorization: "Bearer isolated-upload-secret", "idempotency-key": requestId,
      "x-asset-id": encoded(id), "x-asset-name": encoded(name), "x-asset-format": "ysm",
      "x-asset-sha256": sha, ...(encoding ? { "x-asset-metadata-encoding": encoding } : {}),
      "x-asset-visibility": visibility,
    },
  });
  return { response, summary: await response.json(), sha };
}

async function setVisibility(id, visibility, authorization = "Bearer isolated-upload-secret") {
  const response = await miniflare.dispatchFetch(`https://cloud.example.test/v1/assets/${encodeURIComponent(id)}/visibility`, {
    method: "PUT", headers: { authorization, "content-type": "application/json" }, body: JSON.stringify({ visibility }),
  });
  return { status: response.status, body: await response.json() };
}

async function verifyDownloadAndAcl(id, revision, body) {
  const assetPath = `/v1/assets/${encodeURIComponent(id)}`;
  const headers = { authorization: "Bearer isolated-upload-secret" };
  const download = await miniflare.dispatchFetch(`https://cloud.example.test${assetPath}/revisions/${revision}/content`, { headers });
  assert.equal(download.status, 200, `${id} uploaded asset must be downloadable`);
  assert.deepEqual(Buffer.from(await download.arrayBuffer()), body);
  const acl = await miniflare.dispatchFetch(`https://cloud.example.test${assetPath}/acl`, { headers });
  assert.equal(acl.status, 200);
  assert.ok((await acl.json()).some(entry => entry.account_id === "account_local" && entry.permission === "manage"));
}

try {
  const db = await miniflare.getD1Database("DB");
  const migrations = (await readdir("migrations")).filter(name => name.endsWith(".sql")).sort();
  for (const file of migrations.filter(name => !name.startsWith("0006_"))) {
    for (const sql of (await readFile(resolve("migrations", file), "utf8")).split(";").map(part => part.trim()).filter(Boolean)) {
      await db.prepare(sql).run();
    }
  }
  const bucket = await miniflare.getR2Bucket("ASSETS");
  const repeatBytes = Buffer.from("same-original-model-bytes");
  const repeatId = "私有模型%+repeat";
  const original = await upload(repeatId, `${repeatId}.ysm`, repeatBytes);
  assert.equal(original.response.status, 201);
  const rowsBefore = await db.prepare("SELECT * FROM asset_revisions").all();
  const aclBefore = await db.prepare("SELECT * FROM asset_acl").all();
  for (const file of migrations.filter(name => name.startsWith("0006_"))) {
    const statements = (await readFile(resolve("migrations", file), "utf8")).split(";").map(part => part.trim()).filter(Boolean);
    await db.batch(statements.map(sql => db.prepare(sql)));
  }
  assert.deepEqual((await db.prepare("SELECT * FROM asset_revisions").all()).results, rowsBefore.results, "migration preserves every revision field");
  assert.deepEqual((await db.prepare("SELECT * FROM asset_acl").all()).results, aclBefore.results, "migration preserves permissions");
  const madePublic = await upload(repeatId, `${repeatId}.ysm`, repeatBytes, "utf-8-percent", "PUBLIC");
  assert.equal(madePublic.response.status, 201, JSON.stringify(madePublic.summary));
  assert.equal(madePublic.summary.revision, 2);
  assert.equal(madePublic.summary.visibility, "PUBLIC");
  const madePrivate = await upload(repeatId, `${repeatId}.ysm`, repeatBytes);
  assert.equal(madePrivate.response.status, 201);
  assert.equal(madePrivate.summary.revision, 3);
  assert.equal(madePrivate.summary.visibility, "PRIVATE");
  const sharedContent = await upload("same-bytes-second-asset", "second.ysm", repeatBytes);
  assert.equal(sharedContent.response.status, 201);
  assert.equal(sharedContent.summary.raw_sha256, original.sha);
  assert.deepEqual((await db.prepare("SELECT DISTINCT object_key FROM asset_revisions").all()).results, [{ object_key: `assets/${original.sha}` }]);

  const revisionCount = (await db.prepare("SELECT COUNT(*) AS count FROM asset_revisions").first()).count;
  const objectBefore = await bucket.get(`assets/${original.sha}`);
  for (const visibility of ["PUBLIC", "PUBLIC", "PRIVATE", "PRIVATE"]) {
    const changed = await setVisibility(repeatId, visibility);
    assert.equal(changed.status, 200, JSON.stringify(changed.body));
    assert.deepEqual(changed.body, { ...madePrivate.summary, visibility });
    assert.equal((await db.prepare("SELECT COUNT(*) AS count FROM asset_revisions").first()).count, revisionCount);
    assert.equal((await bucket.get(`assets/${original.sha}`)).uploaded.getTime(), objectBefore.uploaded.getTime(), "visibility changes never rewrite blobs");
  }
  const foreignAccount = "visibility_observer";
  assert.equal((await miniflare.dispatchFetch("https://cloud.example.test/v1/accounts", {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ account_id: foreignAccount, password: "test-password-123" }),
  })).status, 201);
  const login = await miniflare.dispatchFetch("https://cloud.example.test/v1/sessions", {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ account_id: foreignAccount, password: "test-password-123" }),
  });
  const foreignAuthorization = `Bearer ${(await login.json()).access_token}`;
  await db.prepare("INSERT INTO asset_acl(asset_id, account_id, permission) VALUES (?1, ?2, 'manage')").bind(repeatId, foreignAccount).run();
  const denied = await setVisibility(repeatId, "PUBLIC", foreignAuthorization);
  assert.equal(denied.status, 403, "ACL manage permission cannot change owner-only visibility");
  assert.equal(denied.body.code, "ASSET_ACCESS_DENIED");
  assert.equal((await setVisibility(repeatId, "UNLISTED")).status, 400);
  assert.equal((await setVisibility("missing-asset", "PUBLIC")).status, 404);
  const key = crypto.randomUUID();
  const firstIdempotent = await upload("idempotent-visibility", "idempotent.ysm", Buffer.from("idempotent"), "utf-8-percent", "PRIVATE", key);
  assert.equal(firstIdempotent.response.status, 201);
  const conflict = await upload("idempotent-visibility", "idempotent.ysm", Buffer.from("idempotent"), "utf-8-percent", "PUBLIC", key);
  assert.equal(conflict.response.status, 409, "visibility is part of an upload's idempotency identity");
  assert.equal(conflict.summary.code, "IDEMPOTENCY_CONFLICT");
  let serial = 0;
  for (const id of ["芙宁娜v3.14日语配音", "ascii-model", "模型 100%+测试"]) {
    const body = Buffer.from([0, 255, 13, 10, 42, serial++]);
    const name = `${id}.ysm`;
    const { response, summary, sha } = await upload(id, name, body);
    assert.equal(response.status, 201, JSON.stringify(summary));
    assert.equal(summary.asset_id, id);
    assert.equal(summary.name, name);
    assert.equal(summary.raw_sha256, sha);
    assert.equal(summary.byte_length, body.length);
    const object = await bucket.get(`assets/${sha}`);
    assert.deepEqual(Buffer.from(await object.arrayBuffer()), body);
    const stored = await db.prepare("SELECT name FROM asset_revisions WHERE asset_id = ?1").bind(id).first();
    assert.equal(stored.name, name);
    await verifyDownloadAndAcl(id, summary.revision, body);
  }
  const legacy = await upload("legacy%20+id", "literal%20+name.ysm", Buffer.from("legacy"), null);
  assert.equal(legacy.response.status, 201);
  assert.equal(legacy.summary.asset_id, "legacy%20+id");
  assert.equal(legacy.summary.name, "literal%20+name.ysm");
  await verifyDownloadAndAcl("legacy%20+id", legacy.summary.revision, Buffer.from("legacy"));

  if (process.env.SPM_CLOUD_TEST_MODEL) {
    const body = await readFile(process.env.SPM_CLOUD_TEST_MODEL);
    const id = "芙宁娜v3.14日语配音";
    const { response, summary, sha } = await upload(id, `${id}.ysm`, body);
    assert.equal(response.status, 201, JSON.stringify(summary));
    assert.equal(summary.asset_id, id);
    assert.equal(summary.name, `${id}.ysm`);
    assert.equal(summary.raw_sha256, sha);
    assert.equal(summary.byte_length, body.length);
    const object = await bucket.get(`assets/${sha}`);
    assert.deepEqual(Buffer.from(await object.arrayBuffer()), body);
    await verifyDownloadAndAcl(id, summary.revision, body);
    console.log(`User model verified: ${body.length} bytes, SHA-256 ${sha}`);
  }

  for (const [value, encoding] of [["bad%GG", "utf-8-percent"], ["bad%FF", "utf-8-percent"], ["bad%0Aname", "utf-8-percent"], ["ascii", "unsupported"]]) {
    const response = await miniflare.dispatchFetch("https://cloud.example.test/v1/assets", {
      method: "POST", body: "invalid",
      headers: { authorization: "Bearer isolated-upload-secret", "idempotency-key": crypto.randomUUID(),
        "x-asset-id": value, "x-asset-name": "name.ysm", "x-asset-metadata-encoding": encoding },
    });
    assert.equal(response.status, 400, `${value} must return INVALID_METADATA`);
    assert.equal((await response.json()).code, "INVALID_METADATA");
  }
  const options = await miniflare.dispatchFetch("https://cloud.example.test/v1/assets", { method: "OPTIONS" });
  assert.ok(options.headers.get("access-control-allow-headers").includes("x-asset-metadata-encoding"));
  console.log("Cloudflare upload metadata integration passed");
} finally {
  await miniflare.dispose();
}
