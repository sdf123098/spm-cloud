import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { build } from 'esbuild';
import { Miniflare } from 'miniflare';

const fixture=JSON.parse(await readFile('../fixtures/player-display-state-v1.json','utf8'));
const bundle=await build({entryPoints:['src/index.ts'],bundle:true,format:'esm',target:'es2022',write:false,external:['cloudflare:workers']});
const mf=new Miniflare({workers:[{config:{name:'display-test',compatibilityDate:'2026-09-22',manifest:{mainModule:'index.js',modulesRoot:process.cwd(),modules:{'index.js':{type:'esm',contents:bundle.outputFiles[0].text}}},env:{SPM_CLOUD_ACCESS_TOKEN:{type:'text',value:'test-secret'},SPM_CLOUD_PLAYER_DISPLAY_STATE:{type:'text',value:'true'},SPM_CLOUD_ENTITY_MOTION:{type:'text',value:'true'},SPM_CLOUD_INSTANCE_ID:{type:'text',value:'test'},SPM_CLOUD_ORIGIN:{type:'text',value:'https://cloud.test'},DB:{type:'d1',name:'display-test'},ASSETS:{type:'r2',name:'display-assets'}}}}]});
const uuid='22222222-2222-4222-8222-222222222222';
const selection={asset_id:'display-model',asset_revision:1,raw_sha256:'a'.repeat(64),texture_id:'default'};
async function request(path,method='GET',value,token='test-secret') {
  const response=await mf.dispatchFetch('https://cloud.test'+path,{method,headers:{authorization:'Bearer '+token,'content-type':'application/json'},...(value===undefined?{}:{body:typeof value==='string'?value:JSON.stringify(value)})});
  return {status:response.status,body:await response.json()};
}
const put=(revision,display=fixture,extra={},token)=>request('/v1/players/me/appearance','PUT',{identity_id:'display-self',entity_uuid:uuid,expected_revision:revision,...selection,...(display===undefined?{}:{display_state:display}),...extra},token);
const query=token=>request('/v1/players/appearances/query','POST',{entity_uuids:[uuid]},token);
try {
  assert.ok((await request('/v1/instance')).body.capabilities.includes('entity_motion_v1'));

  const db=await mf.getD1Database('DB');
  for(const file of (await readdir('migrations')).filter(f=>f.endsWith('.sql')).sort())
    for(const sql of (await readFile('migrations/'+file,'utf8')).split(';').map(s=>s.trim()).filter(Boolean)) await db.prepare(sql).run();
  await db.prepare("INSERT INTO scopes(scope_id,tenant_id,name,world_epoch) VALUES ('test-scope','account_local','Test','world-reset-3')").run();
  await db.prepare("INSERT INTO scope_acl(scope_id,account_id,role) VALUES ('test-scope','account_local','viewer')").run();
  await db.prepare("INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('display-self','account_local','official',?1,'Self',1)").bind(uuid).run();
  await db.prepare("INSERT INTO assets(asset_id,owner_account_id,current_revision,visibility) VALUES ('display-model','account_local',1,'PUBLIC')").run();
  await db.prepare("INSERT INTO asset_revisions(asset_id,revision,name,format,raw_sha256,byte_length,object_key) VALUES ('display-model',1,'Model','ysm',?1,1,'test')").bind(selection.raw_sha256).run();
  assert.equal((await put(0)).status,200);
  let state=(await query()).body.entries[0].selection.display_state;
  assert.deepEqual(state.state,fixture.state);
  assert.ok(state.expires_at_unix_ms-state.server_time_unix_ms<=60000);
  await request('/v1/accounts','POST',{account_id:'observer',password:'password123'});
  const observer=(await request('/v1/sessions','POST',{account_id:'observer',password:'password123'})).body.access_token;
  assert.equal((await query(observer)).body.entries[0].selection.display_state,null,'public appearance does not grant hidden state scope access');
  await db.prepare("INSERT INTO scope_acl(scope_id,account_id,role) VALUES ('test-scope','observer','viewer')").run();
  assert.deepEqual((await query(observer)).body.entries[0].selection.display_state.state,fixture.state);
  assert.equal((await put(1,fixture,{},observer)).status,403,'only verified identity owner publishes');
  assert.equal((await put(1,fixture,{entity_uuid:'33333333-3333-4333-8333-333333333333'})).status,403);
  for(const state of [[],{food_level:21},{experience_level:'42'},{health:-1},{health:1e30},{strafe_input:1.5},{flying:1},{effect_amplifiers:{'minecraft:speed':0}},{unknown:1}]) {
    const result=await put(1,{...fixture,state});assert.equal(result.status,400,JSON.stringify(state));assert.equal(result.body.code,'INVALID_METADATA');
  }
  assert.equal((await put(1,{...fixture,world_epoch:'old-world'})).status,403);
  const duplicate=JSON.stringify({identity_id:'display-self',entity_uuid:uuid,expected_revision:1,...selection,display_state:fixture}).replace('"experience_level":42','"experience_level":42,"experience_level":3');
  assert.equal((await request('/v1/players/me/appearance','PUT',duplicate)).status,400);
  const race=await Promise.all([put(1,{...fixture,state:{experience_level:10}}),put(1,{...fixture,state:{experience_level:20}})]);
  assert.deepEqual(race.map(r=>r.status).sort(),[200,409]);
  assert.equal((await query()).body.entries[0].selection.display_state.state.experience_level,race[0].status===200?10:20);
  assert.equal((await put(2,null)).status,200);
  assert.equal((await query()).body.entries[0].selection.display_state,null,'null clears the previous display');
  assert.equal((await put(3)).status,200);
  assert.equal((await put(4,null,{asset_id:null})).status,200);
  assert.equal((await query()).body.entries[0].selection,null,'privacy clears display and appearance atomically');
  assert.equal((await put(5)).status,200);
  await db.prepare("UPDATE scopes SET world_epoch='new-world' WHERE scope_id='test-scope'").run();
  assert.equal((await query()).body.entries[0].selection.display_state,null,'epoch rotation hides old hidden values');
  await db.prepare("UPDATE scopes SET world_epoch='world-reset-3' WHERE scope_id='test-scope'").run();
  await db.prepare("DELETE FROM scope_acl WHERE scope_id='test-scope' AND account_id='account_local'").run();
  assert.equal((await query(observer)).body.entries[0].selection.display_state,null,'publisher scope revocation is checked during reads');
  await db.prepare("UPDATE player_appearances SET updated_at=0 WHERE identity_id='display-self'").run();
  assert.equal((await query()).body.entries[0].selection,null,'expired publication cannot disclose display');
  console.log('PASS shared display fixture, self identity, scope/epoch isolation, strict fields and duplicate keys, atomic CAS races, privacy and expiry');
} finally { await mf.dispose(); }
