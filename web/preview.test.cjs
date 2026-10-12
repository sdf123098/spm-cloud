const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const {test} = require('node:test');
const context = {console, AbortController, TextDecoder, TextEncoder, URL, atob, performance};
vm.runInNewContext(fs.readFileSync(__dirname + '/preview.bundle.js', 'utf8'), context);

test('YSM main geometry is displayed instead of the arm model in the same models directory', async () => {
  const body = name => ({'minecraft:geometry': [{bones: [{name, cubes: [{origin: [0,0,0], size: [8,16,8]}]}]}]});
  // The selected skeleton belongs to the full character, which follows arm.json in decoder output.
  const main = body('character-body');main['minecraft:geometry'][0].bones[0].cubes[0].size = [8,32,8];
  const selected = await context.SPMPreview.buildModel({documents:[{path:'model/models/arm.json',value:body('arm-only')},{path:'model/models/main.json',value:main}],images:[]});
  assert.equal(selected.children[0].children[0].children[0].geometry.parameters.height, 32);
});
