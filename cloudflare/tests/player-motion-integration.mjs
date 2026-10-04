import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { build } from 'esbuild';
import { Miniflare } from 'miniflare';

const bundle = await build({entryPoints:['src/index.ts'],bundle:true,format:'esm',target:'es2022',write:false,external:['cloudflare:workers']});
const mf = new Miniflare({workers:[{config:{name:'motion-test',compatibilityDate:'2026-09-22',manifest:{mainModule:'index.js',modulesRoot:process.cwd(),modules:{'index.js':{type:'esm',contents:bundle.outputFiles[0].text}}},env:{SPM_CLOUD_ACCESS_TOKEN:{type:'text',value:'motion-secret'},SPM_CLOUD_INSTANCE_ID:{type:'text',value:'custom-instance'},SPM_CLOUD_ORIGIN:{type:'text',value:'https://custom.cloud.test'},DB:{type:'d1',name:'motion-test'},ASSETS:{type:'r2',name:'motion-assets'}}}}]});
const uuidA='aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa', uuidB='bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb';
const sha=createHash('sha256').update('model').digest('hex');
const selection={asset_id:'test-model',asset_revision:1,raw_sha256:sha,texture_id:'芙宁娜'};
const motion={event_id:'wheel-1',animation_key:'animation.芙宁娜.轮盘',started_at_unix_ms:123456789,roaming:{'v.轮盘':1.25},expressions:[],controllers:{}};
async function req(path,method='GET',body,token='motion-secret') {
  const response=await mf.dispatchFetch('https://custom.cloud.test'+path,{method,headers:{authorization:'Bearer '+token,'content-type':'application/json'},...(body===undefined?{}:{body:JSON.stringify(body)})});
  return {status:response.status,body:await response.json()};
}
const put=(rev,data={},id='identity_a',uuid=uuidA,token)=>req('/v1/players/me/appearance','PUT',{identity_id:id,entity_uuid:uuid,expected_revision:rev,...selection,...data},token);
const query=(ids=[uuidA],token)=>req('/v1/players/appearances/query','POST',{entity_uuids:ids},token);
try {
  const db=await mf.getD1Database('DB');
  for(const file of (await readdir('migrations')).filter(f=>f.endsWith('.sql')).sort()) for(const sql of (await readFile('migrations/'+file,'utf8')).split(';').map(s=>s.trim()).filter(Boolean)) await db.prepare(sql).run();
  await db.prepare("INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('identity_a','account_local','official',?1,'A',1),('identity_b','account_local','official',?2,'B',1)").bind(uuidA,uuidB).run();
  await db.prepare("INSERT INTO assets(asset_id,owner_account_id,current_revision,visibility) VALUES ('test-model','account_local',1,'PUBLIC')").run();
  await db.prepare("INSERT INTO asset_revisions(asset_id,revision,name,format,raw_sha256,byte_length,object_key) VALUES ('test-model',1,'model','ysm',?1,5,'model')").bind(sha).run();
  assert.equal((await put(0,{motion})).status,200);
  assert.deepEqual((await query()).body.entries[0].selection.motion,motion,'other clients must receive the selected wheel animation and Molang variables');
  const invalidMotions=[
    [],{},true,'motion',
    {...motion,event_id:''},{...motion,event_id:'x'.repeat(65)},
    {...motion,animation_key:'x'.repeat(257)},
    {...motion,started_at_unix_ms:-1},{...motion,started_at_unix_ms:1.5},{...motion,started_at_unix_ms:Number.MAX_SAFE_INTEGER+1},
    {...motion,roaming:null},{...motion,roaming:[]},{...motion,roaming:{bad:'1'}},{...motion,roaming:{bad:NaN}},
    {...motion,roaming:{['x'.repeat(33)]:1}},{...motion,roaming:Object.fromEntries(Array.from({length:65},(_,i)=>['v'+i,i]))},
    {...motion,expressions:{}},{...motion,expressions:null},
    {...motion,expressions:[{event_id:'e',started_at_unix_ms:1,expression:'x'.repeat(2049),values:[]}]},
    {...motion,expressions:[{event_id:'e',started_at_unix_ms:1,expression:'x',values:Array(17).fill(1)}]},
    {...motion,expressions:[{event_id:'e',started_at_unix_ms:1,expression:'x',values:[Infinity]}]},
    {...motion,expressions:Array(17).fill({event_id:'e',started_at_unix_ms:1,expression:'x',values:[]})},
    {...motion,controllers:null},{...motion,controllers:[]},{...motion,controllers:{idle:{state:'x'.repeat(129),started_at_unix_ms:1,variables:{}}}},
    {...motion,controllers:{['x'.repeat(129)]:{state:'idle',started_at_unix_ms:1,variables:{}}}},
    {...motion,controllers:{idle:{state:'idle',started_at_unix_ms:1,variables:{['x'.repeat(65)]:1}}}},
    {...motion,controllers:{idle:{state:'idle',started_at_unix_ms:1,variables:Object.fromEntries(Array.from({length:65},(_,i)=>['v'+i,i]))}}},
    {...motion,controllers:Object.fromEntries(Array.from({length:65},(_,i)=>['c'+i,{state:'idle',started_at_unix_ms:1,variables:{}}]))},
    {...motion,expressions:Array.from({length:16},(_,i)=>({event_id:'e'+i,started_at_unix_ms:1,expression:'芙'.repeat(2048),values:[]}))},
  ];
  for(const invalid of invalidMotions) {
    const result=await put(1,{motion:invalid});
    assert.equal(result.status,400,'malformed or excessive motion must fail: '+JSON.stringify(invalid).slice(0,120));
    assert.equal(result.body.code,'INVALID_METADATA');
  }
  assert.deepEqual((await query()).body.entries[0].selection.motion,motion,'invalid motion must not mutate current state');
  assert.ok((await req('/v1/instance')).body.capabilities?.includes('player_motion_v1'),'clients must detect animation support on every Cloud instance');
  const idle={...motion,event_id:'wheel-2',animation_key:'',started_at_unix_ms:123456790,roaming:{'v.轮盘':0,'v.随机待机':2},
    expressions:[{event_id:'expression-1',started_at_unix_ms:123456790,expression:'variable.服饰 = value.0;\nvariable.表情 = value.1;',values:[2,-0.25]}],
    controllers:{'controller.芙宁娜.待机':{state:'随机待机三',started_at_unix_ms:123456790,variables:{'variable.随机数':0.7,'variable.裙子':2}}}};
  assert.equal((await put(1,{motion:idle})).status,200);
  assert.deepEqual((await query()).body.entries[0].selection.motion,idle,'stop, expressions and automatic idle controller state must survive polling');
  assert.equal((await put(1,{motion})).status,409,'stale update cannot overwrite motion');
  assert.equal((await put(0,{motion})).status,409,'stale first write cannot overwrite motion');
  assert.deepEqual((await query()).body.entries[0].selection.motion,idle);
  const concurrent=await Promise.all([put(2,{motion:{...idle,event_id:'race-a'}}),put(2,{motion:{...idle,event_id:'race-b'}})]);
  assert.deepEqual(concurrent.map(r=>r.status).sort(),[200,409]);
  const winner=concurrent[0].status===200?'race-a':'race-b';
  assert.equal((await query()).body.entries[0].selection.motion.event_id,winner,'same-revision races preserve only the accepted motion');
  assert.equal((await put(0,{motion},'identity_b',uuidB)).status,200);
  let entries=(await query([uuidA,uuidB])).body.entries;
  assert.equal(entries[0].selection.motion.event_id,winner);
  assert.equal(entries[1].selection.motion.event_id,'wheel-1','different game identities never share motion');
  assert.equal((await put(3,{},'identity_a',uuidB)).status,403,'motion cannot capture another entity UUID');
  assert.equal((await put(3)).status,200);
  assert.equal((await query()).body.entries[0].selection.motion,null,'old clients omitting motion clear previous actions');
  assert.equal((await put(4,{motion:null})).status,200);
  assert.equal((await query()).body.entries[0].selection.motion,null,'explicit null motion is compatible');
  const minimal={event_id:'minimal',animation_key:'',started_at_unix_ms:0};
  assert.equal((await put(5,{motion:minimal})).status,200);
  assert.deepEqual((await query()).body.entries[0].selection.motion,minimal,'optional collections may be omitted');
  assert.equal((await put(6,{motion,asset_id:null})).status,200);
  assert.equal((await query()).body.entries[0].selection,null,'cleared appearance never exposes motion');
  assert.equal((await put(7,{motion:idle})).status,200);
  await req('/v1/accounts','POST',{account_id:'observer',password:'password123'});
  const token=(await req('/v1/sessions','POST',{account_id:'observer',password:'password123'})).body.access_token;
  assert.deepEqual((await query([uuidA],token)).body.entries[0].selection.motion,idle,'independent clients can see public motions');
  assert.equal((await put(8,{motion},'identity_a',uuidA,token)).status,403,'another account cannot mutate a game identity');
  await db.prepare("UPDATE assets SET visibility='PRIVATE' WHERE asset_id='test-model'").run();
  await db.prepare("INSERT INTO asset_acl(asset_id,account_id,permission) VALUES ('test-model','observer','render_read')").run();
  assert.equal((await query()).body.entries[0].selection,null,'private motions are hidden even from owner queries');
  assert.equal((await query([uuidA],token)).body.entries[0].selection,null,'download ACL cannot disclose private motions');
  await db.prepare("UPDATE assets SET visibility='PUBLIC' WHERE asset_id='test-model'").run();
  assert.deepEqual((await query([uuidA],token)).body.entries[0].selection.motion,idle);
  await db.prepare("UPDATE player_appearances SET updated_at=0 WHERE identity_id='identity_a'").run();
  assert.equal((await query([uuidA],token)).body.entries[0].selection,null,'expired/crashed clients cannot leave visible motion');
  assert.equal((await query([uuidB],token)).body.entries[0].selection.motion.event_id,'wheel-1','expiry is isolated per identity');
  await db.prepare("UPDATE identities SET verified=0 WHERE identity_id='identity_b'").run();
  assert.equal((await query([uuidB],token)).body.entries[0].selection,null,'revoked game identity cannot disclose motions');
  console.log('PASS player motion wheel/stop/Molang/expression/idle state; custom instance capability; validation; concurrent CAS; identity isolation; legacy clients; privacy and expiry');
} finally { await mf.dispose(); }
