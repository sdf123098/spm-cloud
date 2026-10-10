import assert from 'node:assert/strict';
import {readFile,readdir} from 'node:fs/promises';
import {build} from 'esbuild';
import {Miniflare} from 'miniflare';
const fixture=JSON.parse(await readFile('../fixtures/vehicle-appearance-v1.json','utf8'));
const id='11111111-1111-4111-8111-111111111111';
const bundle=await build({stdin:{contents:`import {vehicleRoute} from './src/vehicle-appearances';export default {async fetch(request,env){try{return await vehicleRoute(request,env,request.headers.get('test-account')??'owner')??new Response('missing',{status:404});}catch(e){if(e instanceof Response)return e;throw e;}}};`,resolveDir:process.cwd()},bundle:true,format:'esm',target:'es2022',write:false});
const mf=new Miniflare({workers:[{config:{name:'vehicle-test',compatibilityDate:'2026-09-22',manifest:{mainModule:'index.js',modulesRoot:process.cwd(),modules:{'index.js':{type:'esm',contents:bundle.outputFiles[0].text}}},env:{DB:{type:'d1',name:'vehicle-test'},SPM_CLOUD_VEHICLE_BINDINGS:{type:'text',value:'true'}}}}]});
async function req(endpoint,method,body,account='owner'){
  const response=await mf.dispatchFetch('https://test.cloud/v1/scopes/test-scope/vehicles/'+endpoint,{method,headers:{'content-type':'application/json','test-account':account},body:JSON.stringify(body)});
  return {status:response.status,body:await response.json()};
}
const query={world_epoch:fixture.world_epoch,dimension_id:fixture.dimension_id,entity_uuids:[id]};
try{
  const db=await mf.getD1Database('DB');
  for(const file of (await readdir('migrations')).filter(f=>f.endsWith('.sql')).sort())for(const sql of (await readFile('migrations/'+file,'utf8')).split(';').map(s=>s.trim()).filter(Boolean))await db.prepare(sql).run();
  await db.prepare("INSERT INTO accounts(account_id) VALUES ('owner'),('observer'),('outsider')").run();
  await db.prepare("INSERT INTO scopes(scope_id,tenant_id,name,world_epoch) VALUES ('test-scope','owner','Test','world-reset-3')").run();
  await db.prepare("INSERT INTO scope_acl(scope_id,account_id,role) VALUES ('test-scope','owner','manage'),('test-scope','observer','viewer')").run();
  await db.prepare("INSERT INTO targets(target_id,scope_id,target_kind,display_name,owner_account_id) VALUES ('target-horse','test-scope','VEHICLE','Horse','owner')").run();
  await db.prepare("INSERT INTO target_acl(target_id,account_id,role) VALUES ('target-horse','owner','manage')").run();
  await db.prepare("INSERT INTO assets(asset_id,owner_account_id,current_revision,visibility) VALUES ('model-arrow','owner',1,'PUBLIC')").run();
  await db.prepare("INSERT INTO asset_revisions(asset_id,revision,name,format,raw_sha256,byte_length,object_key) VALUES ('model-arrow',1,'Horse','ysm',?1,1,'test')").bind(fixture.resource.raw_sha256).run();
  assert.equal((await req(id,'PUT',fixture,'observer')).status,403);
  let result=await req(id,'PUT',fixture);assert.equal(result.status,200);assert.deepEqual(result.body.binding,fixture);assert.equal(result.body.revision,1);
  assert.equal((await req(id,'PUT',fixture)).status,409);
  assert.equal((await req('query','POST',query,'observer')).body.entries.length,1);
  assert.equal((await req('query','POST',query,'outsider')).status,403);
  assert.equal((await req('query','POST',{...query,world_epoch:'old'},'observer')).status,403);
  assert.equal((await req('query','POST',{...query,dimension_id:'minecraft:the_nether'},'observer')).body.entries.length,0);
  assert.equal((await req(id,'PUT',{...fixture,entity_kind:'minecraft:pig',expected_revision:1})).status,409);
  const next={...fixture,expected_revision:1};
  const race=await Promise.all([req(id,'PUT',next),req(id,'PUT',next)]);assert.deepEqual(race.map(r=>r.status).sort(),[200,409]);
  await db.prepare("UPDATE assets SET visibility='PRIVATE',current_revision=2").run();
  await db.prepare("INSERT INTO asset_acl(asset_id,account_id,permission) VALUES ('model-arrow','owner','manage')").run();
  assert.equal((await req('query','POST',query,'observer')).body.entries.length,0);
  await db.prepare("INSERT INTO asset_acl(asset_id,account_id,permission) VALUES ('model-arrow','observer','render_read')").run();
  assert.equal((await req('query','POST',query,'observer')).body.entries[0].binding.resource.asset_revision,1);
  assert.equal((await req(id,'PUT',{...fixture,expected_revision:2,resource:null,variables:{}})).status,200);
  assert.equal((await req('query','POST',query,'observer')).body.entries[0].binding.resource,null);
  await db.prepare("DELETE FROM target_acl WHERE account_id='owner'").run();
  assert.equal((await req('query','POST',query,'observer')).body.entries.length,0);
  assert.equal((await req(id,'PUT',{...fixture,expected_revision:3})).status,403);
  assert.equal((await req(id,'PUT',{...fixture,variables:{bad:1e100}})).status,400);
  console.log('PASS shared vehicle fixture, explicit edit ACL, atomic CAS race, exact old revision/PRIVATE ACL, epoch/dimension, unbind and target revocation');
}finally{await mf.dispose();}
