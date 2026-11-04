// End-to-end proof that the renderer produces a real PNG from the payload the
// server serves, through a real headless chromium. The unit suite (web/) can only
// test buildModel() against a stub; this is the only place that proves WebGL,
// the page glue and the PNG encoding all work together.
//
// Skipped, not failed, when chromium is absent: `cargo test` on a host without a
// browser must stay green. CI installs it, so CI always runs the assertion.
import {test} from 'node:test';
import assert from 'node:assert/strict';
import {openRenderer} from './render-thumbnails.mjs';

// Shape matches what `/admin/api/assets/{id}/preview` returns: documents + images.
// `buildModel` picks the richest geometry document and expects `outliner` to be
// present, so an inline Bedrock geometry is wrapped exactly the way preview.rs
// wraps a parsed .json.
const BBMODEL = JSON.stringify({
  documents: [{
    path: 'models/main.json',
    value: {
      'minecraft:geometry': [{
        description: {texture_width: 16, texture_height: 16},
        bones: [{name: 'body', pivot: [0, 0, 0], cubes: [{origin: [-4, 0, -4], size: [8, 24, 8], uv: [0, 0]}]}],
      }],
    },
  }],
  images: [],
});

test('a bbmodel payload renders to a portrait YSM card PNG', async (t) => {
  let renderer;
  try {
    renderer = await openRenderer({concurrency: 1, size: 512});
  } catch (error) {
    t.skip(`chromium unavailable: ${error.message}`);
    return;
  }
  try {
    const result = await renderer.render(BBMODEL);
    assert.equal(result.ok, true, `render failed: ${result.detail}`);
    // PNG magic, then IHDR width/height — proves it is an image of the right
    // size, not just a non-empty buffer.
    assert.deepEqual([...result.png.subarray(0, 8)], [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    assert.equal(result.png.readUInt32BE(16), Math.round(512*52/90));
    assert.equal(result.png.readUInt32BE(20), 512);
    // A flat-colour 512×512 render is small; a blank/black frame would be even
    // smaller, so this also catches "rendered an empty scene".
    assert.ok(result.png.length > 1000, `suspiciously small PNG: ${result.png.length} bytes`);
  } finally {
    await renderer.close();
  }
});

test('a payload with no geometry fails instead of producing a blank image', async (t) => {
  let renderer;
  try {
    renderer = await openRenderer({concurrency: 1, size: 256});
  } catch (error) {
    t.skip(`chromium unavailable: ${error.message}`);
    return;
  }
  try {
    const result = await renderer.render(JSON.stringify({documents: [], images: []}));
    assert.equal(result.ok, false);
    assert.match(result.detail, /几何|geometry/i);
  } finally {
    await renderer.close();
  }
});
