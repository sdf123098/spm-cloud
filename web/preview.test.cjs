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

test('card layers resolve relative to ysm.json without taking another packs same-named image', () => {
  const assets=context.SPMPreview.cardAssets({pack:{'pack/ysm.json':{metadata:{name:'作者卡片'},properties:{gui_background:'art/bg.png',gui_foreground:'art/frame.png'}}},images:[{path:'other/art/bg.png',data:'wrong'},{path:'pack/art/bg.png',data:'background'},{path:'pack/art/frame.png',data:'foreground'}]});
  assert.equal(assets.background,'background');assert.equal(assets.foreground,'foreground');assert.equal(assets.name,'作者卡片');
});
test('declared missing card artwork is reported rather than silently replaced', () => {
  assert.throws(()=>context.SPMPreview.cardAssets({pack:{'ysm.json':{properties:{gui_foreground:'missing.png'}}},images:[]}),/卡片素材缺失/);
});

test('authored preview animation binds named bones and hides zero-scale accessories', async()=>{
  const payload={documents:[{path:'main.json',value:{bones:[{name:'accessory',cubes:[{origin:[0,0,0],size:[1,1,1]}]}]}}],images:[],pack:{'ysm.json':{properties:{preview_animation:'gui'}}},animations:[{value:{animations:{gui:{bones:{accessory:{scale:0}}}}}}]};
  const model=await context.SPMPreview.buildModel(payload);
  const applied=context.SPMPreview.applyPreviewPose(model,payload);
  assert.equal(applied.bones,1);assert.equal(model.getObjectByName('accessory').visible,false);
});

test('Bedrock cubes with per-face UVs leave omitted faces invisible', async()=>{
  const model=await context.SPMPreview.buildModel({documents:[{path:'main.json',value:{bones:[{name:'body',cubes:[{origin:[0,0,0],size:[1,1,1],uv:{north:{uv:[0,0],uv_size:[1,1]}}}]}]}}],images:[]});
  const mesh=model.children[0].children[0].children[0];
  assert.equal(mesh.material.filter(material=>material.visible).length,1);
});
