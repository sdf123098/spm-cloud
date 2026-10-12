import assert from "node:assert/strict";
import proxy from "../src/compat-proxy.js";
let calls = 0;
globalThis.fetch = async (request, options) => {
  calls++;
  assert.equal(request.url, "https://micaftic.top/v1/assets?cursor=a%2Fb");
  assert.equal(request.method, "POST");
  assert.equal(request.headers.get("authorization"), "Bearer test-credential");
  assert.equal(request.headers.get("x-spm-compat-origin"), "https://spm-cloud-official.robinson171.workers.dev");
  assert.equal(await request.text(), "model-metadata");
  assert.equal(options.redirect, "manual");
  return new Response("origin-response", { status: 201 });
};
const env = { SPM_CLOUD_ORIGIN: "https://micaftic.top", SPM_CLOUD_PROXY_MAINTENANCE: "false" };
const request = new Request("https://spm-cloud-official.robinson171.workers.dev/v1/assets?cursor=a%2Fb", {
  method: "POST", body: "model-metadata", headers: { Authorization: "Bearer test-credential", "X-SPM-Compat-Origin": "https://attacker.example" },
});
const result = await proxy.fetch(request, env);
assert.equal(result.status, 201);
assert.equal(await result.text(), "origin-response");
assert.equal((await proxy.fetch(new Request("https://micafic.xyz/health"), { ...env, SPM_CLOUD_PROXY_MAINTENANCE: "true" })).status, 503);
assert.equal((await proxy.fetch(new Request("https://unknown.example/health"), env)).status, 421);
const redirect = await proxy.fetch(new Request("https://micafic.xyz/account/?a=b"), env);
assert.equal(redirect.status, 308);
assert.equal(redirect.headers.get("location"), "https://micaftic.top/account/?a=b");
assert.equal(calls, 1);
console.log("PASS: proxy preserves requests/responses, overwrites alias, blocks maintenance and unknown hosts, redirects browser UI");
