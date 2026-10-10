import assert from 'node:assert/strict';
import {readFile,readdir} from 'node:fs/promises';
import {build} from 'esbuild';
import {Miniflare} from 'miniflare';

const fixture=JSON.parse(await readFile('../fixtures/projectile-snapshot-v1.json','utf8'));
const bundle=await build({stdin:{contents:`import {projectileRoute,visualDefaults,cleanupVisualStates} from './src/projectile-snapshots';
export default {async fetch(request,env){try {
 const limits={...visualDefaults,...JSON.parse(request.headers.get('test-limits')??'{}')};
 if(new URL(request.url).pathname.endsWith('/cleanup')) {await cleanupVisualStates(env,Number(request.headers.get('test-now')));return Response.json({ok:true});}
 return await projectileRoute(request,env,request.headers.get('test-account')??'shooter',limits,Number(request.headers.get('test-now')??'1000'))??new Response('not found',{status:404});
}catch(e){if(e instanceof Response)return e;throw e;}}};`,resolveDir:process.cwd()},bundle:true,format:'esm',target:'es2022',write:false});
const mf=new Miniflare({workers:[{config:{name:'projectile-test',compatibilityDate:'2026-09-22',manifest:{mainModule:'index.js',modulesRoot:process.cwd(),modules:{'index.js':{type:'esm',contents:bundle.outputFiles[0].text}}},env:{DB:{type:'d1',name:'projectile-test'},SPM_CLOUD_PROJECTILE_SNAPSHOTS:{type:'text',value:'true'}}}}]});
const path='/v1/scopes/test-scope/projectiles';
async function req(suffix,method,body,{account='shooter',now=1000,limits={}}={}) {
  const r=await mf.dispatchFetch('https://test.cloud'+path+suffix,{method,headers:{'content-type':'application/json','test-account':account,'test-now':String(now),'test-limits':JSON.stringify(limits)},body:typeof body==='string'?body:JSON.stringify(body)});
  return {status:r.status,body:await r.json()};
}
const query={world_epoch:fixture.world_epoch,dimension_id:fixture.dimension_id,entity_uuids:[fixture.entity_uuid]};
const renewal=revision=>({world_epoch:fixture.world_epoch,dimension_id:fixture.dimension_id,event_id:fixture.event_id,expected_revision:revision});
const leasePath='/'+fixture.entity_uuid+'/lease';
try {
  const db=await mf.getD1Database('DB');
  for(const file of (await readdir('migrations')).filter(f=>f.endsWith('.sql')).sort())for(const sql of (await readFile('migrations/'+file,'utf8')).split(';').map(s=>s.trim()).filter(Boolean))await db.prepare(sql).run();
  await db.prepare("INSERT INTO accounts(account_id) VALUES ('shooter'),('observer'),('outsider')").run();
  await db.prepare("INSERT INTO scopes(scope_id,tenant_id,name,world_epoch) VALUES ('test-scope','shooter','Test','world-reset-3')").run();
  await db.prepare("INSERT INTO scope_acl(scope_id,account_id,role) VALUES ('test-scope','shooter','viewer'),('test-scope','observer','viewer')").run();
  await db.prepare("INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('identity-shooter','shooter','official',?1,'Shooter',1)").bind(fixture.source_entity_uuid).run();
  await db.prepare("INSERT INTO assets(asset_id,owner_account_id,current_revision,visibility) VALUES ('model-arrow','shooter',1,'PUBLIC')").run();
  await db.prepare("INSERT INTO asset_revisions(asset_id,revision,name,format,raw_sha256,byte_length,object_key) VALUES ('model-arrow',1,'Arrow','ysm',?1,1,'test')").bind(fixture.resource.raw_sha256).run();
  const limits={projectile_idle_ttl_seconds:10,projectile_max_lifetime_seconds:15};
  let r=await req('','PUT',fixture,{limits});assert.equal(r.status,200);assert.deepEqual(r.body.snapshot,fixture);assert.equal(r.body.expires_at_unix_ms,11000);
  r=await req('','PUT',fixture,{limits,now:2000});assert.equal(r.status,200);assert.equal(r.body.expires_at_unix_ms,11000);
  assert.equal((await req('','PUT',{...fixture,resource:{...fixture.resource,texture_id:'changed'}},{limits,now:3000})).status,409);
  assert.equal((await req('/query','POST',query,{account:'outsider',now:2000})).status,403);
  assert.equal((await req('/query','POST',{...query,world_epoch:'wrong'},{account:'observer',now:2000})).status,403);
  assert.equal((await req('/query','POST',{...query,dimension_id:'minecraft:the_nether'},{account:'observer',now:2000})).body.entries.length,0);
  assert.equal((await req(leasePath,'DELETE',renewal(1),{account:'observer',now:3000})).status,403);
  const races=await Promise.all([req(leasePath,'PUT',renewal(1),{limits,now:8000}),req(leasePath,'PUT',renewal(1),{limits,now:8000})]);
  assert.deepEqual(races.map(r=>r.status).sort(),[200,409]);
  assert.equal(races.find(r=>r.status===200).body.expires_at_unix_ms,16000);
  assert.equal((await req('/query','POST',query,{account:'observer',now:16000})).body.entries.length,0);
  assert.equal((await req(leasePath,'PUT',renewal(2),{limits,now:16000})).status,404);
  assert.equal((await req('','PUT',fixture,{limits,now:20000})).body.absolute_expires_at_unix_ms,16000);
  for(const body of [{...fixture,unknown:1},{...fixture,variables:null},{...fixture,resource:{...fixture.resource,asset_revision:1.5}},
    {...fixture,variables:{bad:1e100}},{...fixture,variables:{['x'.repeat(33)]:1}},JSON.stringify(fixture).replace('"event_id":"shot-1"','"event_id":"shot-1","event_id":"shot-1"')]) {
    assert.equal((await req('','PUT',body,{now:21000})).status,400);
  }
  assert.equal((await req('','PUT',' '.repeat(8193)+JSON.stringify(fixture),{now:21000})).status,413);
  assert.equal((await req('/cleanup','POST',{}, {now:21000})).status,200);
  assert.equal((await db.prepare("SELECT COUNT(*) AS count FROM projectile_snapshots").first()).count,0);
  assert.equal((await db.prepare("SELECT COUNT(*) AS count FROM projectile_tombstones").first()).count,1);
  assert.equal((await req('','PUT',fixture,{now:22000})).status,404);
  assert.equal((await req('','PUT',{...fixture,entity_uuid:'44444444-4444-4444-8444-444444444444'},{now:22000})).status,409);
  const other={...fixture,entity_uuid:'33333333-3333-4333-8333-333333333333',event_id:'shot-2'};
  assert.equal((await req('','PUT',other,{now:30000,limits:{visual_publish_burst:1,visual_publish_requests_per_second:1}})).status,200);
  assert.equal((await req('','PUT',other,{now:30001,limits:{visual_publish_burst:1,visual_publish_requests_per_second:1}})).status,429);
  assert.equal((await req('/query','POST',{...query,entity_uuids:[other.entity_uuid]},{account:'observer',now:30002})).body.entries.length,1);
  await db.prepare("UPDATE assets SET current_revision=2").run();
  assert.equal((await req('/query','POST',{...query,entity_uuids:[other.entity_uuid]},{account:'observer',now:30002})).body.entries.length,1);
  await db.prepare("UPDATE assets SET visibility='PRIVATE'").run();
  await db.prepare("INSERT INTO asset_acl(asset_id,account_id,permission) VALUES ('model-arrow','shooter','manage')").run();
  assert.equal((await req('/query','POST',{...query,entity_uuids:[other.entity_uuid]},{account:'observer',now:30002})).body.entries.length,0);
  await db.prepare("INSERT INTO asset_acl(asset_id,account_id,permission) VALUES ('model-arrow','observer','render_read')").run();
  assert.equal((await req('/query','POST',{...query,entity_uuids:[other.entity_uuid]},{account:'observer',now:30002})).body.entries.length,1);
  await db.prepare("INSERT INTO player_appearances(identity_id,entity_uuid,revision) VALUES ('identity-shooter',?1,3)").bind(fixture.source_entity_uuid).run();
  assert.equal((await req('/query','POST',{...query,entity_uuids:[other.entity_uuid]},{account:'observer',now:30002})).body.entries.length,0);
  await db.prepare("UPDATE scopes SET world_epoch='reset-4'").run();
  await req('/cleanup','POST',{}, {now:30003});
  assert.equal((await db.prepare("SELECT COUNT(*) AS count FROM projectile_tombstones").first()).count,0);
  assert.equal((await req('','PUT',fixture,{now:30004})).status,403);
  console.log('PASS projectile snapshot fixture, immutable replay, CAS race, expiry, privacy, private ACL, bounded JSON, persisted rate limit and compact replay barriers');
}finally{await mf.dispose();}
