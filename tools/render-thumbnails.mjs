#!/usr/bin/env node
// Batch thumbnail renderer for spm-cloud.
//
// Runs OUTSIDE the API process on purpose: a headless chromium is 200–400 MB and
// an OOM there must not take the service with it. This script pulls the queue
// from the server, renders each model with the same preview code the admin UI
// uses, and PUTs the PNG back.
//
//   node tools/render-thumbnails.mjs --site https://api.micaftic.top \
//     --token "$TOKEN" --concurrency 4 --limit 200
//
// Needs a source checkout (it serves web/preview.bundle.js to the browser), not
// the release tarball, where the preview bundle only exists as compiled bytes.
//
// Exit codes: 0 = every queued item settled (rendered or marked failed),
//             1 = at least one item errored in a way worth retrying next run,
//             2 = the run could not start (no browser, bad flags, queue down).

import {chromium} from 'playwright';
import {createServer} from 'node:http';
import {createReadStream} from 'node:fs';
import {stat} from 'node:fs/promises';
import {fileURLToPath, pathToFileURL} from 'node:url';
import path from 'node:path';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function parseArgs(argv) {
  const args = {site: '', token: '', concurrency: 2, limit: 200, size: 512, model: '', dryRun: false};
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const value = () => {
      const next = argv[++i];
      if (next === undefined) throw new Error(`${flag} needs a value`);
      return next;
    };
    switch (flag) {
      case '--site': args.site = value().replace(/\/+$/, ''); break;
      case '--token': args.token = value(); break;
      case '--concurrency': args.concurrency = Number(value()); break;
      case '--limit': args.limit = Number(value()); break;
      case '--size': args.size = Number(value()); break;
      case '--model': args.model = value(); break;
      case '--dry-run': args.dryRun = true; break;
      case '--help': case '-h': args.help = true; break;
      default: throw new Error(`unknown flag ${flag}`);
    }
  }
  if (args.help) return args;
  if (!args.site) throw new Error('--site is required');
  if (!args.token) throw new Error('--token is required (admin bootstrap or session token)');
  if (!Number.isFinite(args.concurrency) || args.concurrency < 1 || args.concurrency > 8) {
    throw new Error('--concurrency must be 1..8');
  }
  return args;
}

const USAGE = `usage: node tools/render-thumbnails.mjs --site <origin> --token <token>
       [--concurrency 4] [--limit 200] [--size 512] [--model <asset-id>] [--dry-run]`;

/**
 * Owns the browser, its pages, and the loopback server that feeds them the
 * renderer page and preview payloads. Exported so the CI test renders through
 * exactly this path instead of a parallel copy that could drift.
 */
export async function openRenderer({concurrency = 2, size = 512} = {}) {
  // Payloads are handed to the page over a loopback URL rather than injected,
  // which keeps `thumbnail.html` a plain static page that can also be opened by
  // hand for debugging.
  const payloads = new Map();
  let payloadId = 0;
  const server = createServer((request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (url.pathname === '/preview.bundle.js') return sendFile(response, path.join(ROOT, 'web/preview.bundle.js'), 'text/javascript');
    if (url.pathname === '/thumbnail.html') return sendFile(response, path.join(ROOT, 'tools/thumbnail.html'), 'text/html');
    if (url.pathname.startsWith('/payload/')) {
      const payload = payloads.get(url.pathname.slice('/payload/'.length));
      if (!payload) { response.writeHead(404).end(); return; }
      response.writeHead(200, {'content-type': 'application/json'}).end(payload);
      return;
    }
    response.writeHead(404).end();
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const browser = await chromium.launch({args: ['--use-gl=swiftshader', '--enable-unsafe-swiftshader', '--no-sandbox']});
  const contexts = await Promise.all(Array.from({length: concurrency}, () => browser.newContext()));
  const pages = await Promise.all(contexts.map(context => context.newPage()));
  let pageCursor = 0;

  return {
    /** Returns `{ok: true, png: Buffer}` or `{ok: false, detail}`. */
    async render(payload) {
      const key = String(++payloadId);
      payloads.set(key, typeof payload === 'string' ? payload : JSON.stringify(payload));
      const page = pages[pageCursor++ % pages.length];
      try {
        await page.goto(`${origin}/thumbnail.html?payload=/payload/${key}&size=${size}`, {waitUntil: 'load'});
        await page.waitForFunction(
          () => window.__THUMBNAIL_STATE__ && window.__THUMBNAIL_STATE__ !== 'loading',
          null,
          {timeout: 30000},
        );
        const state = await page.evaluate(() => window.__THUMBNAIL_STATE__);
        if (state !== 'ready') {
          return {ok: false, detail: await page.evaluate(() => window.__THUMBNAIL_DETAIL__)};
        }
        const dataUrl = await page.evaluate(() => window.__THUMBNAIL_DATA_URL__);
        return {ok: true, png: Buffer.from(dataUrl.slice(dataUrl.indexOf(',') + 1), 'base64')};
      } finally {
        payloads.delete(key);
      }
    },
    async close() {
      await Promise.all(contexts.map(context => context.close()));
      await browser.close();
      server.close();
    },
  };
}

async function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    console.error(USAGE);
    return 2;
  }
  if (args.help) { console.log(USAGE); return 0; }

  const headers = {authorization: `Bearer ${args.token}`};
  const api = async (method, route, body) => {
    const response = await fetch(`${args.site}${route}`, {method, headers, body});
    return {status: response.status, text: await response.text(), response};
  };
  const markFailed = entry => api('POST', `/admin/api/assets/${encodeURIComponent(entry.asset_id)}/revisions/${entry.revision}/thumbnail/failed`);

  let work;
  if (args.model) {
    work = {entries: [{asset_id: args.model, revision: 0}], remaining: 1};
    const listed = await api('GET', `/v1/assets?scope=accessible&limit=80`);
    if (listed.status === 200) {
      const found = JSON.parse(listed.text).entries?.find(item => item.asset_id === args.model);
      if (found) Object.assign(work.entries[0], {revision: found.revision});
    }
    if (!work.entries[0].revision) {
      console.error(`cannot resolve ${args.model} to a current revision`);
      return 2;
    }
    work.entries[0].preview_url = `/admin/api/assets/${encodeURIComponent(args.model)}/preview`;
  } else {
    const response = await api('GET', `/admin/api/thumbnails/pending?limit=${args.limit}`);
    if (response.status !== 200) {
      console.error(`queue request failed: ${response.status} ${response.text.slice(0, 200)}`);
      return 2;
    }
    work = JSON.parse(response.text);
  }
  if (!work.entries.length) {
    console.log('queue empty — nothing to render');
    return 0;
  }
  console.log(`${work.entries.length} of ${work.remaining} queued item(s) selected`);
  if (args.dryRun) {
    for (const entry of work.entries) console.log(`  ${entry.asset_id} rev ${entry.revision} (${entry.format})`);
    return 0;
  }

  const stats = {ready: 0, failed: 0, error: 0};
  let renderer;
  try {
    renderer = await openRenderer({concurrency: args.concurrency, size: args.size});
  } catch (error) {
    console.error(`cannot launch chromium: ${error.message}`);
    console.error('install it with: (cd tools && npm ci && npx playwright install --with-deps chromium)');
    return 2;
  }

  async function run(entry) {
    const preview = await api('GET', entry.preview_url);
    if (preview.status !== 200) {
      // 4xx means this model will never preview (unsupported format, deleted).
      // Marking it failed is what stops the nightly run retrying it forever.
      if (preview.status >= 400 && preview.status < 500) {
        await markFailed(entry);
        stats.failed++;
        console.log(`FAILED ${entry.asset_id} rev ${entry.revision}: preview ${preview.status}`);
        return;
      }
      throw new Error(`preview answered ${preview.status}`);
    }
    const rendered = await renderer.render(preview.text);
    if (!rendered.ok) {
      await markFailed(entry);
      stats.failed++;
      console.log(`FAILED ${entry.asset_id} rev ${entry.revision}: ${rendered.detail}`);
      return;
    }
    const upload = await api(
      'PUT',
      `/admin/api/assets/${encodeURIComponent(entry.asset_id)}/revisions/${entry.revision}/thumbnail`,
      rendered.png,
    );
    if (upload.status !== 200) throw new Error(`upload answered ${upload.status} ${upload.text.slice(0, 160)}`);
    stats.ready++;
    console.log(`ok     ${entry.asset_id} rev ${entry.revision} ${rendered.png.length}B`);
  }

  // Small worker pool: each in-flight render already holds a chromium page, so
  // the browser — not the loop — is the ceiling.
  let next = 0;
  await Promise.all(Array.from({length: args.concurrency}, async () => {
    while (next < work.entries.length) {
      const entry = work.entries[next++];
      try {
        await run(entry);
      } catch (error) {
        stats.error++;
        console.error(`ERROR  ${entry.asset_id} rev ${entry.revision}: ${error.message}`);
      }
    }
  }));

  await renderer.close();
  console.log(`done: ${stats.ready} ready, ${stats.failed} failed, ${stats.error} error`);
  return stats.error ? 1 : 0;
}

function sendFile(response, file, type) {
  stat(file).then(
    () => response.writeHead(200, {'content-type': type}) && createReadStream(file).pipe(response),
    () => response.writeHead(404).end(),
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().then(code => process.exit(code), error => { console.error(error); process.exit(2); });
}
