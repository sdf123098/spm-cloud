// Runs the existing recovery smoke against an isolated Worker over real loopback HTTP.
import { createServer } from "node:http";
import { readFile, readdir } from "node:fs/promises";
import { build } from "esbuild";
import { Miniflare } from "miniflare";

const contents = process.env.SPM_CLOUD_TEST_BUNDLE
  ? await readFile(process.env.SPM_CLOUD_TEST_BUNDLE, "utf8")
  : (await build({ entryPoints: ["src/index.ts"], bundle: true, format: "esm", target: "es2022",
      write: false, external: ["cloudflare:workers"] })).outputFiles[0].text;
const worker = new Miniflare({ workers: [{ config: {
  name: "local-recovery-test", compatibilityDate: "2026-09-22",
  manifest: { mainModule: "index.js", modulesRoot: process.cwd(),
    modules: { "index.js": { type: "esm", contents } } },
  env: {
    SPM_CLOUD_INSTANCE_ID: { type: "text", value: "recovery-test" },
    SPM_CLOUD_ORIGIN: { type: "text", value: "https://cloud.test" },
    SPM_CLOUD_ALLOW_SELF_REGISTRATION: { type: "text", value: "true" },
    DB: { type: "d1", name: "isolated-recovery" },
  },
} }] });
const server = createServer(async (request, response) => {
  try {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const body = Buffer.concat(chunks);
    const reply = await worker.dispatchFetch("https://cloud.test" + request.url, {
      method: request.method, headers: request.headers, ...(body.length ? { body } : {}),
    });
    response.writeHead(reply.status, Object.fromEntries(reply.headers));
    response.end(Buffer.from(await reply.arrayBuffer()));
  } catch {
    response.writeHead(500, { "content-type": "application/json" });
    response.end('{"message":"isolated recovery request failed"}');
  }
});
try {
  const db = await worker.getD1Database("DB");
  for (const name of (await readdir("migrations")).filter(name => name.endsWith(".sql")).sort()) {
    for (const sql of (await readFile("migrations/" + name, "utf8")).split(";").map(sql => sql.trim()).filter(Boolean)) {
      await db.prepare(sql).run();
    }
  }
  await new Promise((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
  process.env.SPM_CLOUD_TEST_ORIGIN = "http://127.0.0.1:" + server.address().port;
  await import("./recovery-smoke.mjs");
} finally {
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
  await worker.dispose();
}
