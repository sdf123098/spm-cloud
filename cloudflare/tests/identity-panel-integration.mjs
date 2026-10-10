import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { build } from 'esbuild';
import { Miniflare } from 'miniflare';

const bundle = await build({entryPoints:['src/index.ts'],bundle:true,format:'esm',target:'es2022',write:false,external:['cloudflare:workers']});
const mf = new Miniflare({workers:[{config:{name:'identity-panel-test',compatibilityDate:'2026-09-22',
  manifest:{mainModule:'index.js',modulesRoot:process.cwd(),modules:{'index.js':{type:'esm',contents:bundle.outputFiles[0].text}}},
  env:{SPM_CLOUD_ACCESS_TOKEN:{type:'text',value:'isolated-panel-token'},SPM_CLOUD_INSTANCE_ID:{type:'text',value:'panel-test'},SPM_CLOUD_ORIGIN:{type:'text',value:'https://panel.example.test'},
    DB:{type:'d1',name:'identity-panel-test'},ASSETS:{type:'r2',name:'identity-panel-assets'}}}}]});
async function request(path,method='GET',body,token='isolated-panel-token') {
  const response=await mf.dispatchFetch('https://panel.example.test'+path,{method,headers:{...(token?{authorization:'Bearer '+token}:{}),'content-type':'application/json'},...(body?{body:JSON.stringify(body)}:{})});
  return {status:response.status,body:response.status===204?null:await response.json()};
}
try {
  const db=await mf.getD1Database('DB');
  for(const name of (await readdir('migrations')).filter(n=>n.endsWith('.sql')).sort()) {
    const sql=(await readFile('migrations/'+name,'utf8')).replace(/^\s*--.*$/gm,'');
    for(const statement of sql.split(';').map(s=>s.trim()).filter(Boolean)) await db.prepare(statement).run();
  }
  assert.equal((await request('/v1/accounts','POST',{account_id:'panel-user',password:'panel-user-password'})).status,201);
  const login=await request('/v1/sessions','POST',{account_id:'panel-user',password:'panel-user-password'});
  assert.equal(login.status,200);const token=login.body.access_token;
  assert.equal((await request('/v1/scopes','POST',{scope_id:'panel-world',name:'Panel World',world_epoch:'epoch',offline_policy:'STRICT_APPROVAL'})).status,201);
  const url='/v1/scopes/panel-world/permissions';
  assert.equal((await request(url,'GET',undefined,null)).status,401);
  assert.equal((await request(url,'GET',undefined,token)).status,403);
  for(const role of ['viewer','editor','manage']) {
    assert.equal((await request('/v1/scopes/panel-world/acl','PUT',{account_id:'panel-user',role})).status,200);
    const own=await request(url,'GET',undefined,token);
    assert.equal(own.status,200);assert.deepEqual(own.body,{scope_id:'panel-world',role,offline_policy:'STRICT_APPROVAL'});
  }
  assert.equal((await request('/v1/scopes/missing/permissions','GET',undefined,token)).status,403);
  // Exercise the preserved register -> apply -> versioned administrator approval flow.
  assert.equal((await request('/v1/targets','POST',{scope_id:'panel-world',kind:'PLAYER',display_name:'Player Character',target_id:'panel-player'})).status,201);
  const identity=await request('/v1/identities','POST',{identity:'offline:panel-world:12345678-1234-1234-1234-1234567890ab',display_name:'Player'},token);
  assert.equal(identity.status,201);
  const application=await request('/v1/identities/'+identity.body.identity_id+'/offline-bindings','POST',{identity_id:identity.body.identity_id,scope_id:'panel-world',world_epoch:'epoch',target_id:'panel-player'},token);
  assert.equal(application.status,201);
  assert.equal(application.body.status,'PENDING_APPROVAL');
  const pending=await request('/v1/scopes/panel-world/offline-bindings');
  assert.equal(pending.status,200);
  assert.equal(pending.body[0].identity_display_name,'Player');
  assert.equal(pending.body[0].target_display_name,'Player Character');
  await request('/v1/scopes/panel-world/acl','PUT',{account_id:'panel-user',role:'editor'});
  const approval='/v1/scoped-identity-bindings/'+application.body.binding_id;
  assert.equal((await request(approval,'PUT',{status:'APPROVED',expected_revision:application.body.revision},token)).status,403);
  assert.equal((await request(approval,'PUT',{status:'APPROVED',expected_revision:999})).status,409);
  assert.equal((await request(approval,'PUT',{status:'APPROVED',expected_revision:application.body.revision})).status,200);
  await request('/v1/scopes/panel-world/acl','PUT',{account_id:'panel-user',role:'viewer'});
  assert.equal((await request(url,'GET',undefined,token)).body.role,'viewer');
  assert.equal((await request('/v1/identities','POST',{identity:'offline:panel-world:12345678-1234-1234-1234-1234567890ac',display_name:'Denied'},token)).status,403);
  // Real invite creation, redemption, replay denial and revocation against isolated D1.
  assert.equal((await request('/v1/scopes','POST',{scope_id:'invite-world',name:'Invite World',world_epoch:'epoch',offline_policy:'CLAIM_CODE'})).status,201);
  await request('/v1/scopes/invite-world/acl','PUT',{account_id:'panel-user',role:'editor'});
  assert.equal((await request('/v1/targets','POST',{scope_id:'invite-world',kind:'PLAYER',display_name:'Invited Character',target_id:'invite-player'})).status,201);
  const invited=await request('/v1/identities','POST',{identity:'offline:invite-world:12345678-1234-3234-9234-1234567890ab',display_name:'Player'},token);
  assert.equal(invited.status,201);
  const issuePath='/v1/targets/invite-player/claim-codes';
  const issueBody={world_epoch:'epoch',entity_uuid:'12345678-1234-3234-9234-1234567890ab',expires_in_seconds:600};
  assert.equal((await request(issuePath,'POST',issueBody,token)).status,403);
  const invite=await request(issuePath,'POST',issueBody);
  assert.equal(invite.status,201);
  assert.equal(invite.body.expires_in_seconds,600);
  const redeemBody={code:invite.body.code,identity_id:invited.body.identity_id};
  const redeemed=await request('/v1/claim-codes/redeem','POST',redeemBody,token);
  assert.equal(redeemed.status,200);assert.equal(redeemed.body.status,'APPROVED');
  assert.equal((await request('/v1/claim-codes/redeem','POST',redeemBody,token)).status,403);
  const unused=await request(issuePath,'POST',issueBody);
  assert.equal(unused.status,201);
  assert.equal((await request('/v1/claim-codes/revoke','POST',{code:unused.body.code},token)).status,403);
  assert.equal((await request('/v1/claim-codes/revoke','POST',{code:unused.body.code})).status,204);
  assert.equal((await request('/v1/claim-codes/redeem','POST',{code:unused.body.code,identity_id:invited.body.identity_id},token)).status,404);
  console.log('PASS authenticated self permissions, viewer/editor/manage, missing scope, preserved offline registration/application, admin-only revision approval and permission revocation');
} finally { await mf.dispose(); }
