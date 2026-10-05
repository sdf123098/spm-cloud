import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { build } from 'esbuild';
import { Miniflare } from 'miniflare';

// Exercise the same protocol against the official ID and an unrelated self-hosted ID.
// Only hasJoined network responses are fixtures; routing, sessions and D1 are real.
const bundle = await build({entryPoints:['src/index.ts'],bundle:true,format:'esm',target:'es2022',write:false,external:['cloudflare:workers']});
for (const instanceId of ['official','server-owner-test']) {
  const origin = instanceId === 'official' ? 'https://micafic.xyz' : 'https://owner-cloud.example';
  const joined = new Map();
  const mf = new Miniflare({workers:[{
    config:{name:'login-parity',compatibilityDate:'2026-09-22',manifest:{mainModule:'index.js',modulesRoot:process.cwd(),modules:{'index.js':{type:'esm',contents:bundle.outputFiles[0].text}}},env:{
      SPM_CLOUD_ACCESS_TOKEN:{type:'text',value:'operator-secret'},
      SPM_CLOUD_INSTANCE_ID:{type:'text',value:instanceId},
      SPM_CLOUD_ORIGIN:{type:'text',value:origin},
      DB:{type:'d1',name:'login-parity'},
    }},
    dev:{outboundService:{type:'fetcher',handler:async request=>{
      const url = new URL(request.url);
      assert.ok(['sessionserver.mojang.com','littleskin.cn'].includes(url.hostname));
      const profile = joined.get(url.searchParams.get('serverId'));
      return profile ? Response.json(profile) : new Response(null,{status:204});
    }}},
  }]});
  async function call(path,method='GET',body,token,requestOrigin=origin) {
    const response=await mf.dispatchFetch(requestOrigin+path,{method,headers:{'content-type':'application/json',...(token?{authorization:'Bearer '+token}:{})},...(body===undefined?{}:{body:JSON.stringify(body)})});
    const payload=response.status===204?null:await response.text();
    let parsed;
    try { parsed=payload===null?null:JSON.parse(payload); }
    catch { throw new Error(method+' '+path+' returned '+response.status+' non-JSON: '+payload); }
    return {status:response.status,body:parsed};
  }
  async function register(accountId) {
    assert.equal((await call('/v1/accounts','POST',{account_id:accountId,password:'test-password-123'})).status,201);
    const session=await call('/v1/sessions','POST',{account_id:accountId,password:'test-password-123'});
    assert.equal(session.status,200);
    return session.body.access_token;
  }
  async function challenge(profile,purpose='link',token) {
    const path=purpose==='link'?'/v1/auth/challenges':'/v1/auth/login-challenges';
    const result=await call(path,'POST',profile,token);
    assert.equal(result.status,200,JSON.stringify(result.body));
    joined.set(result.body.server_id,{id:profile.profile_uuid.replaceAll('-',''),name:profile.username});
    return {path:path+'/'+result.body.challenge_id+'/complete',body:result.body};
  }
  async function complete(c,token) { return call(c.path,'POST',{challenge_id:c.body.challenge_id},token); }
  try {
    const info=await call('/v1/instance');
    assert.equal(info.status,200);
    assert.equal(info.body.instance_id,instanceId);
    assert.ok(info.body.capabilities.includes('player_motion_v1'));
    assert.ok(info.body.capabilities.includes('game_identity_auth_v1'),'all instances advertise game identity login/link capability');
    assert.deepEqual(info.body.auth,{password_login:true,game_identity_login:true,game_identity_link:true,self_registration:true});
    assert.equal((await call('/v1/instance','GET',undefined,undefined,'https://untrusted.example')).body.origin,origin,'request Host cannot change the configured trust origin');
    const db=await mf.getD1Database('DB');
    for (const name of (await readdir('migrations')).filter(n=>n.endsWith('.sql')).sort()) {
      for (const sql of (await readFile('migrations/'+name,'utf8')).split(';').map(s=>s.trim()).filter(Boolean)) await db.prepare(sql).run();
    }
    const providers=await call('/v1/identity-providers');
    assert.equal(providers.status,200);
    assert.ok(providers.body.some(p=>p.provider_id==='official'&&p.enabled));
    const immutable=await call('/v1/identity-providers','POST',{provider_id:'official',display_name:'Override',base_url:'https://attacker.example',enabled:true},'operator-secret');
    assert.equal(immutable.status,403);
    assert.equal(immutable.body.code,'ACCESS_DENIED','official Minecraft provider trust cannot be reconfigured');
    const token=await register('test');
    const duplicate=await call('/v1/accounts','POST',{account_id:'test',password:'test-password-123'});
    assert.equal(duplicate.status,409);
    assert.equal(duplicate.body.code,'ACCOUNT_EXISTS');
    const wrongPassword=await call('/v1/sessions','POST',{account_id:'test',password:'wrong-password'});
    assert.equal(wrongPassword.status,401);
    assert.equal(wrongPassword.body.code,'UNAUTHENTICATED');
    assert.deepEqual((await call('/v1/identities','GET',undefined,token)).body,[],'password login alone must not claim a Minecraft identity');
    assert.equal((await call('/v1/identities','POST',{identity:'official:aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa',display_name:'Player'},token)).status,400,'verified identities require a game proof');
    const offlineDenied=await call('/v1/identities','POST',{identity:'offline:missing-scope:aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa',display_name:'Player'},token);
    assert.equal(offlineDenied.status,403,'offline identity registration requires scope editor permission');
    assert.equal(offlineDenied.body.code,'SCOPE_ACCESS_DENIED');
    for (const [provider_id,profile_uuid] of [['official','aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa'],['littleskin','bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb']]) {
      const profile={provider_id,profile_uuid,username:provider_id==='official'?'Player':'SkinPlayer'};
      const noLink=await complete(await challenge(profile,'login'));
      assert.equal(noLink.status,404);
      assert.equal(noLink.body.code,'IDENTITY_NOT_LINKED');
      const c=await challenge(profile,'link',token);
      if (provider_id==='official') assert.equal(c.body.profile_key_payload.split('\n')[1],origin,'official Minecraft provider works on a custom Cloud while proof remains origin-bound');
      assert.equal((await call(c.path,'POST',{},token)).status,400,'completion must carry matching challenge_id');
      assert.equal((await call(c.path,'POST',{challenge_id:'wrong'},token)).status,400);
      assert.equal((await call(c.path,'POST',{challenge_id:c.body.challenge_id})).status,401,'link completion requires the authenticated account');
      const linked=await complete(c,token);
      assert.equal(linked.status,200,JSON.stringify(linked.body));
      assert.equal(linked.body.account_id,'test');
      assert.equal(linked.body.verification_status,'VERIFIED');
      assert.equal((await complete(c,token)).status,401,'proof cannot be replayed');
      const relink=await complete(await challenge(profile,'link',token),token);
      assert.equal(relink.body.identity_id,linked.body.identity_id,'binding the same account is idempotent');
      const login=await complete(await challenge(profile,'login'));
      assert.equal(login.status,200);
      assert.equal(login.body.account_id,'test');
      const identities=await call('/v1/identities','GET',undefined,login.body.access_token);
      assert.ok(identities.body.some(i=>i.identity_id===linked.body.identity_id&&i.verification_status==='VERIFIED'));
    }
    const other=await register('other');
    const collision=await complete(await challenge({provider_id:'official',profile_uuid:'aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa',username:'Player'},'link',other),other);
    assert.equal(collision.status,409);
    assert.equal(collision.body.code,'IDENTITY_ALREADY_LINKED');
    assert.deepEqual((await call('/v1/identities','GET',undefined,other)).body,[],'another account cannot capture the game identity');
    const session=(await call('/v1/sessions','POST',{account_id:'test',password:'test-password-123'})).body;
    const refreshed=await call('/v1/sessions/refresh','POST',{refresh_token:session.refresh_token});
    assert.equal(refreshed.status,200);
    assert.notEqual(refreshed.body.access_token,session.access_token);
    const reused=await call('/v1/sessions/refresh','POST',{refresh_token:session.refresh_token});
    assert.equal(reused.status,401,'old refresh token must be atomically revoked by rotation');
    assert.equal(reused.body.code,'REFRESH_REUSED');
    const oldAccess=await call('/v1/identities','GET',undefined,session.access_token);
    assert.equal(oldAccess.status,401,'rotation revokes the old access token');
    assert.equal(oldAccess.body.code,'SESSION_EXPIRED','revoked access code matches Rust');
    assert.equal((await call('/v1/identities','GET',undefined,refreshed.body.access_token)).body.length,2,'rotation stays in the original account');
    const concurrent=await Promise.all([0,1].map(()=>call('/v1/sessions/refresh','POST',{refresh_token:refreshed.body.refresh_token})));
    assert.deepEqual(concurrent.map(r=>r.status).sort(),[200,401],'concurrent refreshes have exactly one winner');
    assert.equal(concurrent.find(r=>r.status===401).body.code,'REFRESH_REUSED');
    const winning=concurrent.find(r=>r.status===200).body;
    assert.equal((await call('/v1/identities','GET',undefined,winning.access_token)).body.length,2);
    assert.equal((await call('/v1/sessions/current','DELETE',undefined,winning.access_token)).status,204);
    const loggedOutAccess=await call('/v1/identities','GET',undefined,winning.access_token);
    assert.equal(loggedOutAccess.status,401,'logout revokes the current access token');
    assert.equal(loggedOutAccess.body.code,'SESSION_EXPIRED');
    const loggedOut=await call('/v1/sessions/refresh','POST',{refresh_token:winning.refresh_token});
    assert.equal(loggedOut.status,401);
    assert.equal(loggedOut.body.code,'REFRESH_REUSED','logout also revokes the refresh token');
    assert.deepEqual((await call('/v1/identities','GET',undefined,other)).body,[],'rotation/logout do not affect other accounts');
    const expired=(await call('/v1/sessions','POST',{account_id:'other',password:'test-password-123'})).body;
    await db.prepare("UPDATE sessions SET refresh_expires_at=0 WHERE account_id='other'").run();
    const expiredRefresh=await call('/v1/sessions/refresh','POST',{refresh_token:expired.refresh_token});
    assert.equal(expiredRefresh.status,401);
    assert.equal(expiredRefresh.body.code,'SESSION_EXPIRED');
    const unknown=await call('/v1/sessions/refresh','POST',{refresh_token:'unknown-refresh'});
    assert.equal(unknown.status,401);
    assert.equal(unknown.body.code,'REFRESH_REUSED');
    for (const refresh_token of ['', 'x'.repeat(513)]) {
      const invalid=await call('/v1/sessions/refresh','POST',{refresh_token});
      assert.equal(invalid.status,401);
      assert.equal(invalid.body.code,'REFRESH_REUSED');
    }
    const rollback=(await call('/v1/sessions','POST',{account_id:'test',password:'test-password-123'})).body;
    await db.prepare("CREATE TRIGGER fail_session_fixture BEFORE INSERT ON sessions WHEN NEW.account_id='test' BEGIN SELECT RAISE(ABORT, 'fixture replacement insertion failure'); END").run();
    assert.equal((await call('/v1/sessions/refresh','POST',{refresh_token:rollback.refresh_token})).status,500,'failed replacement insertion is reported');
    await db.prepare('DROP TRIGGER fail_session_fixture').run();
    assert.equal((await call('/v1/identities','GET',undefined,rollback.access_token)).status,200,'a failed replacement must roll back old session revocation');
    assert.equal((await call('/v1/sessions/refresh','POST',{refresh_token:rollback.refresh_token})).status,200,'old refresh remains valid after transaction rollback');
    for (const username of ['Player\tName', 'Player\u0085Name']) {
      assert.equal((await call('/v1/auth/login-challenges','POST',{provider_id:'official',profile_uuid:'cccccccc-cccc-4ccc-cccc-cccccccccccc',username})).status,400,'control characters in game usernames must be rejected like Rust');
    }
    const invalidName=await call('/v1/auth/login-challenges','POST',{provider_id:'official',profile_uuid:'cccccccc-cccc-4ccc-cccc-cccccccccccc',username:'😀'.repeat(33)});
    assert.equal(invalidName.status,400,'username length is limited to 64 UTF-16 units');
    console.log('PASS '+instanceId+' discovery; password/proof login and binding; replay/account/origin isolation; UTF-16; atomic refresh rotation/races/logout/expiry');
  } finally { await mf.dispose(); }
}
