const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const S=require('./settings.js');
const schema=JSON.parse(fs.readFileSync(require.resolve('../config.schema.json'),'utf8'));
const response=(config={},writable=true)=>({config,revision:'revision-one',schema,writable,active:S.defaults(schema),field_status:{},provider_admin_entries:[]});
test('every current schema field and provider subfield has an explicit Chinese editor',()=>{
  const paths=Object.entries(schema.properties).flatMap(([group,value])=>value.properties?Object.keys(value.properties).map(key=>`${group}.${key}`):[]);
  assert.deepEqual([...Object.keys(S.registry)].sort(),paths.sort());
  for(const [path,meta]of Object.entries(S.registry)){assert.match(meta.title,/[\u4e00-\u9fff]/,path);assert.ok(meta.help,path);assert.ok(S.tabs.some(tab=>tab.id===meta.tab),path);assert.ok(meta.control,path);}
  assert.deepEqual(S.providerFields.slice().sort(),Object.keys(schema.properties.auth.properties.identity_providers.items.properties).sort());
});
test('unmodified nonintegral MiB, nullable paths and custom filters retain exact original values',()=>{
  const draft=new S.Draft(response({limits:{max_asset_bytes:134217729},logging:{level:'warn,spm_cloud[request{id=1}]=debug'},storage:{object_dir:null},performance:{preset:'custom',upload_concurrency:0}}));
  draft.set('logging.format','json');
  assert.equal(draft.payload().config.limits.max_asset_bytes,134217729);
  assert.equal(draft.payload().config.storage.object_dir,null);
  assert.equal(draft.payload().config.performance.upload_concurrency,0);
  assert.equal(draft.payload().config.logging.level,'warn,spm_cloud[request{id=1}]=debug');
  assert.deepEqual([...draft.dirtyPaths],['logging.format']);
});
test('unit conversions preserve boundaries and reject fractional byte values',()=>{
  for(const value of [1,134217729,4294967296])assert.equal(S.fromDisplay('limits.max_asset_bytes',S.toDisplay('limits.max_asset_bytes',value),schema),value);
  assert.equal(S.fromDisplay('moderation.score_threshold','70',schema),0.7);
  assert.throws(()=>S.fromDisplay('limits.max_message_bytes','0.0001',schema));
});
test('one draft survives tabs, searching, rejected saves, conflict and explicit rebase',()=>{
  const draft=new S.Draft(response());draft.set('cache.backend','none');
  for(const tab of S.tabs)draft.activeTab=tab.id;
  assert.ok(S.search('redis').some(item=>item.path==='cache.redis_url_file'));
  assert.equal(draft.payload().revision,'revision-one');assert.equal(draft.payload().config.cache.backend,'none');
  draft.rebase({...response({logging:{level:'debug'}}),revision:'revision-two'});
  assert.equal(draft.payload().config.cache.backend,'none');assert.equal(draft.payload().config.logging.level,'debug');assert.equal(draft.payload().revision,'revision-two');
  draft.set('cache.backend','memory');assert.equal(draft.dirtyPaths.size,0);
});
test('read-only and environment states cannot acquire writable drafts',()=>{
  const draft=new S.Draft(response({},false));assert.throws(()=>draft.set('logging.level','debug'));assert.throws(()=>draft.secret('mysql',{operation:'clear'}));
});
test('secrets are transient, explicit, omitted when kept and cleared after accept/logout',()=>{
  const draft=new S.Draft(response());draft.secret('redis',{operation:'replace',host:'localhost',password:'private'});
  assert.equal(draft.payload().managed_secrets.redis.password,'private');assert.ok(!JSON.stringify(draft.draft).includes('private'));
  draft.secret('redis',{operation:'keep'});assert.equal(draft.payload().managed_secrets,undefined);
  draft.secret('microsoft',{operation:'clear'});assert.equal(draft.payload().managed_secrets.microsoft.operation,'clear');
  draft.accept(response());assert.equal(draft.dirtyCount,0);draft.secret('bootstrap_token',{operation:'generate'});draft.clear();assert.equal(draft.dirtyCount,0);
});
test('standard, gateway and custom provider forms roundtrip without adopting legacy rows',()=>{
  const draft=new S.Draft({...response(),provider_admin_entries:[{provider_id:'legacy',display_name:'Legacy',base_url:'https://example.com',enabled:true,source:'database'}]});
  assert.deepEqual(draft.payload().config.auth.identity_providers,[]);
  for(const original of [{provider_id:'p',display_name:'Provider',base_url:'https://example.com/api',enabled:true},{provider_id:'p',display_name:'Provider',base_url:'',has_joined_url:'https://example.com/custom/hasJoined',authenticate_path:'/login',enabled:true},{provider_id:'p',display_name:'Provider',base_url:'https://example.com',session_path:'/session/hasJoined',authenticate_path:'/auth/authenticate',enabled:false}]){
    const form=S.providerForm(original);assert.deepEqual(S.providerValue(form,original),original);
    form.display_name='Renamed';assert.equal(S.providerValue(form,original).provider_id,'p');
  }
  assert.notEqual(S.providerFingerprint({base_url:'https://one.example.com'}),S.providerFingerprint({base_url:'https://two.example.com'}));
});
test('log directives containing scoped commas are not split inside a selector',()=>{
  assert.deepEqual(S.logRows('warn,spm_cloud[request{a=1,b=2}]=debug'),['warn','spm_cloud[request{a=1,b=2}]=debug']);
});
test('storage confirmation follows actual edits and keeps unused MySQL credentials separate',()=>{
  const draft=new S.Draft(response());
  assert.deepEqual(S.storageChanges(draft),[]);
  draft.secret('mysql',{operation:'replace',host:'db.example.com',database:'spm',password:'test-only'});
  assert.deepEqual(S.storageChanges(draft),[]);
  draft.set('storage.backend','mysql');
  assert.deepEqual(S.storageChanges(draft).sort(),['storage.backend','storage.mysql_url_file']);
  draft.clear();draft.set('storage.object_dir','other-models');
  assert.deepEqual(S.storageChanges(draft),['storage.object_dir']);
});
