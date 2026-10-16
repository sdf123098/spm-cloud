'use strict';
const $ = (id) => document.getElementById(id);
const accountPortal=location.pathname.startsWith('/account/');
let session = null, configResponse = null, formConfig = null, nextCursor = null, nextAccountCursor = null, noticeTimer;
let models = [], modelView = 'grid', selectedModels = new Set(), bulkRunning = false;
let uploadQueue = [], uploadRunning = false;
try { if (localStorage.getItem('spm-cloud-model-view') === 'cards') modelView = 'cards'; } catch {}
const roleNames = { owner: '服主', admin: '管理员', member: '普通账号' };
const statusNames = { R18: 'R18', CLEAN: '未检出', UNKNOWN: '未识别', DISABLED: '识别关闭' };
const tabs = { overview: '概览', models: '模型库', settings: '服务器配置', accounts: '账号与权限', profile:'我的账号与绑定' };
function notice(message,error=false){ $('notice').textContent=message;$('notice').className=error?'error':'';$('notice').hidden=false;clearTimeout(noticeTimer);noticeTimer=setTimeout(()=>{$('notice').hidden=true;},error?12000:7000); }
async function request(path,options={},retry=true){
  const headers=new Headers(options.headers||{}),accessToken=session?.access_token;if(accessToken)headers.set('Authorization',`Bearer ${accessToken}`);
  if(options.json!==undefined){headers.set('Content-Type','application/json');options.body=JSON.stringify(options.json);}
  const response=await fetch(path,{...options,headers});
  const credentialProof=path==='/account/api/link-cloud'||path==='/account/api/game';
  if(response.status===401&&!credentialProof&&retry&&session?.refresh_token&&session.access_token===accessToken){
    const refreshed=await fetch('/v1/sessions/refresh',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({refresh_token:session.refresh_token})});
    if(refreshed.ok){session={...session,...await refreshed.json()};return request(path,options,false);}
  }
  if(!response.ok){let detail;try{detail=await response.json();}catch{detail={message:`HTTP ${response.status}`};}
    if(response.status===401&&!credentialProof&&session&&session.access_token===accessToken&&!settingsUI?.saving){settingsUI?.clear();session=null;$('console').hidden=true;$('login').hidden=false;}
    const messages={R18_MODEL_BLOCKED:'该模型被识别或标记为 R18，已按内容策略阻止上传。',MODEL_UNCLASSIFIED:'该模型未能完成内容识别，已按“拒绝未能完成识别的模型”策略阻止上传。',ASSET_TOO_LARGE:'文件超过服务器允许的模型大小，请调整大小上限或选择较小文件。',IDENTITY_NOT_LINKED:'该游戏身份尚未绑定 Cloud 账号，请先用 Cloud 账号登录并完成绑定。',IDENTITY_ALREADY_LINKED:'该身份或账号已有不同的关联，请检查当前绑定，不能覆盖他人的关联。',IDENTITY_PROFILE_MISMATCH:'身份验证失败，请检查账号、令牌或微软应用授权。',IDENTITY_PROVIDER_UNAVAILABLE:'身份服务暂时无法访问，请稍后重试。',IDENTITY_PROVIDER_UNTRUSTED:'该身份服务尚未启用，或服务地址不符合要求。',ASSET_ACCESS_DENIED:'当前账号没有执行此操作的权限。',UNAUTHENTICATED:'账号或密码不正确，或当前登录已失效。',IDENTITY_CHALLENGE_EXPIRED:'验证已过期，请重新开始。',RATE_LIMITED:'操作过于频繁，请稍后再试。'};
    const error=new Error(messages[detail.code]||detail.message||detail.code||`HTTP ${response.status}`);error.status=response.status;error.code=detail.code;error.errors=detail.errors;throw error;
  }
  if(response.status===204)return null;return response.json();
}
async function busy(button,fn){button.disabled=true;try{return await fn();}catch(e){notice(e.message,true);}finally{button.disabled=false;}}
function node(tag,text,className){const el=document.createElement(tag);if(text!==undefined)el.textContent=text;if(className)el.className=className;return el;}
function icon(name){const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.classList.add('icon');svg.setAttribute('aria-hidden','true');const use=document.createElementNS(svg.namespaceURI,'use');use.setAttribute('href',`/admin/icons.svg#icon-${name}`);svg.append(use);return svg;}
document.querySelectorAll('[data-icon]').forEach(el=>el.append(icon(el.dataset.icon)));
function action(text,fn,iconName){const b=node('button');b.type='button';if(iconName)b.append(icon(iconName));b.append(document.createTextNode(text));b.addEventListener('click',()=>busy(b,fn));return b;}
function cell(row,text){const el=node('td',text);row.append(el);return el;}
function options(select,entries,current){for(const [value,label] of entries){const o=node('option',label);o.value=value;select.append(o);}select.value=current;}
function bytes(n){if(!Number.isFinite(Number(n)))return '未知';n=Number(n);if(n<1024)return `${n} B`;const i=Math.min(3,Math.floor(Math.log(n)/Math.log(1024)));return `${(n/1024**i).toFixed(2)} ${['B','KiB','MiB','GiB'][i]}`;}
async function show(tab,settingsTab){if(tab==='maintenance'||tab==='audit'){settingsTab=tab==='maintenance'?'updates':'audit';tab='settings';}if(tab==='accounts'&&session?.role!=='owner')return;
  if(accountPortal&&tab!=='profile')return;
  for(const name of Object.keys(tabs))$(`tab-${name}`).hidden=name!==tab;
  for(const button of document.querySelectorAll('[data-tab]'))button.classList.toggle('active',button.dataset.tab===tab);
  $('breadcrumb').textContent=tabs[tab];
  document.querySelector('.content').scrollTop=0;
  try{if(tab==='overview')await loadOverview();if(tab==='models'){await loadModels();await loadOwners();}if(tab==='settings')await loadSettings(settingsTab);if(tab==='accounts')await loadAccounts();if(tab==='profile')await loadProfile();}catch(e){notice(e.message,true);}
}
async function loadOverview(){const result=await request('/admin/api/config');const effective=result.effective?.effective||result.effective;const moderation=effective.moderation||{};for(const [id,key]of [['detection','detection_enabled'],['block','block_r18'],['texture','texture_detection_enabled']])$(`overview-${id}`).textContent=moderation[key]===true?'已开启':moderation[key]===false?'已关闭':'未配置';$('overview-instance').textContent=effective.instance?.instance_id||'—';$('overview-origin').textContent=effective.instance?.origin||location.origin;$('overview-role').textContent=roleNames[session.role];}
async function enter(result){session=result;const me=await request('/account/api/profile');session={...session,...me};
  $('login').hidden=true;$('console').hidden=false;$('identity').textContent=me.account_id;$('role-badge').textContent=roleNames[me.role];
  document.querySelectorAll('[data-owner]').forEach(el=>el.hidden=accountPortal||me.role!=='owner');
  document.querySelectorAll('.sidebar [data-tab]').forEach(el=>{if(accountPortal)el.hidden=el.dataset.tab!=='profile';});
  $('upload-owner').value=me.cloud_account_id;
  await show(accountPortal?'profile':'overview');
}
for(const id of ['login-form','token-form'])$(id).addEventListener('submit',e=>{e.preventDefault();busy(e.submitter,async()=>{const data=Object.fromEntries(new FormData(e.target));const result=await request(accountPortal&&id==='login-form'?'/account/api/login':'/admin/api/login',{method:'POST',json:data});e.target.reset();await enter(result);});});
$('logout').addEventListener('click',()=>{if(settingsUI.saving)return;return busy($('logout'),async()=>{if(session?.refresh_token)try{await request('/v1/sessions/current',{method:'DELETE'});}catch{}settingsUI.clear();session=null;configResponse=null;formConfig=null;models=[];SPMPreview.clear();selectedModels.clear();uploadQueue=[];$('upload-form').reset();for(const form of document.querySelectorAll('.game-form,.game-token-form,#self-cloud-form'))form.reset();renderUploads();$('upload-status').textContent='';$('model-cards').replaceChildren();syncSelection();$('model-detail').close();$('raw-config').value='';$('console').hidden=true;$('login').hidden=false;});});
document.querySelectorAll('[data-tab]').forEach(b=>b.addEventListener('click',()=>show(b.dataset.tab)));
document.querySelectorAll('[data-go]').forEach(b=>b.addEventListener('click',()=>show(b.dataset.go,b.dataset.settingsTarget)));
$('show-upload').addEventListener('click',()=>{$('upload-form').hidden=false;});
$('cancel-upload').addEventListener('click',()=>{if(!uploadRunning)$('upload-form').hidden=true;});
function renderUploads(){
  const root=$('upload-queue');root.replaceChildren();root.hidden=!uploadQueue.length;
  for(const item of uploadQueue){const row=node('li',undefined,`upload-item ${item.state}`),title=node('span',item.file.name,'upload-filename');title.title=item.file.name;row.append(title,node('small',bytes(item.file.size)),node('span',item.message||'等待上传','upload-result'));root.append(row);}
  $('upload-name-label').textContent=uploadQueue.length>1?'名称前缀（可选）':'模型名称（可选）';
  $('upload-name').placeholder=uploadQueue.length>1?'留空则分别使用文件名':'留空则使用文件名';
  const pending=uploadQueue.filter(item=>item.state!=='done').length;
  $('submit-upload').textContent=uploadRunning?'正在上传…':!pending&&uploadQueue.length?'已全部上传':uploadQueue.some(item=>item.state==='failed')?'重试未完成项':`上传${uploadQueue.length?` ${uploadQueue.length} 个模型`:'并识别'}`;
  $('submit-upload').disabled=uploadRunning||(!pending&&uploadQueue.length>0);
}
$('upload-files').addEventListener('change',event=>{
  uploadQueue=Array.from(event.target.files, file=>({file,state:'pending',message:'等待上传',key:crypto.randomUUID(),metadata:null}));
  $('upload-status').textContent=uploadQueue.length?`已选择 ${uploadQueue.length} 个本机文件`:'请选择模型文件';renderUploads();
});
$('upload-form').addEventListener('submit',async e=>{
  e.preventDefault();if(uploadRunning)return;
  if(!uploadQueue.length){notice('请选择模型文件，可以一次选择多个。',true);return;}
  const form=e.target,data=new FormData(form),prefix=String(data.get('name')||'').trim(),visibility=data.get('visibility'),owner=String(data.get('owner_account_id')||'').trim();
  if(visibility==='PRIVATE'&&!owner){notice('私有模型必须选择归属 Cloud 账号。',true);return;}
  const targets=uploadQueue.filter(item=>item.state!=='done');if(!targets.length)return;
  uploadRunning=true;$('logout').disabled=true;for(const control of form.querySelectorAll('input,select,button'))control.disabled=true;renderUploads();let succeeded=0;
  try{
    for(const [index,item]of targets.entries()){
      item.state='uploading';item.message='上传与内容识别中…';$('upload-status').textContent=`正在处理 ${index+1}/${targets.length}：${item.file.name}`;renderUploads();
      try{
        const file=item.file,format=file.name.split('.').pop().toLowerCase();
        if(!file.size)throw new Error('文件为空，请重新选择');
        if(!['ysm','zip','bbmodel','gltf','glb'].includes(format))throw new Error('不支持此格式，请选择 YSM、ZIP、BBModel、GLTF 或 GLB');
        const name=prefix?(uploadQueue.length>1?`${prefix} · ${file.name}`:prefix):file.name;
        const metadata=JSON.stringify({name,visibility,owner});if(item.metadata!==null&&item.metadata!==metadata)item.key=crypto.randomUUID();item.metadata=metadata;
        const headers={'Content-Type':'application/octet-stream','Idempotency-Key':item.key,'X-Asset-Name':encodeURIComponent(name),'X-Asset-Format':format,'X-Asset-Visibility':visibility,'X-Asset-Metadata-Encoding':'utf-8-percent'};if(owner)headers['X-Asset-Owner-Account']=owner;
        const result=await request('/admin/api/assets',{method:'POST',body:file,headers});
        item.state='done';item.message=`已上传：${result.name}`;succeeded++;
      }catch(error){item.state='failed';item.message=error.message;if(!session)break;}
      renderUploads();
    }
    const failed=uploadQueue.filter(item=>item.state==='failed').length,pending=uploadQueue.filter(item=>item.state==='pending').length;
    $('upload-status').textContent=`本次成功 ${succeeded} 个${failed?`，失败 ${failed} 个，可重试未完成项`:''}${pending?`，待处理 ${pending} 个`:''}。`;
    if(session&&succeeded)try{await loadModels();}catch(error){notice(`上传结果已保留，模型列表刷新失败：${error.message}`,true);}
  }finally{uploadRunning=false;$('logout').disabled=bulkRunning||settingsUI.saving;for(const control of form.querySelectorAll('input,select,button'))control.disabled=false;renderUploads();}
});
$('search-form').addEventListener('submit',e=>{e.preventDefault();busy(e.submitter,()=>loadModels());});$('include-deleted').addEventListener('change',()=>loadModels().catch(e=>notice(e.message,true)));$('refresh-models').addEventListener('click',()=>busy($('refresh-models'),()=>loadModels()));
function setModelView(view){
  modelView=view==='cards'?'cards':'grid';
  $('model-cards').dataset.view=modelView;
  document.querySelectorAll('button[data-view]').forEach(button=>button.setAttribute('aria-pressed',String(button.dataset.view===modelView)));
  $('model-cards').querySelectorAll('.model-card').forEach(card=>card.style.removeProperty('transform'));
  try{localStorage.setItem('spm-cloud-model-view',modelView);}catch{}
  if(models.length)renderModels();
}
document.querySelectorAll('button[data-view]').forEach(button=>button.addEventListener('click',()=>setModelView(button.dataset.view)));
setModelView(modelView);
const reducedMotion=matchMedia('(prefers-reduced-motion: reduce)'), finePointer=matchMedia('(hover: hover) and (pointer: fine)');
$('model-cards').addEventListener('pointermove',event=>{
  if(modelView!=='cards'||reducedMotion.matches||!finePointer.matches||event.pointerType==='touch')return;
  const slot=event.target.closest('.model-slot');if(!slot)return;
  const rect=slot.getBoundingClientRect(), x=Math.max(-.5,Math.min(.5,(event.clientX-rect.left)/rect.width-.5)), y=Math.max(-.5,Math.min(.5,(event.clientY-rect.top)/rect.height-.5));
  slot.firstElementChild.style.transform=`rotateX(${-y*10}deg) rotateY(${x*10}deg) translateY(-3px)`;
});
$('model-cards').addEventListener('pointerout',event=>{const slot=event.target.closest('.model-slot');if(slot&&!slot.contains(event.relatedTarget))slot.firstElementChild.style.removeProperty('transform');});
reducedMotion.addEventListener('change',()=>setModelView(modelView));

let modelPage=1,accountPage=1,auditPage=1;
const paging={models:{page:1,size:24,total:0},accounts:{page:1,size:24,total:0},audit:{page:1,size:25,total:0}};
const loadSerial={models:0,accounts:0,audit:0};
function renderPagination(kind,result){
  const state=paging[kind];state.page=result.page;state.size=result.page_size;state.total=result.total;
  const pages=Math.max(1,Math.ceil(state.total/state.size));
  for(const root of document.querySelectorAll(`[data-pagination="${kind}"]`)){
    root.className='pagination';root.replaceChildren();
    const go=page=>{const fn={models:loadModels,accounts:loadAccounts,audit:loadAudit}[kind];return fn(page);};
    const previous=action('上一页',()=>go(state.page-1));previous.disabled=state.page<=1;
    const next=action('下一页',()=>go(state.page+1));next.disabled=state.page>=pages;
    const label=node('span',`第 ${state.page} / ${pages} 页 · 共 ${state.total} 项`,'page-summary');
    const size=node('select');size.setAttribute('aria-label','每页显示数量');options(size,[...[24,48,80].map(n=>[String(n),`${n} 项 / 页`]),...(kind==='audit'?[['25','25 项 / 页']]:[])],String(state.size));
    size.addEventListener('change',()=>{state.size=Number(size.value);busy(size,()=>go(1));});
    const jump=node('form',undefined,'page-jump'),input=node('input');input.type='number';input.min=1;input.max=pages;input.value=state.page;input.setAttribute('aria-label','跳转页码');const submit=node('button','跳转');submit.type='submit';jump.append(input,submit);jump.addEventListener('submit',e=>{e.preventDefault();if(input.checkValidity())busy(submit,()=>go(Number(input.value)));});
    root.append(previous,label,next,size,jump);
  }
}
async function loadModels(page=1){
  const serial=++loadSerial.models;
  const params=new URLSearchParams({q:new FormData($('search-form')).get('q'),deleted:$('include-deleted').checked,page,page_size:paging.models.size});
  const result=await request(`/admin/api/assets?${params}`);if(serial!==loadSerial.models)return;
  if(page>1&&!result.entries.length&&result.total)return loadModels(Math.max(1,Math.ceil(result.total/result.page_size)));
  models=result.entries;modelPage=result.page;nextCursor=result.next_cursor;renderModels();renderPagination('models',result);
  $('model-count').textContent=`本页 ${models.length} 个 · 共 ${result.total} 个模型`;
}

function syncSelection(){
  const visible=new Set(models.map(model=>model.asset_id));
  selectedModels=new Set([...selectedModels].filter(id=>visible.has(id)));
  const count=selectedModels.size, all=$('select-all-models');
  all.checked=models.length>0&&count===models.length;all.indeterminate=count>0&&count<models.length;all.disabled=bulkRunning||models.length===0;
  $('selection-toolbar').hidden=count===0;$('selection-count').textContent=`已选 ${count} 个`;
  for(const el of document.querySelectorAll('#selection-toolbar button, #selection-toolbar select, #selection-toolbar input'))el.disabled=bulkRunning;
  $('logout').disabled=bulkRunning||uploadRunning||settingsUI.saving;
  for(const card of $('model-cards').querySelectorAll('.model-card')){const checked=selectedModels.has(card.dataset.asset);card.classList.toggle('selected',checked);const input=card.querySelector('input[type=checkbox]');input.checked=checked;input.disabled=bulkRunning;}
  document.querySelectorAll('.model-name').forEach(button=>button.disabled=bulkRunning);
  const active=models.filter(model=>selectedModels.has(model.asset_id)&&!model.deleted).length;
  for(const el of document.querySelectorAll('[data-bulk]'))el.disabled=bulkRunning||(el.dataset.bulk==='restore'?count===active:active===0);
  $('bulk-mark').disabled=bulkRunning||active===0;
}
$('select-all-models').addEventListener('change',event=>{selectedModels=event.target.checked?new Set(models.map(model=>model.asset_id)):new Set();syncSelection();});
$('clear-selection').addEventListener('click',()=>{selectedModels.clear();syncSelection();});
$('close-detail').addEventListener('click',()=>$('model-detail').close());
$('model-detail').addEventListener('click',event=>{if(event.target===$('model-detail')){const rect=event.target.getBoundingClientRect();if(event.clientX<rect.left||event.clientX>rect.right||event.clientY<rect.top||event.clientY>rect.bottom)event.target.close();}});

function renderModels(){
  const root=$('model-cards');root.replaceChildren();
  for(const model of models){
    const slot=node('div',undefined,'model-slot'), card=node('article',undefined,`model-card${model.deleted?' deleted':''}`);card.dataset.asset=model.asset_id;card.setAttribute('aria-label',model.name);
    const visual=node('div',undefined,'model-visual'), pick=node('input');pick.type='checkbox';pick.setAttribute('aria-label',`选择 ${model.name}`);pick.className='model-pick';pick.addEventListener('change',()=>{if(pick.checked)selectedModels.add(model.asset_id);else selectedModels.delete(model.asset_id);syncSelection();});
    visual.append(pick,...(modelView==='cards'?[]:[icon('box')]),node('span',(model.format||'MODEL').toUpperCase(),'format-tag'));
    const body=node('div',undefined,'model-body'), title=action(model.name,()=>showModelDetail(model));title.className='model-name';title.title=`查看 ${model.name} 的详情`;body.append(title);
    const status=node('div',undefined,'model-status'), known=Object.hasOwn(statusNames,model.moderation)?model.moderation:'UNKNOWN';
    status.append(node('span',statusNames[known],`badge status-${known}`));if(model.deleted)status.append(node('span','已删除','badge deleted-label'));
    else{const visibility=node('span',undefined,'visibility');visibility.append(icon(model.visibility==='PUBLIC'?'globe':'lock-keyhole'));visibility.title=model.visibility==='PUBLIC'?'公开':'私有';visibility.setAttribute('aria-label',visibility.title);status.append(visibility);}
    const meta=node('div',undefined,'model-meta'), owner=node('span',model.owner_account_id,'model-owner');owner.title=`所有者：${model.owner_account_id}`;const size=node('span',bytes(model.byte_length));size.title=`${model.byte_length} 字节（原始文件）`;meta.append(owner,size);body.append(status,meta);
    card.append(visual,body);slot.append(card);root.append(slot);
  }
  $('model-empty').hidden=models.length>0;syncSelection();if(modelView==='cards')SPMPreview.mount(models.map((model,i)=>({model,visual:root.children[i].querySelector('.model-visual')})),request);else SPMPreview.clear();
}

function showModelDetail(model){
  const root=$('model-detail-content');root.replaceChildren();$('model-detail-title').textContent=model.name;
  const metadata=node('dl');for(const [label,value]of [['模型 ID',model.asset_id],['所有者',model.owner_account_id],['格式',(model.format||'').toUpperCase()],['大小',bytes(model.byte_length)],['版本',`v${model.revision}`],['可见性',model.visibility==='PUBLIC'?'公开':'私有'],['内容识别',statusNames[model.moderation]||model.moderation],['人工标记',model.manual_status?(statusNames[model.manual_status]||model.manual_status):'无'],['状态',model.deleted?'已删除':'正常']]){const row=node('div');row.append(node('dt',label),node('dd',value));metadata.append(row);}root.append(metadata,node('p',model.reason||'尚无识别结果','detail-reason'));
  if(model.scan?.textures_scanned)root.append(node('p',`已检测 ${model.scan.textures_scanned} 张贴图`,'small muted'));
  const actions=node('div',undefined,'model-actions'), path=`/admin/api/assets/${encodeURIComponent(model.asset_id)}`;
  actions.append(action('导出原始模型',()=>download(`${path}/export`),'file-box'));
  const update=async(action)=>{await action();$('model-detail').close();await loadModels(modelPage);};
  if(model.deleted)actions.append(action('恢复模型',()=>update(()=>request(`${path}/restore`,{method:'POST'})),'rotate-ccw'));
  else{
    actions.append(action(model.visibility==='PUBLIC'?'设为私有':'设为公开',()=>update(()=>request(`${path}/visibility`,{method:'PUT',json:{visibility:model.visibility==='PUBLIC'?'PRIVATE':'PUBLIC'}})),model.visibility==='PUBLIC'?'lock-keyhole':'globe'));
    actions.append(action('重新识别',()=>update(async()=>{const scan=await request(`${path}/scan`,{method:'POST'});notice(`${statusNames[scan.status]||scan.status}：${scan.reason}`);}),'scan-line'));
    const remove=action('删除模型',async()=>{if(!confirm(`删除模型“${model.name}”？它将停止下载和使用，之后可以恢复。`))return;await update(()=>request(path,{method:'DELETE'}));},'trash');remove.classList.add('danger-text');actions.append(remove);
    const mark=node('label',undefined,'model-mark');mark.append(document.createTextNode('人工复核'));const select=node('select');select.setAttribute('aria-label',`${model.name} 人工标记`);options(select,[['','选择标记…'],['R18','标为 R18'],['CLEAN','人工确认通过'],['UNKNOWN','标为未识别'],['auto','移除人工标记']],'');select.addEventListener('change',()=>busy(select,async()=>{if(select.value)await update(()=>request(`${path}/moderation`,{method:'PUT',json:{status:select.value==='auto'?null:select.value}}));}));mark.append(select);actions.append(mark);
  }
  root.append(actions);if(!$('model-detail').open)$('model-detail').showModal();
  if(!model.deleted){const form=node('div',undefined,'model-owner-form'),label=node('label','归属 Cloud 账号'),input=node('input');input.value=model.owner_account_id;input.setAttribute('list','model-owners');input.setAttribute('aria-label','模型归属 Cloud 账号');label.append(input);form.append(label,action('保存归属',async()=>{const owner=input.value.trim();if(!owner)throw new Error('请选择归属 Cloud 账号');if(owner===model.owner_account_id)return;if(!confirm(`将模型归属改为“${owner}”？原账号及现有共享授权将移除。`))return;await update(()=>request(`${path}/owner`,{method:'PUT',json:{owner_account_id:owner,expected_owner_account_id:model.owner_account_id}}));}));root.append(form);}
}

async function runBulk(kind,mark){
  if(bulkRunning)return;
  const targets=models.filter(model=>selectedModels.has(model.asset_id)&&(kind==='restore'?model.deleted:!model.deleted));
  if(!targets.length)return;
  const owner=$('bulk-owner').value.trim();if(kind==='owner'&&!owner){notice('先填写目标 Cloud 账号。',true);return;}
  if(kind==='owner'&&!confirm(`将 ${targets.length} 个模型归属改为“${owner}”？原账号及现有共享授权将移除。`))return;
  if(kind==='delete'&&!confirm(`删除选中的 ${targets.length} 个模型？它们将停止下载和使用，之后可以恢复。`))return;
  if(kind==='mark'&&mark==='R18'&&!confirm(`将选中的 ${targets.length} 个模型标为 R18？相同内容的其他模型也会受此标记影响。`))return;
  bulkRunning=true;syncSelection();let succeeded=0;const failed=[];
  try{
    for(const [index,model]of targets.entries()){
      const path=`/admin/api/assets/${encodeURIComponent(model.asset_id)}`;
      $('selection-count').textContent=`正在处理 ${index+1}/${targets.length}`;
      notice(`正在处理 ${index+1}/${targets.length}：${model.name}`);
      try{
        if(kind==='delete')await request(path,{method:'DELETE'});
        else if(kind==='restore')await request(`${path}/restore`,{method:'POST'});
        else if(kind==='scan')await request(`${path}/scan`,{method:'POST'});
        else if(kind==='mark')await request(`${path}/moderation`,{method:'PUT',json:{status:mark==='auto'?null:mark}});
        else if(kind==='owner')await request(`${path}/owner`,{method:'PUT',json:{owner_account_id:owner,expected_owner_account_id:model.owner_account_id}});
        else await request(`${path}/visibility`,{method:'PUT',json:{visibility:kind==='public'?'PUBLIC':'PRIVATE'}});
        succeeded++;selectedModels.delete(model.asset_id);
      }catch(error){failed.push(`${model.name}：${error.message}`);if(!session)break;}
    }
    if(session)await loadModels();
    notice(failed.length?`已完成 ${succeeded} 个，失败 ${failed.length} 个。${failed.join('；')}`:`已完成 ${succeeded} 个模型的批量操作。`,failed.length>0);
  }catch(error){notice(`已处理 ${succeeded} 个模型；刷新失败：${error.message}`,true);}
  finally{bulkRunning=false;$('bulk-mark').value='';syncSelection();}
}
document.querySelectorAll('[data-bulk]').forEach(button=>button.addEventListener('click',()=>runBulk(button.dataset.bulk)));
$('bulk-mark').addEventListener('change',event=>{if(event.target.value)runBulk('mark',event.target.value);});

async function loadSettings(tab){await settingsUI.open(tab);}
$('account-form').addEventListener('submit',e=>{e.preventDefault();busy(e.submitter,async()=>{await request('/admin/api/accounts',{method:'POST',json:Object.fromEntries(new FormData(e.target))});e.target.reset();notice('账号已创建，请自行分发账号和初始密码。');await loadAccounts();});});
async function loadAccounts(page=1){
  const serial=++loadSerial.accounts;const params=new URLSearchParams({q:new FormData($('account-search-form')).get('q'),page,page_size:paging.accounts.size});
  const result=await request(`/admin/api/accounts?${params}`);if(serial!==loadSerial.accounts)return;
  if(page>1&&!result.entries.length&&result.total)return loadAccounts(Math.max(1,Math.ceil(result.total/result.page_size)));
  accountPage=result.page;const root=$('account-rows');root.replaceChildren();
  for(const account of result.entries){const card=node('article',undefined,`account-card${account.role==='owner'?' owner-card':''}`);const title=node('h2',account.account_id);card.append(title,node('span',roleNames[account.role],'badge'),node('p',account.disabled?'已停用':'正常','small muted'),node('p',`创建于 ${account.created_at}`,'small muted'));const actions=node('div',undefined,'account-actions');
    const path=`/admin/api/accounts/${encodeURIComponent(account.account_id)}`;const change=async(body)=>{await request(path,{method:'PUT',json:{role:account.role,disabled:account.disabled,...body}});await loadAccounts(accountPage);};
    if(account.role!=='owner')actions.append(action(account.disabled?'启用':'停用',()=>change({disabled:!account.disabled})),action(account.role==='admin'?'撤销管理权':'授权管理员',()=>change({role:account.role==='admin'?'member':'admin'})),action('重置密码',async()=>{const password=prompt('输入新密码（至少 8 个字符）。已有会话将失效。');if(password!==null)await change({password});}));
    else actions.append(action('设置服主密码',async()=>{const password=prompt('为服主设置登录密码（至少 8 个字符）。已有密码会话将失效，初始令牌仍可使用。');if(password!==null){await change({role:'owner',disabled:false,password});notice('服主密码已设置，可使用账号和密码登录。');}}));
    card.append(actions);root.append(card);
  }
  $('account-empty').hidden=result.entries.length>0;renderPagination('accounts',result);
}
async function loadAudit(page=1){
  const serial=++loadSerial.audit;const params=new URLSearchParams({q:new FormData($('audit-search-form')).get('q'),page,page_size:paging.audit.size});const result=await request(`/admin/api/audit?${params}`);if(serial!==loadSerial.audit)return;auditPage=result.page;
  $('audit-rows').replaceChildren();for(const event of result.entries){const row=node('tr');cell(row,event.created_at);cell(row,event.actor);cell(row,event.action);const subject=cell(row,event.subject||'—');subject.title=JSON.stringify(event.details);$('audit-rows').append(row);}
  $('audit-empty').hidden=result.entries.length>0;renderPagination('audit',result);
}
$('refresh-audit').addEventListener('click',()=>busy($('refresh-audit'),()=>loadAudit(auditPage)));
$('audit-search-form').addEventListener('submit',e=>{e.preventDefault();busy(e.submitter,()=>loadAudit());});
$('export-audit').addEventListener('click',()=>busy($('export-audit'),async()=>{if(!await settingsUI.beforeMaintenance())return;await download('/admin/api/exports/audit?'+new URLSearchParams({q:new FormData($('audit-search-form')).get('q')}));}));
$('account-search-form').addEventListener('submit',e=>{e.preventDefault();busy(e.submitter,()=>loadAccounts());});$('refresh-accounts').addEventListener('click',()=>busy($('refresh-accounts'),()=>loadAccounts(accountPage)));

let ownerSearchTimer,authProviders=[];
async function loadOwners(q=''){
  if(accountPortal||!session)return;
  const result=await request('/admin/api/model-owners?'+new URLSearchParams({q}));
  $('model-owners').replaceChildren();for(const id of result.entries){const option=node('option');option.value=id;$('model-owners').append(option);}
}
for(const input of [$('upload-owner'),$('bulk-owner')])input.addEventListener('input',()=>{clearTimeout(ownerSearchTimer);ownerSearchTimer=setTimeout(()=>loadOwners(input.value).catch(e=>notice(e.message,true)),300);});
async function loadProfile(){
  const profile=await request('/account/api/profile');session={...session,...profile};
  $('profile-web-account').textContent=profile.account_id;$('profile-cloud-account').textContent=profile.cloud_account_id;$('profile-link-status').textContent=profile.linked?'已关联两个账号':'共用同一个 Cloud 账号';
  $('upload-owner').value=profile.cloud_account_id;
  const root=$('bound-game-identities');root.replaceChildren();const entries=Array.isArray(profile.identities)?profile.identities:profile.identities.entries||[];
  const verified=entries.filter(identity=>identity.verification_status==='VERIFIED');
  if(!verified.length)root.append(node('p','尚未绑定已验证的游戏身份。','small muted'));
  for(const identity of verified){const row=node('div',undefined,'bound-identity');row.append(icon('shield-check'),node('strong',identity.display_name),node('span',identity.identity,'small muted'));root.append(row);}
  filterIdentities();await loadAuthProviders();
}
$('refresh-profile').addEventListener('click',()=>busy($('refresh-profile'),()=>loadProfile()));
$('self-cloud-form').addEventListener('submit',e=>{e.preventDefault();busy(e.submitter,async()=>{await request('/account/api/link-cloud',{method:'POST',json:Object.fromEntries(new FormData(e.target))});e.target.reset();await loadProfile();notice('已验证并关联你自己的 Cloud 账号，管理权限保持原样。');});});
async function loadAuthProviders(){
  const result=await request('/account/api/providers');authProviders=Array.isArray(result.providers)?result.providers:result.providers.entries||[];
  for(const select of document.querySelectorAll('[data-providers], [data-thirdparty-providers]')){
    const current=select.value;select.replaceChildren();const entries=authProviders.filter(p=>!select.hasAttribute('data-thirdparty-providers')||p.provider_id!=='official').map(p=>[p.provider_id,p.provider_id==='official'?'Minecraft 正版':p.display_name]);
    options(select,entries,entries.some(([v])=>v===current)?current:entries[0]?.[0]||'');select.disabled=!entries.length;
  }
  for(const id of ['microsoft-login','microsoft-link'])$(id).disabled=!result.microsoft_configured;
  for(const note of document.querySelectorAll('.microsoft-note'))note.textContent=result.microsoft_configured?'将跳转到微软官方页面授权，并验证 Minecraft 游戏身份。':'服主尚未配置微软网页登录。可以先使用 Cloud 账号，或在模组中完成游戏身份绑定。';
}
async function microsoft(purpose){
  const result=await request('/admin/oauth/microsoft/start',{method:'POST',json:{purpose,admin:!accountPortal}});
  location.assign(result.url);
}
$('microsoft-login').addEventListener('click',()=>busy($('microsoft-login'),()=>microsoft('login')));
$('microsoft-link').addEventListener('click',()=>busy($('microsoft-link'),()=>microsoft('link')));
for(const form of document.querySelectorAll('.game-form,.game-token-form'))form.addEventListener('submit',e=>{e.preventDefault();busy(e.submitter,async()=>{
  const input=Object.fromEntries(new FormData(form));for(const key of Object.keys(input))if(input[key]==='')delete input[key];input.purpose=form.dataset.purpose;input.admin=!accountPortal;
  const result=await request('/account/api/game',{method:'POST',json:input});
  if(result.choose_profile){const label=form.querySelector('.profile-choice');label.hidden=false;const select=label.querySelector('select');select.replaceChildren();options(select,result.profiles.map(p=>[p.id,`${p.name}（${p.id}）`]),result.profiles[0]?.id);notice('该第三方账号包含多个角色，请选择角色后再次验证。');return;}
  form.reset();form.querySelector('.profile-choice')?.setAttribute('hidden','');await enter(result);
  if(input.purpose==='link'){await show('profile');notice('游戏身份已验证并绑定，模组与网页共用绑定结果。');}
});});
if(accountPortal){document.title='SPM Cloud · 账号中心';document.querySelector('#login h2').textContent='登录 Cloud 账号中心';document.querySelector('#login .login-panel>p').textContent='使用自己的 Cloud 账号登录、关联与绑定游戏身份。';document.querySelector('#login-form input[name=account_id]').placeholder='你的 Cloud 账号';$('account-portal-link').textContent='返回管理控制台';$('account-portal-link').href='/admin/';}
async function finishOAuth(){
  const params=new URLSearchParams(location.hash.slice(1));
  if(params.has('oauth')||params.has('oauth_error'))history.replaceState(null,'',location.pathname);
  if(params.has('oauth_error')){notice(params.get('oauth_message')||('微软登录未完成：'+params.get('oauth_error')+'。请检查应用配置或重新授权。'),true);return;}
  if(params.has('oauth')){const result=await request('/admin/oauth/microsoft/redeem',{method:'POST',json:{ticket:params.get('oauth')}});await enter(result);if(result.linked_identity){await show('profile');notice('微软正版身份已验证并绑定。');}}
}

function filterIdentities(){const q=$('identity-search').value.trim().toLowerCase();for(const row of $('bound-game-identities').querySelectorAll('.bound-identity'))row.hidden=!row.textContent.toLowerCase().includes(q);}
$('identity-search').addEventListener('input',filterIdentities);
async function download(path,retry=true){
  const response=await fetch(path,{headers:{Authorization:`Bearer ${session.access_token}`}});
  if(response.status===401&&retry&&session.refresh_token){await request('/account/api/profile');return download(path,false);}
  if(!response.ok){let detail;try{detail=await response.json();}catch{}throw new Error(detail?.message||`导出失败：HTTP ${response.status}`);}
  const blob=await response.blob(),url=URL.createObjectURL(blob),link=node('a');link.href=url;link.download=response.headers.get('content-disposition')?.match(/filename="([^"]+)"/)?.[1]||'spm-cloud-export';document.body.append(link);link.click();link.remove();setTimeout(()=>URL.revokeObjectURL(url),10000);notice('导出内容已生成，请在浏览器下载中核对文件。');
}
let latestRelease=null;
async function loadMaintenance(){const result=await request('/admin/api/maintenance');$('install-update').hidden=!result.native_update_supported;$('update-status').textContent=`当前版本 ${result.version}`;$('update-mode').textContent=`自动更新：${result.updates.auto_update?'已开启':'已关闭'} · 检查间隔 ${result.updates.check_interval_hours} 小时。${result.native_update_supported?'原生部署支持自动安装，更新前生成数据库与模型备份。':'当前为容器部署，请通过部署平台更新镜像。'}`;latestRelease=null;$('install-update').disabled=true;$('release-link').hidden=true;}
$('check-update').addEventListener('click',()=>busy($('check-update'),async()=>{latestRelease=await request('/admin/api/updates/check',{method:'POST'});$('update-status').textContent=latestRelease.update_available?`当前 ${latestRelease.current_version} · 发现新版本 ${latestRelease.latest_version}`:`当前 ${latestRelease.current_version} · 已是最新正式版本`;$('install-update').disabled=!latestRelease.update_available||!latestRelease.native_update_supported;const link=$('release-link');if(typeof latestRelease.release_url==='string'&&latestRelease.release_url.startsWith('https://github.com/sdf123098/spm-cloud/releases/')){link.href=latestRelease.release_url;link.hidden=false;}}));
$('install-update').addEventListener('click',()=>busy($('install-update'),async()=>{if(!await settingsUI.beforeMaintenance(true))return;if(!confirm(`安装 ${latestRelease?.latest_version} 并重启 Cloud？更新前将自动备份数据。`))return;const result=await request('/admin/api/updates/install',{method:'POST'});notice(result.message);$('update-status').textContent=result.message;}));
document.querySelectorAll('[data-export]').forEach(button=>button.addEventListener('click',()=>busy(button,async()=>{if(await settingsUI.beforeMaintenance())await download(`/admin/api/exports/${button.dataset.export}`);})));


const settingsUI=SPMSettings.mount({request,notice,getSession:()=>session,loadMaintenance,loadAudit,
 onSavingChange:value=>{$('logout').disabled=value||uploadRunning||bulkRunning;},
 downloadBackup:()=>download('/admin/api/exports/backup'),
 onBootstrapChanged:async value=>{if(session?.refresh_token)return true;if(value){const result=await request('/admin/api/login',{method:'POST',json:{bootstrap_token:value}});session=result;session={...session,...await request('/account/api/profile')};return true;}settingsUI.clear();session=null;$('console').hidden=true;$('login').hidden=false;notice('配置已保存，初始令牌已清除。请使用账号密码重新登录。');return false;},
 onDisabled:command=>{session=null;$('console').hidden=true;$('login').hidden=false;for(const section of $('login').children)section.hidden=true;const info=node('section',undefined,'webui-disabled panel');info.append(node('h2','管理网页已关闭'),node('p','在服务器执行以下命令恢复，然后刷新本页。'),node('pre',command));$('login').append(info);},
 onPasswordChanged:()=>{settingsUI.clear();session=null;$('console').hidden=true;$('login').hidden=false;}
});

loadAuthProviders().then(()=>finishOAuth()).catch(e=>notice(e.message,true));
