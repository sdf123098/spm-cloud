const {test}=require('node:test');const assert=require('node:assert/strict');const Offline=require('./offline-identity.js');
test('account panel shows backend authorization and does not expose manual issuance',async()=>{
 const panel=Offline.create(async()=>({account_id:'cloud-user',enabled:true,authorization_status:'PENDING',identities:[]}));await panel.load();assert.equal(panel.state.authorizationStatus,'PENDING');assert.equal(panel.issue,undefined);assert.equal(panel.rotate,undefined);assert.equal(panel.state.secret,undefined);
});
test('late responses from a logged out account cannot repopulate its identities',async()=>{
 let resolve;const panel=Offline.create(()=>new Promise(r=>resolve=r));const pending=panel.load();panel.clear();resolve({account_id:'old',enabled:true,authorization_status:'APPROVED',identities:[{}]});await pending;assert.equal(panel.state.accountId,null);assert.deepEqual(panel.state.identities,[]);
});
test('device revocation remains available when instance authorization is closed',async()=>{
 const calls=[];const panel=Offline.create(async(path,options={})=>{calls.push({path,options});return {account_id:'u',enabled:false,authorization_status:'APPROVED',identities:[]};});await panel.load();await panel.revoke({identity_id:'coff_1'});assert.ok(calls.some(c=>c.path.endsWith('/key')&&c.options.method==='DELETE'));
});
