'use strict';
const $ = (id) => document.getElementById(id);
const accountPortal=location.pathname.startsWith('/account/');
let session = null, configResponse = null, formConfig = null, nextCursor = null, nextAccountCursor = null, noticeTimer;
let models = [], modelView = 'grid', selectedModels = new Set(), bulkRunning = false;
let uploadQueue = [], uploadRunning = false;
try { if (localStorage.getItem('spm-cloud-model-view') === 'cards') modelView = 'cards'; } catch {}
const roleNames = { owner: '服主', admin: '管理员', member: '普通账号' };
const statusNames = { R18: 'R18', CLEAN: '未检出', UNKNOWN: '未识别', DISABLED: '识别关闭' };
const tabs = { overview: '概览', models: '模型库', settings: '服务器配置', accounts: '账号与权限', profile:'我的账号与绑定', audit: '操作记录' };
const groups = { instance: '实例与网络', storage: '存储路径', auth: '账号与身份验证', limits: '资源与请求限制', features: '功能开关', retention: '保留与清理', logging: '日志', web_ui: '管理控制台', moderation: 'R18 内容策略' };
const labels = { detection_enabled:'R18 识别',block_r18:'阻止 R18 模型',texture_detection_enabled:'贴图内容识别',scanner_url:'NudeNet 服务地址',ysm_parser_path:'YSMParser 程序路径',score_threshold:'识别置信度阈值',timeout_seconds:'识别超时（秒）',max_textures:'单模型贴图上限',max_texture_bytes:'单贴图字节上限',block_unclassified:'阻止未识别模型',enabled:'启用 WebUI',allow_self_registration:'允许普通账号自行注册',bootstrap_account_id:'服主账号 ID',bootstrap_access_token_file:'服主初始令牌文件',bootstrap_password_hash_file:'服主密码哈希文件',identity_providers:'外部身份提供方',instance_id:'实例 ID',origin:'公开 HTTPS 地址',bind:'监听地址',trusted_proxy_ips:'可信代理 IP',data_dir:'数据目录',database_path:'数据库路径',object_dir:'模型文件目录',max_asset_bytes:'模型大小上限（字节）',max_message_bytes:'请求消息上限（字节）',max_entity_query_count:'实体查询数量上限',entity_motion:'实体动作',player_display_state:'玩家显示状态',vehicle_bindings:'载具绑定',projectile_snapshots:'弹射物快照（暂不可用）',level:'日志级别',format:'日志格式' };
const hints = { detection_enabled:'关闭后停止新识别；已有识别与人工标记保留。',block_r18:'拒绝 R18 上传、目录展示、下载和使用。可独立于识别开启。',texture_detection_enabled:'将内嵌贴图发送到你配置的 NudeNet 服务。',scanner_url:'例如 http://127.0.0.1:8080/infer。仅访问服主配置的服务。',block_unclassified:'拒绝识别关闭、加密、无贴图、超限或检测失败的模型。',score_threshold:'0 到 1；初始值 0.7。未检出不代表内容一定安全。',enabled:'关闭后控制台和管理 API 停止访问；需修改服务器 JSON 文件重新开启。' };
Object.assign(labels,{max_message_bytes:'请求消息大小上限',max_texture_bytes:'单张贴图大小上限',score_threshold:'裸露内容识别置信度',timeout_seconds:'识别等待上限',max_visual_state_bytes:'玩家显示状态大小上限',max_visual_variables:'玩家显示变量数量上限',max_projectile_snapshots_per_publisher:'每个发布者的弹射物快照上限',max_projectile_snapshots_per_world:'每个世界的弹射物快照上限',visual_publish_requests_per_second:'显示状态每秒发布次数',visual_publish_burst:'显示状态瞬时发布次数',projectile_idle_ttl_seconds:'弹射物闲置保留时间',projectile_max_lifetime_seconds:'弹射物最长保留时间',cleanup_interval_seconds:'过期显示数据检查间隔'});
Object.assign(hints,{instance_id:'用于区分不同 Cloud 实例。请保留稳定的英文、数字或短横线标识；修改需重启。',origin:'玩家连接这台 Cloud 使用的 HTTPS 域名，例如 https://cloud.example.com，不包含路径。修改需重启。',bind:'Cloud 进程监听的 IP 与端口；通常让反向代理连接这里。修改需重启。',trusted_proxy_ips:'每行一个可信反向代理 IP；只有这些代理可以转发客户端 IP。',data_dir:'服务器上的数据目录。修改前请迁移原有数据库与模型文件，修改需重启。',database_path:'服务器上的 SQLite 数据库路径；留空使用数据目录内的默认位置。修改需重启。',object_dir:'服务器上的模型文件目录；留空使用数据目录内的默认位置。修改需重启。',bootstrap_account_id:'初始服主账号，不能在控制台中更名。管理员账号由服主在“账号与权限”创建。',bootstrap_access_token_file:'服务器上的初始令牌文件路径；这里只配置路径，不显示令牌。修改需重启。',bootstrap_password_hash_file:'服务器上的服主密码哈希文件路径；留空不使用该文件。修改需重启。',identity_providers:'外置 Yggdrasil 认证服务列表；仅在需要接入外置登录时配置。',ysm_parser_path:'服务器上的 YSMParser 可执行文件路径，用于读取受支持的加密 YSM 贴图；程序需单独安装。',timeout_seconds:'每个模型等待贴图识别的最长时间；超时后标记为未识别。',max_textures:'单个模型最多识别的贴图数量；超过上限标记为未识别。',max_texture_bytes:'单张贴图的读取大小上限；超过上限标记为未识别。',max_message_bytes:'客户端单条协议消息大小上限。一般保留默认值。',max_entity_query_count:'客户端一次最多查询的实体数量。一般保留默认值。',max_visual_state_bytes:'单次玩家显示状态的数据大小上限。',max_visual_variables:'单个玩家显示状态可包含的变量数量。',max_projectile_snapshots_per_publisher:'预留的弹射物快照限制；当前版本该功能不可开启。',max_projectile_snapshots_per_world:'预留的弹射物快照限制；当前版本该功能不可开启。',visual_publish_requests_per_second:'限制客户端持续发布显示状态的频率。',visual_publish_burst:'允许客户端短时间内突发发布的次数。',entity_motion:'允许支持的客户端同步已绑定女仆与假玩家的显示动作。',player_display_state:'允许支持的客户端同步玩家显示状态。',vehicle_bindings:'允许支持的客户端同步已绑定载具的显示信息。',projectile_snapshots:'当前版本暂未实现此功能，保持关闭。',projectile_idle_ttl_seconds:'预留的弹射物快照闲置时限；当前版本不启用弹射物快照。',projectile_max_lifetime_seconds:'预留的弹射物快照最长时限；需大于闲置保留时间。',cleanup_interval_seconds:'定期检查过期的临时显示状态；不会删除模型库文件。一般保持 60 秒。',level:'日常使用“常规”；排查问题时可临时使用“调试”。修改需重启。',format:'文本便于人工阅读，JSON 便于日志收集工具处理。修改需重启。'});
const settingSections=[
  {title:'模型与内容安全',description:'控制模型上传大小，以及哪些内容允许玩家查看和使用。',fields:[
    ['limits','max_asset_bytes','单个模型最大大小','超过此大小的文件无法上传。默认 128 MB；远程部署时代理也需允许相同大小。',1048576,'MB'],
    ['moderation','detection_enabled','识别 R18 内容'],['moderation','block_r18','阻止 R18 内容'],
    ['moderation','block_unclassified','拒绝未能完成识别的模型'],['moderation','texture_detection_enabled','识别模型贴图中的裸露内容','需要先在下方高级设置中连接识别服务；目前支持自建 NudeNet。'],
  ]},
  {title:'账号与权限',description:'管理员由服主创建并分发；普通玩家注册不会获得管理权限。',fields:[['auth','allow_self_registration','允许玩家自行注册普通账号','关闭后，普通账号也需要由服主创建。管理员始终由服主在“账号与权限”分发。']]},
  {title:'服务访问',description:'填写玩家实际连接的地址。通常只需在首次部署时设置。',fields:[['instance','origin','玩家连接地址']]},
  {title:'维护与清理',description:'调整临时数据清理与日志记录，便于日常维护。',fields:[['retention','cleanup_interval_seconds','临时显示数据清理间隔',null,1,'秒'],['logging','level','日志详细程度']]},
];
const settingUnits={max_message_bytes:[1024,'KB'],max_visual_state_bytes:[1024,'KB'],max_texture_bytes:[1048576,'MB'],timeout_seconds:[1,'秒'],projectile_idle_ttl_seconds:[1,'秒'],projectile_max_lifetime_seconds:[1,'秒'],score_threshold:[.01,'%']};
function notice(message,error=false){ $('notice').textContent=message;$('notice').className=error?'error':'';$('notice').hidden=false;clearTimeout(noticeTimer);noticeTimer=setTimeout(()=>{$('notice').hidden=true;},error?12000:7000); }
async function request(path,options={},retry=true){
  const headers=new Headers(options.headers||{});if(session)headers.set('Authorization',`Bearer ${session.access_token}`);
  if(options.json!==undefined){headers.set('Content-Type','application/json');options.body=JSON.stringify(options.json);}
  const response=await fetch(path,{...options,headers});
  const credentialProof=path==='/account/api/link-cloud'||path==='/account/api/game';
  if(response.status===401&&!credentialProof&&retry&&session?.refresh_token){
    const refreshed=await fetch('/v1/sessions/refresh',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({refresh_token:session.refresh_token})});
    if(refreshed.ok){session={...session,...await refreshed.json()};return request(path,options,false);}
  }
  if(!response.ok){let detail;try{detail=await response.json();}catch{detail={message:`HTTP ${response.status}`};}
    if(response.status===401&&!credentialProof&&session){session=null;$('console').hidden=true;$('login').hidden=false;}
    const messages={R18_MODEL_BLOCKED:'该模型被识别或标记为 R18，已按内容策略阻止上传。',MODEL_UNCLASSIFIED:'该模型未能完成内容识别，已按“拒绝未能完成识别的模型”策略阻止上传。',ASSET_TOO_LARGE:'文件超过服务器允许的模型大小，请调整大小上限或选择较小文件。',IDENTITY_NOT_LINKED:'该游戏身份尚未绑定 Cloud 账号，请先用 Cloud 账号登录并完成绑定。',IDENTITY_ALREADY_LINKED:'该身份或账号已有不同的关联，请检查当前绑定，不能覆盖他人的关联。',IDENTITY_PROFILE_MISMATCH:'身份验证失败，请检查账号、令牌或微软应用授权。',IDENTITY_PROVIDER_UNAVAILABLE:'身份服务暂时无法访问，请稍后重试。',IDENTITY_PROVIDER_UNTRUSTED:'该身份服务尚未启用，或服务地址不符合要求。',ASSET_ACCESS_DENIED:'当前账号没有执行此操作的权限。',UNAUTHENTICATED:'账号或密码不正确，或当前登录已失效。',IDENTITY_CHALLENGE_EXPIRED:'验证已过期，请重新开始。',RATE_LIMITED:'操作过于频繁，请稍后再试。'};
    throw new Error(messages[detail.code]||detail.message||detail.code||`HTTP ${response.status}`);
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
function bytes(n){return `${(n/1024/1024).toFixed(2)} MB`;}
async function show(tab){if(tab==='accounts'&&session?.role!=='owner')return;
  if(accountPortal&&tab!=='profile')return;
  for(const name of Object.keys(tabs))$(`tab-${name}`).hidden=name!==tab;
  for(const button of document.querySelectorAll('[data-tab]'))button.classList.toggle('active',button.dataset.tab===tab);
  $('breadcrumb').textContent=tabs[tab];
  document.querySelector('.content').scrollTop=0;
  try{if(tab==='overview')await loadOverview();if(tab==='models'){await loadModels();await loadOwners();}if(tab==='settings')await loadSettings();if(tab==='accounts')await loadAccounts();if(tab==='audit')await loadAudit();if(tab==='profile')await loadProfile();}catch(e){notice(e.message,true);}
}
async function loadOverview(){const result=await request('/admin/api/config');const effective=result.effective?.effective||result.effective;const moderation=effective.moderation||{};for(const [id,key]of [['detection','detection_enabled'],['block','block_r18'],['texture','texture_detection_enabled']])$(`overview-${id}`).textContent=moderation[key]===true?'已开启':moderation[key]===false?'已关闭':'未配置';$('overview-instance').textContent=effective.instance?.instance_id||'—';$('overview-origin').textContent=effective.instance?.origin||location.origin;$('overview-role').textContent=roleNames[session.role];}
async function enter(result){session=result;const me=await request('/account/api/profile');session={...session,...me};
  $('login').hidden=true;$('console').hidden=false;$('identity').textContent=me.account_id;$('role-badge').textContent=roleNames[me.role];
  document.querySelectorAll('[data-owner]').forEach(el=>el.hidden=accountPortal||me.role!=='owner');
  document.querySelectorAll('nav [data-tab]').forEach(el=>{if(accountPortal)el.hidden=el.dataset.tab!=='profile';});
  $('upload-owner').value=me.cloud_account_id;
  await show(accountPortal?'profile':'overview');
}
for(const id of ['login-form','token-form'])$(id).addEventListener('submit',e=>{e.preventDefault();busy(e.submitter,async()=>{const data=Object.fromEntries(new FormData(e.target));const result=await request(accountPortal&&id==='login-form'?'/account/api/login':'/admin/api/login',{method:'POST',json:data});e.target.reset();await enter(result);});});
$('logout').addEventListener('click',()=>busy($('logout'),async()=>{if(session?.refresh_token)try{await request('/v1/sessions/current',{method:'DELETE'});}catch{}session=null;configResponse=null;formConfig=null;models=[];selectedModels.clear();uploadQueue=[];$('upload-form').reset();renderUploads();$('upload-status').textContent='';$('model-cards').replaceChildren();syncSelection();$('model-detail').close();$('raw-config').value='';$('config-fields').replaceChildren();$('console').hidden=true;$('login').hidden=false;}));
document.querySelectorAll('[data-tab]').forEach(b=>b.addEventListener('click',()=>show(b.dataset.tab)));
document.querySelectorAll('[data-go]').forEach(b=>b.addEventListener('click',()=>show(b.dataset.go)));
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
  }finally{uploadRunning=false;$('logout').disabled=bulkRunning;for(const control of form.querySelectorAll('input,select,button'))control.disabled=false;renderUploads();}
});
$('search-form').addEventListener('submit',e=>{e.preventDefault();busy(e.submitter,()=>loadModels());});$('include-deleted').addEventListener('change',()=>loadModels().catch(e=>notice(e.message,true)));$('refresh-models').addEventListener('click',()=>busy($('refresh-models'),()=>loadModels()));$('next-models').addEventListener('click',()=>busy($('next-models'),()=>loadModels(true)));
function setModelView(view){
  modelView=view==='cards'?'cards':'grid';
  $('model-cards').dataset.view=modelView;
  document.querySelectorAll('button[data-view]').forEach(button=>button.setAttribute('aria-pressed',String(button.dataset.view===modelView)));
  $('model-cards').querySelectorAll('.model-card').forEach(card=>card.style.removeProperty('transform'));
  try{localStorage.setItem('spm-cloud-model-view',modelView);}catch{}
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

async function loadModels(append=false){
  const params=new URLSearchParams({q:new FormData($('search-form')).get('q'),deleted:$('include-deleted').checked});
  if(append&&nextCursor)params.set('after',nextCursor);
  const result=await request(`/admin/api/assets?${params}`);
  if(!append)models=[];
  const seen=new Set(models.map(model=>model.asset_id));
  for(const model of result.entries)if(!seen.has(model.asset_id)){models.push(model);seen.add(model.asset_id);}
  nextCursor=result.next_cursor;
  renderModels();
  $('next-models').hidden=!result.has_more;
  $('model-count').textContent=`已加载 ${models.length} 个模型${result.has_more?' · 还有更多':''}`;
}

function syncSelection(){
  const visible=new Set(models.map(model=>model.asset_id));
  selectedModels=new Set([...selectedModels].filter(id=>visible.has(id)));
  const count=selectedModels.size, all=$('select-all-models');
  all.checked=models.length>0&&count===models.length;all.indeterminate=count>0&&count<models.length;all.disabled=bulkRunning||models.length===0;
  $('selection-toolbar').hidden=count===0;$('selection-count').textContent=`已选 ${count} 个`;
  for(const el of document.querySelectorAll('#selection-toolbar button, #selection-toolbar select, #selection-toolbar input'))el.disabled=bulkRunning;
  $('logout').disabled=bulkRunning||uploadRunning;
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
    visual.append(pick,icon('box'),node('span',(model.format||'MODEL').toUpperCase(),'format-tag'));
    const body=node('div',undefined,'model-body'), title=action(model.name,()=>showModelDetail(model));title.className='model-name';title.title=`查看 ${model.name} 的详情`;body.append(title);
    const status=node('div',undefined,'model-status'), known=Object.hasOwn(statusNames,model.moderation)?model.moderation:'UNKNOWN';
    status.append(node('span',statusNames[known],`badge status-${known}`));if(model.deleted)status.append(node('span','已删除','badge deleted-label'));
    else{const visibility=node('span',undefined,'visibility');visibility.append(icon(model.visibility==='PUBLIC'?'globe':'lock-keyhole'));visibility.title=model.visibility==='PUBLIC'?'公开':'私有';visibility.setAttribute('aria-label',visibility.title);status.append(visibility);}
    const meta=node('div',undefined,'model-meta'), owner=node('span',model.owner_account_id,'model-owner');owner.title=`所有者：${model.owner_account_id}`;meta.append(owner,node('span',bytes(model.byte_length)));body.append(status,meta);
    card.append(visual,body);slot.append(card);root.append(slot);
  }
  $('model-empty').hidden=models.length>0;syncSelection();
}

function showModelDetail(model){
  const root=$('model-detail-content');root.replaceChildren();$('model-detail-title').textContent=model.name;
  const metadata=node('dl');for(const [label,value]of [['模型 ID',model.asset_id],['所有者',model.owner_account_id],['格式',(model.format||'').toUpperCase()],['大小',bytes(model.byte_length)],['版本',`v${model.revision}`],['可见性',model.visibility==='PUBLIC'?'公开':'私有'],['内容识别',statusNames[model.moderation]||model.moderation],['人工标记',model.manual_status?(statusNames[model.manual_status]||model.manual_status):'无'],['状态',model.deleted?'已删除':'正常']]){const row=node('div');row.append(node('dt',label),node('dd',value));metadata.append(row);}root.append(metadata,node('p',model.reason||'尚无识别结果','detail-reason'));
  if(model.scan?.textures_scanned)root.append(node('p',`已检测 ${model.scan.textures_scanned} 张贴图`,'small muted'));
  const actions=node('div',undefined,'model-actions'), path=`/admin/api/assets/${encodeURIComponent(model.asset_id)}`;
  const update=async(action)=>{await action();$('model-detail').close();await loadModels();};
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

function defaults(schema){if(schema.type==='object'){const value={};for(const [key,s]of Object.entries(schema.properties||{})){const v=defaults(s);if(v!==undefined)value[key]=v;}return value;}if('default'in schema)return structuredClone(schema.default);return undefined;}
function merge(base,value){for(const [key,v]of Object.entries(value||{})){if(v&&typeof v==='object'&&!Array.isArray(v))base[key]=merge(base[key]||{},v);else base[key]=v;}return base;}
async function loadSettings(){
  configResponse=await request('/admin/api/config');
  $('settings-note').textContent=configResponse.writable?(session.role==='owner'?'保存后，内容策略和上传限制立即生效；标注需重启的设置将在重启后生效。':'当前为管理员权限，可以查看设置；只有服主能够修改。'):'当前由服务器启动环境控制设置，只能查看。使用配置文件启动服务后，服主可以在此保存修改。';
  $('save-settings').disabled=!configResponse.writable||session.role!=='owner';$('apply-raw').disabled=$('save-settings').disabled;$('raw-config').readOnly=$('save-settings').disabled;
  const effective=configResponse.effective?.effective||configResponse.effective;
  formConfig=merge(defaults(configResponse.schema),configResponse.config||effective);renderConfig();
}
function configField(group,key,title,description,scale=1,unit=''){
  const field=configResponse.schema.properties[group].properties[key],value=formConfig[group]?.[key];
  const label=node('label',undefined,field.type==='boolean'?'check':'setting-field');label.append(node('span',title||labels[key]||key));let input;
  const nullable=Array.isArray(field.type)&&field.type.includes('null');
  if(field.type==='boolean'){input=node('input');input.type='checkbox';input.checked=Boolean(value);}
  else if(key==='trusted_proxy_ips'){input=node('textarea');input.value=(value||[]).join('\n');input.rows=3;}
  else if(field.type==='array'){input=node('textarea');input.value=JSON.stringify(value||[],null,2);}
  else if(field.enum){input=node('select');options(input,field.enum.map(v=>[v,v==='text'?'文本（便于阅读）':v==='json'?'JSON（日志收集）':v]),value);}
  else if(key==='level'){
    input=node('select');const entries=[['info','常规：运行信息与错误（推荐）'],['warn','精简：警告与错误'],['error','仅记录错误'],['debug','调试：记录更多排查信息'],['trace','追踪：非常详细']];
    if(!entries.some(([v])=>v===value))entries.push([value,`自定义过滤规则：${value}`]);options(input,entries,value);
  }else{input=node('input');input.type=field.type==='integer'||field.type==='number'?'number':'text';input.value=value===null||value===undefined?'':input.type==='number'?Number((value/scale).toPrecision(15)):value;
    if(field.minimum!==undefined)input.min=field.minimum/scale;if(field.maximum!==undefined)input.max=field.maximum/scale;if(input.type==='number')input.step='any';
    if(field.pattern)input.pattern=field.pattern.replace('._-', '._\\-');if(field.maxLength)input.maxLength=field.maxLength;
  }
  input.dataset.group=group;input.dataset.key=key;input.dataset.scale=scale;input.dataset.label=title||labels[key]||key;
  input.dataset.kind=key==='trusted_proxy_ips'?'lines':field.type==='array'?'array':field.type==='boolean'?'boolean':input.type==='number'?'number':nullable||field.default===null?'nullable':'text';
  input.disabled=session.role!=='owner'||!configResponse.writable||key==='bootstrap_account_id'||key==='projectile_snapshots';
  if(unit){const wrap=node('div',undefined,'setting-unit');wrap.append(input,node('span',unit));label.append(wrap);}else label.append(input);
  label.append(node('small',description||hints[key]||'一般保留默认设置。'));
  if(key==='score_threshold')label.lastChild.textContent='初始值 70%；达到此置信度的裸露内容会标记为 R18。二次元贴图可能误判，建议人工复核。';
  return label;
}
function renderConfig(){
  const root=$('config-fields');root.replaceChildren();const used=new Set();
  for(const spec of settingSections){const section=node('section',undefined,'panel config-section everyday-settings');section.append(node('h2',spec.title),node('p',spec.description,'setting-description'));const grid=node('div',undefined,'config-grid');
    for(const [group,key,title,description,scale,unit]of spec.fields){used.add(`${group}.${key}`);grid.append(configField(group,key,title,description,scale,unit));}
    section.append(grid);
    if(spec.title==='账号与权限'&&session.role==='owner')section.append(action('管理与分发账号',()=>show('accounts'),'users'));
    root.append(section);
  }
  const advanced=node('details',undefined,'panel advanced-settings');advanced.id='advanced-settings';advanced.append(node('summary','高级设置 · 识别服务连接、客户端功能、存储与网络'),node('p','首次接入贴图识别时，请配置 NudeNet 服务地址与 YSMParser 路径。其他参数通常保留默认值。','setting-description'));
  for(const group of ['moderation','features','instance','storage','auth','limits','retention','logging','web_ui']){
    const fields=Object.keys(configResponse.schema.properties[group].properties).filter(key=>!used.has(`${group}.${key}`));if(!fields.length)continue;
    const section=node('section',undefined,'advanced-group');section.append(node('h3',group==='moderation'?'贴图识别服务（NudeNet）':groups[group]));const grid=node('div',undefined,'config-grid');
    for(const key of fields){const [scale,unit]=settingUnits[key]||[1,''];grid.append(configField(group,key,null,null,scale,unit));}section.append(grid);advanced.append(section);
  }
  root.append(advanced);$('raw-config').value=JSON.stringify(formConfig,null,2);
}
function readConfig(){
  const copy=structuredClone(formConfig);
  for(const input of document.querySelectorAll('#config-fields [data-group]')){
    const title=input.dataset.label;let value=input.value;
    switch(input.dataset.kind){
      case 'boolean':value=input.checked;break;
      case 'number':{
        if(!input.validity.valid||value==='')throw new Error(`请检查“${title}”的数值范围`);
        const scale=Number(input.dataset.scale),original=copy[input.dataset.group][input.dataset.key];
        value=Number(value)===Number((original/scale).toPrecision(15))?original:Number((Number(value)*scale).toPrecision(15));
        const field=configResponse.schema.properties[input.dataset.group].properties[input.dataset.key];
        if(field.type==='integer'){const rounded=Math.round(value);if(Math.abs(value-rounded)>1e-6)throw new Error(`“${title}”换算后必须为整数`);value=rounded;}
        break;
      }
      case 'array':try{value=JSON.parse(value);if(!Array.isArray(value))throw new Error();}catch{throw new Error(`“${title}”需要填写有效的 JSON 数组`);}break;
      case 'lines':value=value.split(/\r?\n/).map(line=>line.trim()).filter(Boolean);break;
      case 'nullable':if(value==='')value=null;break;
    }
    if(!input.validity.valid)throw new Error(`请检查“${title}”的格式`);
    copy[input.dataset.group][input.dataset.key]=value;
  }
  return copy;
}
$('reload-settings').addEventListener('click',()=>busy($('reload-settings'),()=>loadSettings()));$('apply-raw').addEventListener('click',()=>{try{const parsed=JSON.parse($('raw-config').value);if(!parsed||Array.isArray(parsed)||typeof parsed!=='object')throw new Error('完整配置必须是 JSON 对象');formConfig=merge(defaults(configResponse.schema),parsed);renderConfig();notice('JSON 已应用到表单，点击保存后生效。');}catch(e){notice(e.message,true);}});
$('settings-form').addEventListener('submit',e=>{e.preventDefault();busy(e.submitter,async()=>{const config=readConfig();if(!config.web_ui.enabled&&!confirm('关闭 WebUI 后需在服务器上修改 JSON 才能重新开启。继续保存？'))return;await request('/admin/api/config',{method:'PUT',json:{config,revision:configResponse.revision}});notice('配置已保存。运行时设置已生效；监听地址、存储、凭据和日志更改需重启。');if(!config.web_ui.enabled){session=null;$('console').hidden=true;$('login').hidden=false;return;}await loadSettings();});});
$('account-form').addEventListener('submit',e=>{e.preventDefault();busy(e.submitter,async()=>{await request('/admin/api/accounts',{method:'POST',json:Object.fromEntries(new FormData(e.target))});e.target.reset();notice('账号已创建，请自行分发账号和初始密码。');await loadAccounts();});});
async function loadAccounts(append=false){const params=new URLSearchParams({q:new FormData($('account-search-form')).get('q')});if(append&&nextAccountCursor)params.set('after',nextAccountCursor);const result=await request(`/admin/api/accounts?${params}`);if(!append)$('account-rows').replaceChildren();for(const account of result.entries){const row=node('tr');cell(row,account.account_id);cell(row,roleNames[account.role]);cell(row,account.disabled?'已停用':'正常');cell(row,account.created_at);const actions=cell(row);if(account.role!=='owner'){const path=`/admin/api/accounts/${encodeURIComponent(account.account_id)}`;const change=async(body)=>{await request(path,{method:'PUT',json:{role:account.role,disabled:account.disabled,...body}});await loadAccounts();};actions.append(action(account.disabled?'启用':'停用',()=>change({disabled:!account.disabled})),action(account.role==='admin'?'撤销管理权':'授权管理员',()=>change({role:account.role==='admin'?'member':'admin'})),action('重置密码',async()=>{const password=prompt('输入新密码（至少 8 个字符）。已有会话将失效。');if(password!==null)await change({password});}));}else{actions.append(action('设置服主密码',async()=>{const password=prompt('为服主设置登录密码（至少 8 个字符）。已有密码会话将失效，初始令牌仍可使用。');if(password===null)return;await request(`/admin/api/accounts/${encodeURIComponent(account.account_id)}`,{method:'PUT',json:{role:'owner',disabled:false,password}});notice('服主密码已设置，可使用账号和密码登录。');}));}$('account-rows').append(row);}nextAccountCursor=result.next_cursor;$('next-accounts').hidden=!result.has_more;}
async function loadAudit(){const result=await request('/admin/api/audit');$('audit-rows').replaceChildren();for(const event of result.entries){const row=node('tr');cell(row,event.created_at);cell(row,event.actor);cell(row,event.action);const subject=cell(row,event.subject||'—');subject.title=JSON.stringify(event.details);$('audit-rows').append(row);}}
$('refresh-audit').addEventListener('click',()=>busy($('refresh-audit'),()=>loadAudit()));

$('account-search-form').addEventListener('submit',e=>{e.preventDefault();busy(e.submitter,()=>loadAccounts());});$('refresh-accounts').addEventListener('click',()=>busy($('refresh-accounts'),()=>loadAccounts()));$('next-accounts').addEventListener('click',()=>busy($('next-accounts'),()=>loadAccounts(true)));

Object.assign(labels,{microsoft_client_id:'微软登录应用 Client ID',microsoft_client_secret_file:'微软应用密钥文件路径',microsoft_redirect_uri:'微软登录回调地址'});
Object.assign(hints,{microsoft_client_id:'在 Microsoft Entra 注册支持个人 Microsoft 账号的 Web 应用后，填写应用程序（客户端）ID。',microsoft_client_secret_file:'服务器上保存客户端密码“值”的文本文件路径，密钥不会发送到浏览器。相对路径以 Cloud 配置文件目录为准。',microsoft_redirect_uri:'与微软应用登记的 Web 回调地址完全一致，以 /admin/oauth/microsoft/callback 结尾；本机可使用 http://localhost:18822/admin/oauth/microsoft/callback。'});
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
  await loadAuthProviders();
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
loadAuthProviders().then(()=>finishOAuth()).catch(e=>notice(e.message,true));
