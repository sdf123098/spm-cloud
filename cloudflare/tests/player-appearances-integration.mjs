import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import { build } from 'esbuild';
import { Miniflare } from 'miniflare';

const profileRoot=generateKeyPairSync('rsa',{modulusLength:2048});
const publicProfileRoot=profileRoot.publicKey.export({type:'spki',format:'der'}).toString('base64');
const bundle = await build({entryPoints:['src/index.ts'], bundle:true, format:'esm', target:'es2022', write:false, external:['cloudflare:workers']});
const mf = new Miniflare({workers:[{config:{name:'players-test', compatibilityDate:'2026-09-22', manifest:{mainModule:'index.js', modulesRoot:process.cwd(), modules:{'index.js':{type:'esm',contents:bundle.outputFiles[0].text}}}, env:{SPM_CLOUD_ACCESS_TOKEN:{type:'text',value:'isolated-players-secret'}, SPM_CLOUD_INSTANCE_ID:{type:'text',value:'official'}, SPM_CLOUD_ORIGIN:{type:'text',value:'https://cloud.test'}, DB:{type:'d1',name:'players-test'}, ASSETS:{type:'r2',name:'players-assets'}}},dev:{outboundService:{type:'fetcher',handler:async request=>{assert.equal(new URL(request.url).pathname,'/publickeys');return Response.json({playerCertificateKeys:[{publicKey:publicProfileRoot}],profilePropertyKeys:[{publicKey:publicProfileRoot}]});}}}}]});
const sha = createHash('sha256').update('model').digest('hex');
const uuidA = 'aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa', uuidB = 'bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb';
const offline = name => { const bytes=createHash('md5').update('OfflinePlayer:'+name).digest(); bytes[6]=(bytes[6]&15)|48; bytes[8]=(bytes[8]&63)|128; const s=bytes.toString('hex'); return `${s.slice(0,8)}-${s.slice(8,12)}-${s.slice(12,16)}-${s.slice(16,20)}-${s.slice(20)}`; };
async function req(path, method='GET', body, token='isolated-players-secret') {
  const r=await mf.dispatchFetch('https://cloud.test'+path,{method,headers:{authorization:'Bearer '+token,'content-type':'application/json'},...(body===undefined?{}:{body:JSON.stringify(body)})});
  return {status:r.status,body:await r.json()};
}
const selection={asset_id:'test-model',asset_revision:1,raw_sha256:sha,texture_id:'贴图二'};
const put=(id,uuid,rev,sel=selection)=>req('/v1/players/me/appearance','PUT',{identity_id:id,entity_uuid:uuid,expected_revision:rev,...sel});
const query=(...ids)=>req('/v1/players/appearances/query','POST',{entity_uuids:ids});
try {
  const db=await mf.getD1Database('DB');
  for(const f of (await readdir('migrations')).filter(f=>f.endsWith('.sql')).sort()) for(const sql of (await readFile('migrations/'+f,'utf8')).split(';').map(s=>s.trim()).filter(Boolean)) await db.prepare(sql).run();
  assert.equal((await req("/v1/players/me/appearance")).status,400,"invalid player revision query returns structured validation error");
  await db.prepare("INSERT INTO identity_providers(provider_id,display_name,base_url,session_path,enabled) VALUES ('test','test','https://provider.test','/session/minecraft/hasJoined',1)").run();
  await db.prepare("INSERT INTO identities(identity_id,account_id,identity_kind,provider_id,profile_uuid,display_name,canonical_name,verified) VALUES ('identity_a','account_local','official',NULL,?1,'UntrustedAlias',NULL,1), ('identity_b','account_local','yggdrasil','test',?2,'Micaftic','Micaftic',1), ('identity_offline','account_local','offline',NULL,?3,'offline',NULL,0)").bind(uuidA,uuidB,offline('offline')).run();
  await db.prepare("INSERT INTO assets(asset_id,owner_account_id,current_revision,visibility) VALUES ('test-model','account_local',1,'PUBLIC')").run();
  await db.prepare("INSERT INTO asset_revisions(asset_id,revision,name,format,raw_sha256,byte_length,object_key) VALUES ('test-model',1,'model','ysm',?1,5,'model')").bind(sha).run();
  assert.equal((await put('identity_a',uuidA,0)).status,200);
  assert.equal((await put('identity_b',offline('Micaftic'),0,{...selection,texture_id:'符玄'})).status,200);
  let entries=(await query(uuidA,offline('Micaftic'))).body.entries;
  assert.equal(entries[0].selection.texture_id,'贴图二'); assert.equal(entries[1].selection.texture_id,'符玄');
  assert.equal((await put('identity_a',offline('UntrustedAlias'),1)).status,403,'certificate-only claimed name cannot capture offline UUID');
  assert.equal((await put('identity_a',uuidB,1)).status,403,'arbitrary UUID cannot be claimed');
  assert.equal((await put('identity_offline',offline('offline'),0)).status,403);
  assert.equal((await put('identity_b',offline('Micaftic'),0)).status,409,'stale creation cannot overwrite');
  assert.equal((await put('identity_a',uuidA,1,{...selection,texture_id:'new'})).status,200);
  assert.equal((await put('identity_a',uuidA,1,{...selection,texture_id:'old'})).status,409);
  assert.equal((await query(uuidA)).body.entries[0].selection.texture_id,'new');
  assert.equal((await put('identity_a',uuidA,2,{...selection,raw_sha256:'0'.repeat(64)})).status,403);
  assert.equal((await put('identity_a',uuidA,2,{asset_id:null})).status,200);
  assert.equal((await query(uuidA)).body.entries[0].selection,null);
  assert.equal((await query('cccccccc-cccc-4ccc-cccc-cccccccccccc')).body.entries[0].selection,null);
  assert.equal((await req('/v1/players/appearances/query','POST',{entity_uuids:Array(65).fill(uuidA)})).status,400);
  // Independent observer account; PRIVATE must be hidden even if granted download ACL.
  await req('/v1/accounts','POST',{account_id:'observer',password:'password123'});
  const login=await req('/v1/sessions','POST',{account_id:'observer',password:'password123'});
  const token=login.body.access_token;
  await db.prepare("UPDATE assets SET visibility='PRIVATE' WHERE asset_id='test-model'").run();
  assert.equal((await query(offline('Micaftic'))).body.entries[0].selection,null,'private appearance stays local even across clients sharing the owner account');
  await db.prepare("INSERT INTO asset_acl(asset_id,account_id,permission) VALUES ('test-model','observer','render_read')").run();
  assert.equal((await req('/v1/players/appearances/query','POST',{entity_uuids:[offline('Micaftic')]},token)).body.entries[0].selection,null);
  await db.prepare("UPDATE assets SET visibility='PUBLIC' WHERE asset_id='test-model'").run();
  assert.equal((await req('/v1/players/appearances/query','POST',{entity_uuids:[offline('Micaftic')]},token)).body.entries[0].selection.texture_id,'符玄');
  const signedProfile=(uuid=uuidA,name='OfficialA',timestamp=Date.now(),root=profileRoot)=>{
    const value=Buffer.from(JSON.stringify({profileId:uuid.replaceAll('-',''),profileName:name,timestamp,signatureRequired:true,textures:{}})).toString('base64');
    return {value,signature:sign('RSA-SHA1',Buffer.from(value),root.privateKey).toString('base64')};
  };
  assert.equal((await put('identity_a',offline('OfficialA'),3,{...selection,profile_name_proof:signedProfile()})).status,200,'signed Mojang profile name supports official offline UUID without Worker SESSION_JOIN');
  assert.equal((await query(offline('OfficialA'))).body.entries[0].selection.texture_id,'贴图二');
  for(const proof of [signedProfile(uuidB),signedProfile(uuidA,'OfficialA',Date.now()-360000),signedProfile(uuidA,'OfficialA',Date.now()+120000),signedProfile(uuidA,'OfficialA',Date.now(),generateKeyPairSync('rsa',{modulusLength:2048})),{...signedProfile(),value:Buffer.from('{}').toString('base64')}]) {
    assert.equal((await put('identity_a',offline('OfficialA'),4,{...selection,profile_name_proof:proof})).status,403,'wrong UUID, stale/future metadata and forged signatures must fail');
  }
  await db.prepare('UPDATE player_appearances SET updated_at=0').run();
  assert.equal((await query(offline('Micaftic'))).body.entries[0].selection,null,'expired/crashed clients revert to vanilla');
  // Long cursors must progress and public transition must appear to another account immediately.
  const long='a'.repeat(90);
  const ids=[' a-leading',long+'1',long+'2','z-last ','z-last2'];
  for(const id of ids) {
    await db.prepare("INSERT INTO assets(asset_id,owner_account_id,current_revision,visibility) VALUES (?1,'account_local',1,'PRIVATE')").bind(id).run();
    await db.prepare("INSERT INTO asset_revisions(asset_id,revision,name,format,raw_sha256,byte_length,object_key) VALUES (?1,1,'searchable','ysm',?2,5,'model')").bind(id,sha).run();
    assert.equal((await req('/v1/assets/'+id+'/visibility','PUT',{visibility:'PUBLIC'})).status,200);
  }
  const seen=[]; let after='';
  for(let i=0;i<7;i++) { const page=await req('/v1/assets?scope=public&q=searchable&limit=1&after='+encodeURIComponent(after),'GET',undefined,token); seen.push(...page.body.entries.map(e=>e.asset_id)); if(!page.body.has_more) break; after=page.body.next_cursor; }
  assert.deepEqual(seen,ids,'public long-ID pages must never loop');
  if(process.env.SPM_CLOUD_REAL_PROFILE==='1') {
    const id='61699b2e-d327-4a01-9f1e-0ea8c3f06bc6';
    const upstream=await fetch('https://sessionserver.mojang.com/session/minecraft/profile/'+id.replaceAll('-','')+'?unsigned=false');
    assert.equal(upstream.status,200);const profile=await upstream.json();const property=profile.properties.find(p=>p.name==='textures'&&p.signature);
    assert.ok(property);await db.prepare("INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('real_signature_fixture','account_local','official',?1,'not trusted',1)").bind(id).run();
    assert.equal((await put('real_signature_fixture',offline(profile.name),0,{...selection,profile_name_proof:{value:property.value,signature:property.signature}})).status,200,'real Mojang signature must verify against deployed profile property trust roots');
    console.log('PASS real public Mojang signed profile name and Java offline UUID (isolated local fixture only)');
  }
  console.log('PASS player identities, offline aliases, independent models/textures, CAS, privacy, expiry, and public search pagination');
} finally { await mf.dispose(); }
