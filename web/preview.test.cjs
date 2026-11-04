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

test('authored preview animation keeps zero-scale accessories exactly hidden', async()=>{
  const payload={documents:[{path:'main.json',value:{bones:[{name:'accessory',cubes:[{origin:[0,0,0],size:[1,1,1]}]}]}}],images:[],pack:{'ysm.json':{properties:{preview_animation:'gui'}}},animations:[{value:{animations:{gui:{bones:{accessory:{scale:0}}}}}}]};
  const model=await context.SPMPreview.buildModel(payload);
  const applied=context.SPMPreview.applyPreviewPose(model,payload);
  const accessory=model.getObjectByName('accessory');
  assert.equal(applied.bones,1);assert.equal(accessory.scale.x,0);
});
test('Molang scale expressions evaluate in degrees at the card snapshot time', async()=>{
  const payload={documents:[{path:'main.json',value:{bones:[{name:'Root',cubes:[{origin:[0,0,0],size:[1,1,1]}]}]}}],images:[],pack:{'ysm.json':{properties:{preview_animation:'gui'}}},animations:[{value:{animations:{gui:{bones:{Root:{scale:['1-math.sin(-60+q.anim_time*180)*0.0025',1,1]}}}}}}]};
  const model=await context.SPMPreview.buildModel(payload);
  context.SPMPreview.applyPreviewPose(model,payload);
  const root=model.getObjectByName('Root');
  assert.ok(Math.abs(root.scale.x-(1-Math.sin(120*Math.PI/180)*.0025))<1e-9);assert.equal(root.scale.y,1);assert.equal(root.visible,true);
});

test('Furina scrolling backgrounds and Mingfeng root offsets evaluate authored Molang',async()=>{
  const tracks={ysmGlowbackground:{position:['-200+math.mod(q.anim_time*40,400)',0,0]},ysmGlowbackground2:{position:['-200+math.mod(200+query.anim_time*40,400)',0,0]},Root:{position:[-2,'-36+math.sin(q.anim_time*360/3)*0.5',0],scale:2.2}};
  const payload={documents:[{path:'main.json',value:{bones:Object.keys(tracks).map(name=>({name}))}}],images:[],pack:{'ysm.json':{properties:{preview_animation:'gui'}}},animations:[{value:{animations:{gui:{animation_length:1000,bones:tracks}}}}]};
  for(const time of [1,2]){
    const model=await context.SPMPreview.buildModel(payload);context.SPMPreview.applyPreviewPose(model,payload,time);
    assert.equal(model.getObjectByName('ysmGlowbackground').position.x,-200+time*40);
    assert.equal(model.getObjectByName('ysmGlowbackground2').position.x,time*40);
    assert.equal(model.getObjectByName('Root').position.x,-2);
    assert.ok(Math.abs(model.getObjectByName('Root').position.y-(-36+Math.sin(time*120*Math.PI/180)*.5))<1e-9);
  }
});

test('SPM parallel initialization hides effects and initializes variables around the GUI pose',async()=>{
  const animations={
    pre_parallel2:{bones:{effect:{scale:'!(0||!0)',position:[0,9,0]},Root:{position:[0,5,0]}}},
    gui:{bones:{effect:{position:['0/0',0,0]},Root:{position:[0,2,0],rotation:[0,20,0]}}},
    parallel1:{timeline:{'0':['[;',"'comment containing ysm.bone_rot(\'Root\').x';",'v.roaming.player_size=v.roaming.player_size?v.roaming.player_size:1;','];']},bones:{Root:{scale:'v.roaming.player_size',rotation:[0,10,0]}}},
  };
  const payload={documents:[{path:'main.json',value:{bones:[{name:'Root'},{name:'effect',parent:'Root'}]}}],images:[],pack:{'ysm.json':{properties:{preview_animation:'gui'}}},animations:[{value:{animations}}]};
  const model=await context.SPMPreview.buildModel(payload);context.SPMPreview.applyPreviewPose(model,payload,1);
  assert.equal(model.getObjectByName('effect').scale.x,0);
  assert.equal(model.getObjectByName('effect').position.y,9);
  assert.equal(model.getObjectByName('Root').scale.x,1);
  assert.equal(model.getObjectByName('Root').position.y,2);
  assert.ok(Math.abs(model.getObjectByName('Root').rotation.y-Math.PI/6)<1e-9);
});

test('unset model size uses its authored null-coalescing default while explicit zero stays hidden',async()=>{
  const payload={documents:[{path:'main.json',value:{bones:[{name:'Root'},{name:'accessory'}]}}],images:[],pack:{'ysm.json':{properties:{preview_animation:'gui'}}},animations:[{value:{animations:{parallel1:{timeline:{'0':['v.explicit_zero=0;']},bones:{Root:{scale:'v.player_size??1'},accessory:{scale:'v.explicit_zero??1'}}}}}}]};
  const model=await context.SPMPreview.buildModel(payload);context.SPMPreview.applyPreviewPose(model,payload);
  assert.equal(model.getObjectByName('Root').scale.x,1);
  assert.equal(model.getObjectByName('accessory').scale.x,0);
});

test('Bedrock cubes with per-face UVs leave omitted faces invisible', async()=>{
  const model=await context.SPMPreview.buildModel({documents:[{path:'main.json',value:{bones:[{name:'body',cubes:[{origin:[0,0,0],size:[1,1,1],uv:{north:{uv:[0,0],uv_size:[1,1]}}}]}]}}],images:[]});
  const mesh=model.children[0].children[0].children[0];
  assert.equal(mesh.material.filter(material=>material.visible).length,1);
});

test('card snapshot samples keyframe post values at one shared time', async()=>{
  const track={rotation:{'0':{post:[0,0,0]},'2':{pre:[0,90,0],post:[0,180,0]}},position:{'0':[0,0,0],'4':[0,8,0]},scale:{'0':{post:[1,1,1]},'2':{post:[2,2,2]}}};
  const payload={
    documents:[{path:'main.json',value:{bones:[{name:'body',pivot:[0,2,0],cubes:[{origin:[0,0,0],size:[1,1,1]}]}]}}],
    images:[],pack:{'ysm.json':{properties:{preview_animation:'gui'}}},
    animations:[{value:{animations:{gui:{animation_length:4,bones:{body:track}}}}}],
  };
  const model=await context.SPMPreview.buildModel(payload),bone=model.getObjectByName('body');
  context.SPMPreview.applyPreviewPose(model,payload,1);
  assert.ok(Math.abs(bone.rotation.y-Math.PI/4)<1e-9);
  assert.equal(bone.position.y,4);
  assert.equal(bone.scale.y,1.5);
});

test('YSM card coordinates follow the mods mirrored X and rotation conventions',async()=>{
  const payload={documents:[{path:'main.json',value:{bones:[{name:'body',pivot:[2,0,0],cubes:[{origin:[0,0,0],size:[1,1,1]}]}]}}],images:[],pack:{'ysm.json':{properties:{preview_animation:'gui'}}},animations:[{value:{animations:{gui:{bones:{body:{position:[5,0,0],rotation:[30,40,50]}}}}}}]};
  const model=await context.SPMPreview.buildModel(payload),bone=model.getObjectByName('body');
  context.SPMPreview.applyPreviewPose(model,payload);
  model.updateMatrixWorld(true);
  assert.equal(bone.getWorldPosition(bone.position.clone()).x,-7);
  assert.ok(Math.abs(bone.rotation.x+Math.PI/6)<1e-9);
  assert.ok(Math.abs(bone.rotation.z+50*Math.PI/180)<1e-9);
});
