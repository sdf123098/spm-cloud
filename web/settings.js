(function(root){
'use strict';
const tabs=[{id:'general',title:'基础与网络'},{id:'auth',title:'账号与认证'},{id:'models',title:'模型与内容安全'},{id:'performance',title:'性能与缓存'},{id:'storage',title:'存储与数据库'},{id:'client',title:'客户端功能'},{id:'logging',title:'日志'},{id:'updates',title:'更新与备份'},{id:'audit',title:'操作记录'}];
const registry={};
register('client','offline_identity',[
 ['authorization_mode','离线同步授权','客户端登录后自动申请。关闭授权会使已有发布会话失效；手动授权等待管理员批准；默认自动授权。','select',1,'',[['disabled','关闭授权'],['manual','手动授权'],['automatic','自动授权']]],
 ['max_identities_per_account','每账号离线身份上限','同一 Cloud 账号可以创建的独立离线身份数量。','number'],
 ['session_ttl_seconds','离线发布会话有效期','有效发布会续期；到期后停止向观察者提供外观。','number',1,'秒'],
 ['activation_requests_per_minute','每分钟离线激活上限','限制同一账号创建离线发布会话的次数。','number'],
 ['issuance_requests_per_hour','每小时设备注册上限','同一账号注册离线设备的次数限制。','number']]);
function register(tab,group,entries){for(const [key,title,help,control='text',scale=1,unit='',choices]of entries)registry[`${group}.${key}`]={tab,title,help,control,scale,unit,choices};}
register('general','instance',[
 ['instance_id','实例标识','用于区分 Cloud 实例。修改后重启服务生效。'],
 ['origin','玩家连接地址','填写不含路径的公开 HTTPS 地址，例如 https://cloud.example.com。修改后重启生效。'],
 ['bind','监听方式与端口','反向代理通常连接本机监听地址；对外监听前请配置防火墙。修改后重启生效。','bind'],
 ['trusted_proxy_ips','可信反向代理','每行一个 IP；仅这些代理可以转发真实客户端 IP。','lines']]);
register('general','web_ui',[['enabled','开放管理网页','关闭后网页和管理 API 停止访问。保存前会提供该实例的恢复命令。','boolean']]);
register('storage','storage',[
 ['backend','数据库类型','内置 SQLite 适合单机。外部 MySQL 需先检查连接、备份，再保存并重启。','select',1,'',[['sqlite','内置 SQLite'],['mysql','外部 MySQL']]],
 ['data_dir','数据目录','服务器上的数据根目录；相对路径以配置文件目录为基准。改路径不会自动搬迁文件。'],
 ['database_path','SQLite 文件位置','留空使用数据目录内的 spm-cloud.db。更改位置需要先安排数据迁移。','nullable'],
 ['object_dir','模型文件目录','留空使用数据目录内的 objects。目录不变时保留原模型文件。','nullable'],
 ['mysql_url_file','MySQL 连接与凭据','分项填写连接信息，由服务器保存凭据；也可兼容已有凭据文件。','secret']]);
register('auth','auth',[
 ['allow_self_registration','允许玩家自行注册','新注册账号没有管理权限；管理员仍由服主创建。','boolean'],
 ['bootstrap_account_id','固定服主账号','此实例的初始服主标识，仅展示；不通过网页更换。','readonly'],
 ['bootstrap_access_token_file','服主初始令牌','可以保留、替换或生成新令牌。新令牌仅在保存成功时显示一次。','secret'],
 ['bootstrap_password_hash_file','首次初始化密码摘要','仅用于首次创建服主凭据，不能用它给已有账号改密码。实际密码使用下方“更改服主密码”。','secret'],
 ['identity_providers','第三方认证服务','添加、检查、编辑、停用和恢复服务；改名保留身份标识及历史绑定。','providers']]);
register('auth','web_ui',[
 ['microsoft_client_id','微软登录应用标识','填写 Microsoft Entra 应用程序的客户端 UUID，应用应支持个人微软账号。','nullable'],
 ['microsoft_client_secret_file','微软应用密钥','填写客户端密码的值，由服务器保存；旧密钥不会读回浏览器。','secret'],
 ['microsoft_redirect_uri','微软登录回调地址','应与应用登记值完全一致，以 /admin/oauth/microsoft/callback 结尾。留空使用公开地址推导。','nullable']]);
register('models','limits',[['max_asset_bytes','单个模型大小上限','超过上限的文件不能上传；反向代理上传上限也应足够大。','number',1048576,'MiB']]);
register('models','moderation',[
 ['detection_enabled','识别模型内容','关闭后停止新识别，已有结果和人工标记继续保留。','boolean'],
 ['block_r18','阻止成人内容','拒绝已识别或人工标记为 R18 的模型上传、展示、下载和使用。','boolean'],
 ['block_unclassified','拒绝未识别模型','识别关闭、超时、加密或预算不足的模型也会被阻止，请谨慎开启。','boolean'],
 ['texture_detection_enabled','识别模型贴图','需要开启内容识别并连接 NudeNet 服务；只发送模型内嵌贴图。','boolean'],
 ['scanner_url','贴图识别服务地址','支持现有内网 HTTP(S) 部署，例如 http://127.0.0.1:8080/infer。','nullable'],
 ['score_threshold','裸露内容识别置信度','达到此阈值的内容标为 R18；可能误判，建议配合人工复核。','number',0.01,'%'],
 ['timeout_seconds','识别等待上限','超时后标记为未识别，不代表模型安全。','number',1,'秒'],
 ['max_textures','单模型贴图预算','最多识别的贴图数量；超过预算会标记为未识别。','number',1,'张'],
 ['max_texture_bytes','单张贴图大小预算','限制每张贴图读取大小，超过预算会标记为未识别。','number',1048576,'MiB'],
 ['ysm_parser_path','加密模型解析程序','服务器上 YSMParser 的可执行文件路径，程序需要单独安装。','nullable']]);
register('performance','performance',[
 ['preset','服务器性能预设','低配：上传 2 / 处理 1；中配：4 / 2；高配：8 / 4；自定义可分别设置。','select',1,'',[['low','低配 · 2 / 1'],['medium','中配 · 4 / 2'],['high','高配 · 8 / 4'],['custom','自定义']]],
 ['upload_concurrency','同时上传数量','自定义模式下可编辑；0 表示不限制。降低并发不会中断现有任务。','number',1,'个'],
 ['model_processing_concurrency','扫描与解码并发','模型扫描、解码和预览共同使用此限制；0 表示不限制。','number',1,'个']]);
register('performance','cache',[
 ['backend','目录缓存方式','内存缓存无需额外服务；Redis 故障时回落到有界内存；关闭后直接查询数据库。','select',1,'',[['memory','内存缓存'],['redis','Redis 缓存'],['none','关闭缓存']]],
 ['redis_url_file','Redis 连接与凭据','支持结构化连接、密码与加密连接，也可使用服务器已有凭据文件。','secret'],
 ['ttl_seconds','缓存有效时间','过期后重新查询数据库。切换模式不会清空其他模式的设置。','number',1,'秒'],
 ['max_pages','最多缓存页数','限制本地缓存内存占用，权限校验始终由数据库执行。','number',1,'页'],
 ['timeout_ms','Redis 等待上限','Redis 响应超时后使用有界本地缓存。','number',1,'毫秒']]);
register('client','features',[
 ['entity_motion','实体显示动作','允许支持的客户端同步已绑定女仆与假玩家的显示动作。','boolean'],
 ['player_display_state','玩家显示状态','允许支持的客户端同步玩家的显示状态和变量。','boolean'],
 ['vehicle_bindings','载具模型绑定','允许支持的客户端同步已绑定载具的显示信息。','boolean'],
 ['projectile_snapshots','弹射物快照','当前版本尚未实现，不能启用。','boolean']]);
register('client','limits',[
 ['max_message_bytes','单条协议消息上限','限制客户端协议消息大小，应大于显示状态上限。','number',1024,'KiB'],
 ['max_entity_query_count','单次实体查询数量','限制一次查询的实体数量，避免请求过大。','number',1,'个'],
 ['max_visual_state_bytes','显示状态大小上限','单次玩家显示状态的数据预算，不能超过协议消息上限。','number',1024,'KiB'],
 ['max_visual_variables','显示变量数量上限','限制单个显示状态携带的变量数量。','number',1,'项'],
 ['max_projectile_snapshots_per_publisher','每个发布者的弹射物预算','预留参数，当前弹射物功能不可用。','number',1,'个'],
 ['max_projectile_snapshots_per_world','每个世界的弹射物预算','预留参数，当前弹射物功能不可用。','number',1,'个'],
 ['visual_publish_requests_per_second','显示状态持续发布频率','限制客户端持续发布的速率。','number',1,'次/秒'],
 ['visual_publish_burst','显示状态突发预算','允许短时间内突发发布，应不小于持续频率。','number',1,'次']]);
register('client','retention',[
 ['projectile_idle_ttl_seconds','弹射物闲置保留时间','预留参数，当前弹射物功能不可用。','number',1,'秒'],
 ['projectile_max_lifetime_seconds','弹射物最长保留时间','预留参数，当前弹射物功能不可用。','number',1,'秒'],
 ['cleanup_interval_seconds','临时显示数据清理间隔','检查并清理过期显示状态，不会删除模型文件。','number',1,'秒']]);
register('logging','logging',[
 ['level','日志详细程度与模块过滤','可选择常规等级，或逐行设置模块过滤；已有合法表达式未编辑时会完整保留。修改需重启。','logfilter'],
 ['format','日志输出格式','文本适合人工查看，结构化日志适合采集工具。修改需重启。','select',1,'',[['text','易读文本'],['json','结构化日志（JSON）']]]]);
register('updates','updates',[
 ['auto_update','自动检查并安装更新','默认关闭。原生部署校验并备份后安装；容器由部署平台更新。','boolean'],
 ['check_interval_hours','更新检查间隔','自动更新开启后按此间隔检查正式版本。','number',1,'小时']]);
const providerFields=['provider_id','display_name','base_url','enabled','session_path','has_joined_url','authenticate_path'];
const secretNames={'storage.mysql_url_file':'mysql','cache.redis_url_file':'redis','web_ui.microsoft_client_secret_file':'microsoft','auth.bootstrap_access_token_file':'bootstrap_token','auth.bootstrap_password_hash_file':'bootstrap_password_hash'};
const clone=value=>value===undefined?undefined:JSON.parse(JSON.stringify(value));
function get(value,path){return path.split('.').reduce((v,k)=>v?.[k],value);}
function set(value,path,replacement){const keys=path.split('.');let part=value;for(const key of keys.slice(0,-1))part=part[key]||(part[key]={});part[keys.at(-1)]=clone(replacement);}
function defaults(schema){if(schema.type==='object')return Object.fromEntries(Object.entries(schema.properties||{}).map(([k,s])=>[k,defaults(s)]));return clone(schema.default);}
function merge(base,value){for(const [key,v]of Object.entries(value||{}))base[key]=v&&typeof v==='object'&&!Array.isArray(v)?merge(base[key]||{},v):clone(v);return base;}
function fieldSchema(schema,path){return path.split('.').reduce((s,k)=>s?.properties?.[k],schema);}
function toDisplay(path,value){if(value===null||value===undefined)return '';return registry[path].control==='number'?String(value/registry[path].scale):String(value);}
function fromDisplay(path,value,schema){const meta=registry[path],field=fieldSchema(schema,path);if(meta.control==='number'){if(String(value).trim()==='')throw Error('请输入数值');let result=meta.scale===0.01?Number(value)/100:Number(value)*meta.scale;if(!Number.isFinite(result))throw Error('请输入有效数值');if(field.type==='integer'){const rounded=Math.round(result);if(Math.abs(result-rounded)>1e-6)throw Error('换算后必须为整数');result=rounded;}if((field.minimum!==undefined&&result<field.minimum)||(field.maximum!==undefined&&result>field.maximum))throw Error('数值超出允许范围');return result;}if(meta.control==='lines')return value.split(/\r?\n/).map(s=>s.trim()).filter(Boolean);return (meta.control==='nullable'||meta.control==='secret')&&value===''?null:value;}
function search(query){const terms=query.trim().toLowerCase().split(/\s+/).filter(Boolean);return Object.entries(registry).filter(([path,m])=>terms.every(t=>(path+' '+m.title+' '+m.help+' '+tabs.find(tab=>tab.id===m.tab).title).toLowerCase().includes(t))).map(([path,m])=>({path,...m}));}
function logRows(text){const rows=[];let start=0,depth=0,quote='';for(let i=0;i<text.length;i++){const c=text[i];if(quote){if(c===quote&&text[i-1]!=='\\')quote='';continue;}if(c==='"'||c==="'")quote=c;else if(c==='['||c==='{')depth++;else if(c===']'||c==='}')depth--;else if(c===','&&depth===0){rows.push(text.slice(start,i));start=i+1;}}rows.push(text.slice(start));return rows;}
function providerForm(p={}){return {mode:p.has_joined_url?'gateway':p.session_path||p.authenticate_path?'custom':'standard',display_name:p.display_name||'',address:p.has_joined_url||p.base_url||'',session_path:p.session_path||'',authenticate_path:p.authenticate_path||'',enabled:p.enabled!==false};}
function providerValue(form,original){if(original&&JSON.stringify(form)===JSON.stringify(providerForm(original)))return clone(original);const base=original?clone(original):{provider_id:`provider_${root.crypto?.randomUUID?.().replaceAll('-','')||Date.now().toString(36)+Math.random().toString(36).slice(2)}`};base.display_name=form.display_name.trim();base.enabled=form.enabled;if(form.mode==='gateway'){base.base_url='';base.session_path=null;base.has_joined_url=form.address.trim();}else{base.base_url=form.address.trim().replace(/\/$/,'');base.has_joined_url=null;base.session_path=form.mode==='custom'?(form.session_path||null):null;}base.authenticate_path=form.mode==='custom'||form.mode==='gateway'?(form.authenticate_path||null):null;return base;}
function providerFingerprint(p){return JSON.stringify([p.base_url||'',p.session_path||'',p.has_joined_url||'',p.authenticate_path||'']);}
class Draft{
 constructor(response){this.activeTab='general';this.accept(response);}
 accept(response){this.response=clone(response);this.schema=response.schema;this.writable=!!response.writable;this.base=merge(defaults(response.schema),response.config||response.active||response.effective?.effective||{});this.draft=clone(this.base);this.revision=response.revision;this.dirtyPaths=new Set();this.secrets={};}
 set(path,value){if(!this.writable)throw Error('当前配置只读');set(this.draft,path,value);if(JSON.stringify(get(this.base,path))===JSON.stringify(value))this.dirtyPaths.delete(path);else this.dirtyPaths.add(path);}
 secret(name,value){if(!this.writable)throw Error('当前配置只读');if(value.operation==='keep'||(value.operation==='replace'&&'value'in value&&value.value===''))delete this.secrets[name];else this.secrets[name]=clone(value);}
 get dirtyCount(){return this.dirtyPaths.size+Object.keys(this.secrets).length;}
 payload(){return {config:clone(this.draft),revision:this.revision,...(Object.keys(this.secrets).length?{managed_secrets:clone(this.secrets)}:{})};}
 rebase(response){const changes=[...this.dirtyPaths].map(p=>[p,get(this.draft,p)]),secrets=clone(this.secrets),tab=this.activeTab;this.accept(response);for(const [path,value]of changes)this.set(path,value);this.secrets=secrets;this.activeTab=tab;}
 clear(){this.secrets={};this.dirtyPaths.clear();this.draft=clone(this.base);}
}
function storageChanges(state){const paths=[...state.dirtyPaths].filter(path=>path.startsWith('storage.')&&(path!=='storage.mysql_url_file'||state.draft.storage.backend==='mysql'));if(state.draft.storage.backend==='mysql'&&state.secrets.mysql&&!paths.includes('storage.mysql_url_file'))paths.push('storage.mysql_url_file');return paths;}

// Browser code is mounted explicitly by admin.js, never while loading /account/.
function mount(bridge){
 const doc=root.document,$=id=>doc.getElementById(id),el=(tag,text,className)=>{const e=doc.createElement(tag);if(text!==undefined)e.textContent=text;if(className)e.className=className;return e;};
 let state=null,loading=null,loadedPanels=new Set(),invalidInputs=new Map(),providerChecks=new Map(),activeDialogs=new Set(),secretModes=new Map(),connectionResults=new Map(),saving=false;
 const button=(title,fn,className)=>{const b=el('button',title,className);b.type='button';b.addEventListener('click',async()=>{b.disabled=true;try{await fn();}catch(error){bridge.notice(error.message,true);}finally{if(b.isConnected)b.disabled=false;}});return b;};
 const option=(select,value,title)=>{const o=el('option',title);o.value=value;select.append(o);};
 function readonly(path){return !state.writable||registry[path].control==='readonly'||state.response.field_status?.[path]?.editable===false||path==='features.projectile_snapshots'||path.startsWith('limits.max_projectile_')||path.startsWith('retention.projectile_');}
 function condition(path){const d=state.draft;if(path.includes('_concurrency'))return d.performance.preset==='custom';if(path==='cache.timeout_ms')return d.cache.backend==='redis';if(path.startsWith('cache.')&&['ttl_seconds','max_pages'].includes(path.split('.')[1]))return d.cache.backend!=='none';if(path==='updates.check_interval_hours')return d.updates.auto_update;if(path==='moderation.texture_detection_enabled')return d.moderation.detection_enabled;if(['moderation.score_threshold','moderation.timeout_seconds','moderation.max_textures','moderation.max_texture_bytes','moderation.ysm_parser_path'].includes(path))return d.moderation.detection_enabled&&d.moderation.texture_detection_enabled;return true;}
 function updateBar(){bridge.onSavingChange?.(saving);if(!state)return;$('reload-settings').disabled=saving;$('apply-raw').disabled=saving||!state.writable;$('raw-config').readOnly=saving||!state.writable;for(const input of $('owner-password-form').querySelectorAll('input,button'))input.disabled=saving||!state.writable;$('settings-dirty').textContent=invalidInputs.size?`${invalidInputs.size} 项输入需要修正`:(state.dirtyCount?`${state.dirtyCount} 项待保存`:'所有更改已保存');$('save-settings').disabled=saving||!state.writable||!state.dirtyCount||invalidInputs.size>0;$('validate-settings').disabled=saving||!state.writable||invalidInputs.size>0;$('undo-settings').disabled=saving||(!state.dirtyCount&&!invalidInputs.size);$('settings-form').classList.toggle('is-idle',!saving&&!state.dirtyCount&&invalidInputs.size===0);for(const row of doc.querySelectorAll('[data-setting-path]')){const path=row.dataset.settingPath;row.classList.toggle('is-dirty',state.dirtyPaths.has(path)||!!state.secrets[secretNames[path]]);for(const input of row.querySelectorAll('input,select,textarea,button'))input.disabled=saving||readonly(path)||!condition(path);const note=row.querySelector('.condition-note');if(note)note.hidden=condition(path);}}
 function change(path,value){state.set(path,value);invalidInputs.delete(path);updateBar();}
 function connectionFingerprint(kind){const path=kind==='mysql'?'storage.mysql_url_file':'cache.redis_url_file';return JSON.stringify([get(state.draft,path),state.secrets[kind]||null]);}
 function focus(path){path=Object.keys(registry).find(key=>path===key||path.startsWith(key+'.')||path.startsWith(key+'['))||path;const spec=registry[path];if(!spec)return;activate(spec.tab);const target=doc.querySelector(`[data-setting-path="${path}"]`);target?.scrollIntoView({behavior:'smooth',block:'center'});target?.querySelector('input,select,textarea,button')?.focus();target?.classList.add('is-highlighted');setTimeout(()=>target?.classList.remove('is-highlighted'),1800);}
 function showErrors(errors){const root=$('settings-errors');root.replaceChildren();root.hidden=!errors.length;for(const error of errors){const path=error.path||'config';root.append(button(`${registry[path]?.title||'配置'}：${error.message}`,()=>focus(path),'field-error-link'));}}
 function humanValue(value,meta){if(value===undefined)return '未知';if(value===null||value==='')return '自动 / 未设置';if(typeof value==='boolean')return value?'开启':'关闭';if(Array.isArray(value))return `${value.length} 项`;if(meta.control==='secret')return '已配置引用';if(meta.choices)return meta.choices.find(([key])=>key===value)?.[1]||String(value);return String(meta.control==='number'?toDisplay(Object.keys(registry).find(p=>registry[p]===meta),value)+(meta.unit?' '+meta.unit:''):value);}
 function finishField(row,path,meta){
  const field=fieldSchema(state.schema,path),status=state.response.field_status?.[path],footer=el('div',undefined,'setting-field-footer');
  footer.append(el('small',`默认：${humanValue(field.default,meta)}`));
  const explanations={fixed_owner:'固定服主 · 只读',owner_only:'仅服主可编辑',environment_mode:'环境配置 · 只读',not_implemented:'当前版本不可用',derived_value:'按运行状态计算'};
  footer.append(el('small',explanations[status?.reason]||({restart:'重启后生效',bootstrap_only:'仅首次初始化时使用',unavailable:'当前版本不可用'}[status?.apply]||'保存后生效')));
  if(!readonly(path))footer.append(button('恢复默认',()=>{if(meta.control==='secret'){state.secret(secretNames[path],{operation:'clear'});secretModes.delete(secretNames[path]);}change(path,clone(field.default));render();}));
  row.append(footer);
  const note=el('p','依赖项尚未开启；此值会保留，满足条件后可编辑。','setting-help condition-note');note.hidden=condition(path);row.append(note);
  if((state.response.pending_restart_fields||[]).includes(path)){const activeState=status?.active_state;row.append(el('p',`正在使用：${activeState==='unknown'?'未知（无法核实当前运行值）':activeState==='not_applicable'?'不适用':humanValue(get(state.response.active,path),meta)} · 已保存：${humanValue(get(state.base,path),meta)} · 保存值将在重启后生效`,'pending-note'));}
  return row;
 }
 function renderField(path,meta){
  const row=el('section',undefined,'setting-field-card');row.dataset.settingPath=path;row.dataset.control=meta.control;const heading=el('div',undefined,'setting-field-heading'),label=el('label',meta.title);label.htmlFor=`setting-${path.replaceAll('.','-')}`;label.id=label.htmlFor+'-label';row.setAttribute('aria-labelledby',label.id);heading.append(label);row.append(heading,el('p',meta.help,'setting-help'));
  const field=fieldSchema(state.schema,path),value=get(state.draft,path);let input;
  if(meta.control==='providers'){label.removeAttribute('for');renderProviders(row);return finishField(row,path,meta);}
  if(meta.control==='secret'){label.removeAttribute('for');renderSecret(row,path);return finishField(row,path,meta);}
  if(meta.control==='bind'){label.removeAttribute('for');renderBind(row,path);return finishField(row,path,meta);}
  if(meta.control==='logfilter'){label.removeAttribute('for');renderLog(row,path);return finishField(row,path,meta);}
  if(meta.control==='select'){input=el('select');for(const [v,title]of meta.choices)option(input,v,title);input.value=value;}
  else if(meta.control==='boolean'){input=el('input');input.type='checkbox';input.checked=value===true;input.setAttribute('role','switch');}
  else if(meta.control==='lines'){input=el('textarea');input.rows=3;input.value=(value||[]).join('\n');input.placeholder='每行一个 IP 地址';}
  else{input=el('input');input.type=meta.control==='number'?'number':'text';input.value=toDisplay(path,value);if(meta.control==='number'){input.step='any';if(field.minimum!==undefined)input.min=field.minimum/meta.scale;if(field.maximum!==undefined)input.max=field.maximum/meta.scale;}if(meta.control==='nullable')input.placeholder='自动 / 未设置';if(meta.control==='readonly')input.readOnly=true;}
  input.id=label.htmlFor;input.dataset.settingInput='';input.disabled=readonly(path)||!condition(path);const error=el('p','','field-error');error.id=`${input.id}-error`;input.setAttribute('aria-describedby',error.id);
  input.addEventListener('input',()=>{try{if(!input.validity.valid)throw Error('请检查格式与范围');change(path,meta.control==='boolean'?input.checked:fromDisplay(path,input.value,state.schema));error.textContent='';input.removeAttribute('aria-invalid');}catch(e){invalidInputs.set(path,e.message);error.textContent=e.message;input.setAttribute('aria-invalid','true');updateBar();}});
  if(meta.unit){const wrap=el('div',undefined,'setting-with-unit');wrap.append(input,el('span',meta.unit));row.append(wrap);}else row.append(input);row.append(error);
  if(meta.control==='number'){const bounds=[];if(field.minimum!==undefined)bounds.push(`最小 ${field.minimum/meta.scale}`);if(field.maximum!==undefined)bounds.push(`最大 ${field.maximum/meta.scale}`);row.append(el('small',bounds.join('，')+(meta.unit?' '+meta.unit:''),'setting-help'));}
  return finishField(row,path,meta);
 }
 function renderBind(row,path){const value=get(state.draft,path),match=/^(?:\[([^\]]+)\]|([^:]+)):(\d+)$/.exec(value||'');const host=el('input'),port=el('input'),mode=el('select');host.value=match?.[1]||match?.[2]||'';host.setAttribute('aria-label','监听 IP 地址');port.type='number';port.min=1;port.max=65535;port.value=match?.[3]||8787;port.setAttribute('aria-label','监听端口');for(const [v,t]of [['local','仅本机访问'],['network','监听所有网卡'],['custom','自定义 IP']])option(mode,v,t);mode.value=host.value==='127.0.0.1'?'local':host.value==='0.0.0.0'?'network':'custom';mode.setAttribute('aria-label','监听访问方式');for(const input of [mode,host,port]){input.dataset.settingInput='';input.disabled=readonly(path);}const sync=()=>{const ip=host.value.trim();if(!ip||!port.validity.valid)throw Error('请填写有效的监听 IP 与端口');change(path,`${ip.includes(':')?'['+ip+']':ip}:${port.value}`);};mode.addEventListener('change',()=>{if(mode.value!=='custom')host.value=mode.value==='local'?'127.0.0.1':'0.0.0.0';sync();});for(const input of [host,port])input.addEventListener('change',()=>{try{sync();}catch(e){invalidInputs.set(path,e.message);showErrors([{path,message:e.message}]);updateBar();}});const fields=el('div',undefined,'inline-fields');fields.append(mode,host,port);row.append(fields,el('small','端口范围 1–65535；修改后重启服务。','setting-help'));}
 function renderLog(row,path){const level=el('select'),value=get(state.draft,path);for(const [v,t]of [['info','常规 · 信息与错误'],['warn','精简 · 警告与错误'],['error','仅记录错误'],['debug','调试 · 详细排查'],['trace','追踪 · 最详细'],['custom','按模块设置']])option(level,v,t);level.value=['info','warn','error','debug','trace'].includes(value)?value:'custom';level.dataset.settingInput='';level.setAttribute('aria-label','日志详细程度');const rows=el('div',undefined,'log-filter-rows');let directives=logRows(value||'');function draw(){rows.replaceChildren();rows.hidden=level.value!=='custom';for(let i=0;i<directives.length;i++){const line=el('div',undefined,'inline-fields'),input=el('input');input.value=directives[i];input.setAttribute('aria-label',`模块过滤规则 ${i+1}`);input.placeholder='例如 spm_cloud=debug';input.dataset.settingInput='';input.disabled=readonly(path);input.addEventListener('input',()=>{directives[i]=input.value;change(path,directives.join(','));});const remove=button('删除',()=>{directives.splice(i,1);change(path,directives.join(','));draw();});remove.disabled=readonly(path);line.append(input,remove);rows.append(line);}const add=button('添加模块过滤',()=>{directives.push('spm_cloud=info');change(path,directives.join(','));draw();});add.disabled=readonly(path);rows.append(add);}level.disabled=readonly(path);level.addEventListener('change',()=>{if(level.value==='custom'){directives=logRows(get(state.draft,path));draw();}else{change(path,level.value);draw();}});draw();row.append(level,rows,el('small','例如全局 warn，另加 spm_cloud=debug；复杂选择器和原始精度在未编辑时保留。','setting-help'));}
 function renderSecret(row,path){
  const name=secretNames[path],status=state.response.secret_status?.[name],current=state.secrets[name]||{operation:secretModes.get(name)||'keep'},mode=el('select');
  for(const [v,t]of [['keep',status?.configured?'保留现有凭据':'暂不配置'],['replace','填写并替换'],['clear','明确清除'],['file','使用服务器已有凭据文件'],...(name==='bootstrap_token'?[['generate','生成新的安全令牌']]:[])])option(mode,v,t);
  mode.value=current.operation;mode.setAttribute('aria-label',`${registry[path].title}操作`);mode.dataset.settingInput='';mode.disabled=readonly(path);row.append(el('p',status?.configured?'已配置 · 秘密不会读回浏览器':'尚未配置','secret-state'),mode);const body=el('div',undefined,'secret-fields');row.append(body);
  const values=clone(current),controls={};
  function sync(){secretModes.set(name,mode.value);connectionResults.delete(name);if(mode.value==='file'||mode.value==='keep'){state.secret(name,{operation:'keep'});updateBar();return;}const input={operation:mode.value};if(mode.value==='replace'){for(const [key,control]of Object.entries(controls)){input[key]=control.type==='checkbox'?control.checked:control.type==='number'?Number(control.value):control.value;}if(name==='redis'&&input.database==='')input.database=0;}state.secret(name,input);updateBar();}
  function add(key,title,type='text',fallback=''){const label=el('label',title),input=el('input');input.type=type;input.setAttribute('aria-label',title);input.autocomplete=type==='password'?'new-password':'off';input.value=values[key]??fallback;if(type==='checkbox')input.checked=values[key]??fallback;input.dataset.settingInput='';input.disabled=readonly(path);if(type==='number'){input.min=key==='port'?1:0;input.max=key==='port'?65535:2147483647;}input.addEventListener('input',sync);controls[key]=input;label.append(input);body.append(label);}
  function draw(){body.replaceChildren();for(const key of Object.keys(controls))delete controls[key];
   if(mode.value==='replace'){
    if(name==='mysql'||name==='redis'){const metadata=status?.connection||{};add('host','服务器地址','text',metadata.host||'127.0.0.1');add('port','端口','number',metadata.port||(name==='mysql'?3306:6379));add('username','连接用户名');add('password','新的连接密码','password');add('database',name==='mysql'?'数据库名称':'逻辑数据库编号',name==='mysql'?'text':'number',metadata.database||(name==='mysql'?'spm_cloud':0));if(name==='redis')add('tls','使用加密连接','checkbox',metadata.tls||false);body.append(el('p',name==='mysql'?'特殊字符会自动编码；自定义 TLS 使用已有凭据文件模式。':'密码仅在本次保存中发送，Redis 故障时可回落内存缓存。','small muted'));}
    else add('value',name==='bootstrap_password_hash'?'新的 Argon2id 摘要':'新的秘密值','password');
   }else if(mode.value==='file'){const label=el('label','服务器上的凭据文件路径'),input=el('input');input.value=get(state.draft,path)||'';input.setAttribute('aria-label',`${registry[path].title}文件路径`);input.disabled=readonly(path);input.addEventListener('input',()=>change(path,input.value||null));label.append(input);body.append(label,el('p','相对路径以配置文件所在目录为基准，不会覆盖这个文件。','small muted'));}
   else if(mode.value==='clear')body.append(el('p','保存后移除此秘密引用；原凭据文件保留以便回退。','pending-note'));
    else if(mode.value==='generate')body.append(el('p','保存成功后仅展示一次新令牌，请立即复制。旧令牌随即失效。','pending-note'));
   if(name==='mysql'||name==='redis'){const check=button('检查连接',async()=>{const candidate=state.secrets[name];if(candidate&&candidate.operation!=='replace'&&candidate.operation!=='keep')throw Error('请先填写新的连接信息，或选择保留当前运行凭据');const fingerprint=connectionFingerprint(name);resultLine.textContent='正在检查连接…';const result=await bridge.request('/admin/api/config/check-connection',{method:'POST',json:{kind:name,draft:state.payload()}});if(fingerprint!==connectionFingerprint(name))return;connectionResults.set(name,{fingerprint,result});resultLine.textContent=result.connected?(result.message||'连接检查通过；未写入数据'):checkReason(result.reason);if(result.target)resultLine.textContent+=`；目标账户 ${result.target.accounts??'未建表'}，模型 ${result.target.assets??'未建表'}。${result.target.sqlite_import_eligible?'满足空目标导入条件。':'不会覆盖非空目标。'}`;});check.disabled=readonly(path);const resultLine=el('p','','connection-result');body.append(check,resultLine);}
  }
  mode.addEventListener('change',()=>{if(mode.value==='keep')change(path,get(state.base,path));draw();sync();});draw();
 }
 function checkReason(reason){return {timeout:'连接超时，请检查地址与防火墙',authentication_failed:'认证失败，请检查用户名和密码',database_not_found:'目标数据库不存在，请先创建数据库',invalid_address:'地址或连接字段无效',redirect_rejected:'服务返回重定向，检查已停止',response_too_large:'服务响应超过允许大小',public_dns_required:'认证域名必须解析到允许的公网地址',invalid_provider:'认证服务地址或接口格式不符合要求',connection_or_tls_failed:'连接或 TLS 校验失败',dns_or_connection_failed:'域名解析或连接失败',connection_or_protocol_failed:'连接或协议检查失败'}[reason]||'检查未通过，请核对地址、凭据及服务状态';}
 function configuredProviders(){const configured=get(state.draft,'auth.identity_providers')||[],map=new Map((state.response.provider_admin_entries||[]).map(p=>[p.provider_id,p]));for(const p of configured)map.set(p.provider_id,{...map.get(p.provider_id),...p,source:'config'});return [...map.values()].map(p=>p.source==='managed'&&!configured.some(c=>c.provider_id===p.provider_id)?{...p,enabled:false}:p);}
 function setProvider(provider){const list=clone(get(state.draft,'auth.identity_providers')||[]),index=list.findIndex(p=>p.provider_id===provider.provider_id),clean={};for(const key of providerFields)if(provider[key]!==undefined)clean[key]=provider[key];if(index<0)list.push(clean);else list[index]=clean;change('auth.identity_providers',list);render();}
 function renderProviders(row){
  const editable=!readonly('auth.identity_providers'),list=el('div',undefined,'provider-list'),disabled=el('details',undefined,'disabled-providers');disabled.append(el('summary','已停用服务 · 可随时恢复'));row.append(button('添加第三方服务',()=>editProvider()),list,disabled);row.querySelector('button').disabled=!editable;
  for(const p of configuredProviders()){const card=el('article',undefined,'provider-card'),title=el('h3',p.provider_id==='official'?'Minecraft 官方认证':p.display_name),address=el('p',p.has_joined_url||p.base_url||'','small muted'),source=el('span',{official:'系统内置 · 受保护',database:'数据库遗留 · 尚未纳入配置',config:'配置管理',managed:'配置管理 · 已移除'}[p.source]||'配置管理','badge');card.append(title,address,source,el('p',p.enabled?'已启用':'已停用','small muted'));
   const result=providerChecks.get(p.provider_id);card.append(el('p',result&&result.fingerprint===providerFingerprint(p)?result.message:'尚未检查 · 服务检查不会验证账号登录','connection-result'));
   if(p.provider_id!=='official'){const actions=el('div',undefined,'provider-actions');for(const b of [button(p.source==='database'?'纳入管理 / 编辑':'编辑',()=>editProvider(p)),button(p.enabled?'停用':'恢复',()=>{const clean={};for(const key of providerFields)if(p[key]!==undefined)clean[key]=p[key];clean.enabled=!p.enabled;setProvider(clean);})]){b.disabled=!editable;actions.append(b);}card.append(actions);}else card.append(el('p','官方认证始终保留，不能在此修改或停用。','small muted'));
   (p.enabled?list:disabled).append(card);
  }
 }
 function dialog(title,build){return new Promise(resolve=>{const previousFocus=doc.activeElement,modal=el('dialog',undefined,'settings-dialog'),header=el('div',undefined,'dialog-heading'),heading=el('h2',title);heading.id=`settings-dialog-title-${Date.now()}-${activeDialogs.size}`;modal.setAttribute('aria-labelledby',heading.id);header.append(heading);const close=button('关闭',()=>finish(null));header.append(close);modal.append(header);const content=el('div'),actions=el('div',undefined,'dialog-actions');modal.append(content,actions);let finished=false;function finish(value){if(finished)return;finished=true;for(const input of modal.querySelectorAll('input[type=password],textarea'))input.value='';modal.close();modal.remove();activeDialogs.delete(modal);if(previousFocus?.isConnected)previousFocus.focus();resolve(value);}modal.finish=finish;build(content,actions,finish);modal.addEventListener('cancel',event=>{event.preventDefault();finish(null);});doc.body.append(modal);activeDialogs.add(modal);modal.showModal();(content.querySelector('input,select,textarea')||actions.querySelector('button')||close).focus();});}
 async function editProvider(provider){
  const original=provider?Object.fromEntries(providerFields.filter(k=>provider[k]!==undefined).map(k=>[k,provider[k]])):null,form=providerForm(original||{}),inputs={};
  await dialog(provider?'编辑认证服务':'添加认证服务',(content,actions,finish)=>{
   const mode=el('select');for(const [v,t]of [['standard','标准 Yggdrasil 服务'],['gateway','完整会话验证地址 / 网关'],['custom','自定义兼容接口']])option(mode,v,t);mode.value=form.mode;mode.setAttribute('aria-label','认证接口模式');const modeLabel=el('label','接口方案');modeLabel.append(mode);content.append(modeLabel);
   function add(key,title,placeholder){const label=el('label',title),input=el('input');input.value=form[key]||'';input.placeholder=placeholder||'';input.setAttribute('aria-label',title);input.addEventListener('input',()=>{form[key]=input.value;probe.textContent='地址有更改时需重新检查；检查不会验证账号登录。';});label.append(input);content.append(label);inputs[key]=label;}
   add('display_name','服务名称','例如：我的皮肤站');add('address','服务入口地址','https://skin.example.com/api/yggdrasil');add('session_path','会话验证接口路径','留空使用默认兼容接口');add('authenticate_path','账号登录接口路径','留空沿用默认推导');const probe=el('p','检查服务地址不会保存配置，也不会创建玩家绑定。','connection-result');content.append(probe);
   const visible=()=>{form.mode=mode.value;inputs.session_path.hidden=form.mode!=='custom';inputs.authenticate_path.hidden=form.mode==='standard';inputs.address.firstChild.textContent=form.mode==='gateway'?'完整会话验证地址':'服务入口地址';};mode.addEventListener('change',()=>{visible();probe.textContent='接口方案已更改，请重新检查。';});visible();
   function candidate(){if(!form.display_name.trim()||!form.address.trim())throw Error('请填写服务名称和地址');const value=providerValue(form,original);if(!original&&!form.provider_id)form.provider_id=value.provider_id;if(!original)value.provider_id=form.provider_id;return value;}
    actions.append(button('检查服务',async()=>{const p=candidate(),fingerprint=providerFingerprint(p),result=await bridge.request('/admin/api/providers/check',{method:'POST',json:p});if(providerFingerprint(candidate())!==fingerprint)return;const message=result.reachable?`服务可连接，登录尚未验证。${result.interfaces_recognized?'已识别兼容接口。':'接口尚未识别，可检查自定义路径；这不等于服务不可用。'}`:checkReason(result.reason);probe.textContent=message;providerChecks.set(p.provider_id,{fingerprint,message});}),button('加入配置草稿',()=>{const p=candidate();setProvider(p);finish(true);},'primary'),button('取消',()=>finish(null)));
  });
 }
 async function activate(tab){if(!state||!tabs.some(t=>t.id===tab))return;if(tab==='updates'&&bridge.getSession()?.role!=='owner')tab='general';state.activeTab=tab;for(const b of doc.querySelectorAll('[data-settings-tab]')){const selected=b.dataset.settingsTab===tab;b.setAttribute('aria-selected',String(selected));b.tabIndex=selected?0:-1;}for(const panel of doc.querySelectorAll('[data-settings-panel]'))panel.hidden=panel.dataset.settingsPanel!==tab;
  if((tab==='updates'||tab==='audit')&&!loadedPanels.has(tab)){loadedPanels.add(tab);try{await (tab==='updates'?bridge.loadMaintenance():bridge.loadAudit());}catch(e){loadedPanels.delete(tab);bridge.notice(e.message,true);}}
 }
 function render(){if(!state)return;for(const container of doc.querySelectorAll('[data-settings-fields]'))container.replaceChildren();for(const [path,meta]of Object.entries(registry))doc.querySelector(`[data-settings-fields="${meta.tab}"]`).append(renderField(path,meta));
  for(const tab of tabs){const b=doc.querySelector(`[data-settings-tab="${tab.id}"]`);if(b)b.hidden=tab.id==='updates'&&bridge.getSession()?.role!=='owner';}
  $('settings-note').textContent=state.writable?'修改保留在统一草稿中。保存前会校验，标注需重启的设置在重启后生效。':bridge.getSession()?.role==='admin'?'管理员可查看配置和操作记录，只有服主可以修改或运行连接检查。':'当前为环境配置模式，只读。使用 --config 指定配置文件后可编辑。';
  const pending=state.response.pending_restart_fields||[];$('settings-restart').hidden=!pending.length;$('settings-restart').textContent=pending.length?`${pending.length} 项保存值等待重启：${pending.map(p=>registry[p]?.title||'运行日志过滤').join('、')}`:'';
  $('raw-config').readOnly=!state.writable;$('apply-raw').disabled=!state.writable;$('raw-config').value=JSON.stringify(state.draft,null,2);$('owner-password-panel').hidden=!state.writable;
  const storage=$('storage-guide');storage.textContent=`当前运行数据库：${state.response.active?.storage?.backend==='mysql'?'外部 MySQL':'内置 SQLite'}。实际模型目录：${state.response.active?.storage?.object_dir||'未知'}。切换步骤：检查连接 → 备份当前数据 → 保存配置 → 重启服务。仅支持已有 SQLite 导入空 MySQL；非空目标不覆盖，MySQL 到 SQLite 不自动迁移。更改目录不会移动模型文件。`;
  const scanner=button('检查扫描服务连接',async()=>{const result=await bridge.request('/admin/api/config/check-connection',{method:'POST',json:{kind:'nudenet',scanner_url:get(state.draft,'moderation.scanner_url')}});scannerResult.textContent=result.connected?'扫描服务已响应，图片识别尚未验证':checkReason(result.reason);});scanner.disabled=!state.writable;const scannerResult=el('p','','connection-result');doc.querySelector('[data-settings-fields="models"]').append(scanner,scannerResult);
  showErrors([]);activate(state.activeTab);updateBar();
 }
 async function open(tab){if(!state){if(!loading)loading=bridge.request('/admin/api/config').then(response=>{state=new Draft(response);render();}).finally(()=>{loading=null;});await loading;}if(tab)await activate(tab);else await activate(state.activeTab);}
  async function reload(rebase=false){const fresh=await bridge.request('/admin/api/config');if(rebase&&state)state.rebase(fresh);else{const tab=state?.activeTab||'general';state=new Draft(fresh);state.activeTab=tab;invalidInputs.clear();providerChecks.clear();secretModes.clear();connectionResults.clear();}render();}
 async function validate(payload=state.payload()){if(invalidInputs.size){showErrors([...invalidInputs].map(([path,message])=>({path,message})));return false;}const result=await bridge.request('/admin/api/config/validate',{method:'POST',json:payload});showErrors(result.errors||[]);if(result.valid)bridge.notice(`校验通过：${result.applied_fields.length} 项可即时生效，${result.pending_restart_fields.length} 项待重启。`);return !!result.valid;}
  async function confirmStorageChange(){
   const changed=storageChanges(state);if(!changed.length)return true;
   return !!await dialog('确认存储与数据库变更',(content,actions,finish)=>{
    const active=state.response.active?.storage||{},target=state.draft.storage;
    content.append(el('p',`当前运行：${active.backend==='mysql'?'外部 MySQL':'内置 SQLite'}；目标：${target.backend==='mysql'?'外部 MySQL':'内置 SQLite'}。`),el('p',`当前数据库：${active.database_path||'未知'}；模型目录：${active.object_dir||'未知'}。`),el('p','仅支持有数据 SQLite 导入空 MySQL；非空目标不会被覆盖。MySQL → SQLite 不会自动迁移。更改目录不会搬迁文件。'));
    const resultLine=el('p','','connection-result'),steps=el('div',undefined,'database-steps'),backupLabel=el('label'),backup=el('input');backup.type='checkbox';backupLabel.append(backup,el('span','我已保存当前数据库与模型备份，并了解切换需要重启。'));steps.append(backupLabel);content.append(resultLine,steps);
    let checked=target.backend!=='mysql'||connectionResults.get('mysql')?.fingerprint===connectionFingerprint('mysql')&&connectionResults.get('mysql')?.result.connected;
    const confirm=button('确认保存，重启后生效',()=>finish(true),'primary');const sync=()=>confirm.disabled=!checked||!backup.checked;backup.addEventListener('change',sync);
    if(target.backend==='mysql')actions.append(button('检查目标 MySQL',async()=>{const fingerprint=connectionFingerprint('mysql');const report=await bridge.request('/admin/api/config/check-connection',{method:'POST',json:{kind:'mysql',draft:state.payload()}});checked=report.connected&&fingerprint===connectionFingerprint('mysql');connectionResults.set('mysql',{fingerprint,result:report});resultLine.textContent=report.connected?`连接通过；目标账户 ${report.target?.accounts??'未建表'}，模型 ${report.target?.assets??'未建表'}。${report.target?.sqlite_import_eligible?'满足空目标导入条件。':'非空目标保留原数据，不自动覆盖。'}`:checkReason(report.reason);sync();}));
    actions.append(button('下载当前数据备份',async()=>{await bridge.downloadBackup();resultLine.textContent='当前运行数据的备份已交给浏览器下载，请核对文件后勾选确认。';}),confirm,button('取消',()=>finish(false)));sync();
   });
  }
  async function showGenerated(generated){if(!Object.keys(generated).length)return;await dialog('请立即保存新令牌',(content,actions,finish)=>{content.append(el('p','此值只展示一次。关闭后无法从管理接口读回。'));const field=el('textarea');field.readOnly=true;field.value=Object.values(generated).join('\n');field.setAttribute('aria-label','一次性生成的初始令牌');content.append(field);actions.append(button('复制令牌',()=>root.navigator.clipboard.writeText(field.value)),button('已保存，关闭',()=>finish(true),'primary'));});}
  async function save(){if(!state?.writable||saving)return false;const submitted=state,payload=submitted.payload(),account=bridge.getSession()?.account_id;saving=true;updateBar();try{if(!await validate(payload))return false;if(!await confirmStorageChange())return false;let recovery='';if(get(state.draft,'web_ui.enabled')===false){recovery=state.response.capabilities?.web_ui_recovery_command||'';const accepted=await dialog('关闭管理网页',(content,actions,finish)=>{content.append(el('p','关闭后管理页面和 API 将停止访问，请复制恢复命令。'),el('pre',recovery));actions.append(button('复制恢复命令',async()=>{await root.navigator.clipboard.writeText(recovery);bridge.notice('恢复命令已复制');}),button('确认关闭并保存',()=>finish(true),'primary'),button('取消',()=>finish(false)));});if(!accepted)return false;}
   const bootstrap=payload.managed_secrets?.bootstrap_token;const result=await bridge.request('/admin/api/config',{method:'PUT',json:payload});const generated=result.generated_secrets||{},nextToken=generated.bootstrap_token||(bootstrap?.operation==='replace'?bootstrap.value:null);submitted.clear();if(state!==submitted||bridge.getSession()?.account_id!==account){bridge.notice('配置已保存；当前登录已变化，请重新登录确认状态。',true);return true;}secretModes.clear();connectionResults.clear();for(const input of doc.querySelectorAll('#tab-settings input[type=password]'))input.value='';await showGenerated(generated);if(recovery){bridge.onDisabled(recovery);clear();return true;}if(bootstrap&&await bridge.onBootstrapChanged(nextToken)===false)return true;try{await reload();}catch(error){bridge.notice("配置已保存，但未能重新读取状态。请重新载入确认。",true);return true;}loadedPanels.delete('updates');bridge.notice(result.pending_restart_fields?.length?'配置已保存；部分设置等待服务重启。':'配置已保存并生效。');return true;}
   catch(e){showErrors(e.errors||[{path:'config',message:e.message}]);if(e.status===409)await dialog('配置已被其他操作更新',(content,actions,finish)=>{content.append(el('p','你的草稿和秘密输入仍在。可载入服务器新值作比较，再保留本次改动重试。'));actions.append(button('载入新值并保留我的改动',async()=>{const fresh=await bridge.request('/admin/api/config');const changed=[...state.dirtyPaths].map(path=>`${registry[path]?.title||path}：服务器值 ${humanValue(get(merge(defaults(fresh.schema),fresh.config),path),registry[path])}`);content.append(el('pre',changed.join('\n')||'秘密操作将继续保留。'));actions.replaceChildren(button('采用此修订号，保留我的草稿',async()=>{state.rebase(fresh);render();finish(true);},'primary'),button('取消',()=>finish(false)));}),button('保留草稿，稍后处理',()=>finish(false)));});throw e;}
   finally{saving=false;updateBar();}
 }
 async function beforeMaintenance(restart=false){if(saving)return false;if(!state?.dirtyCount&&!invalidInputs.size)return true;return !!await dialog(restart?'更新前处理配置草稿':'此操作使用当前运行配置',(content,actions,finish)=>{content.append(el('p',restart?'安装更新会重启服务；请先保存或丢弃本次配置草稿。':'备份和导出使用已生效的数据库、模型目录及查询条件，不包含未保存草稿。'));if(restart)actions.append(button('先保存，再继续',async()=>{if(await save())finish(true);},'primary'),button('丢弃草稿并继续',()=>{resetDraft();render();finish(true);}));else actions.append(button('继续操作',()=>finish(true),'primary'));actions.append(button('取消',()=>finish(false)));});}
 for(const b of doc.querySelectorAll('[data-settings-tab]'))b.addEventListener('click',()=>activate(b.dataset.settingsTab));
 $('settings-tablist').addEventListener('keydown',event=>{if(!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;const visible=[...doc.querySelectorAll('[data-settings-tab]')].filter(b=>!b.hidden),index=visible.indexOf(doc.activeElement);let next=event.key==='Home'?0:event.key==='End'?visible.length-1:(index+(event.key==='ArrowRight'?1:-1)+visible.length)%visible.length;event.preventDefault();visible[next].focus();activate(visible[next].dataset.settingsTab);});
 $('reload-settings').addEventListener('click',async()=>{if(saving)return;try{if(state?.dirtyCount&&!root.confirm('重新载入会丢弃当前草稿和秘密输入，继续吗？'))return;await reload();}catch(e){bridge.notice(e.message,true);}});
 $('settings-form').addEventListener('submit',event=>{event.preventDefault();const b=$('save-settings');b.disabled=true;save().catch(e=>bridge.notice(e.message,true)).finally(updateBar);});
 $('validate-settings').addEventListener('click',()=>validate().catch(e=>bridge.notice(e.message,true)));
 function resetDraft(){state?.clear();invalidInputs.clear();secretModes.clear();connectionResults.clear();providerChecks.clear();}
 $('undo-settings').addEventListener('click',()=>{if(!state||saving)return;resetDraft();render();});
 $('settings-search').addEventListener('input',event=>{const results=$('settings-search-results'),query=event.target.value;results.replaceChildren();results.hidden=!query.trim();if(query.trim()){const matches=search(query);if(!matches.length)results.append(el('p','没有匹配的设置，请尝试其他名称或技术别名。'));for(const item of matches)results.append(button(`${tabs.find(t=>t.id===item.tab).title} · ${item.title}`,()=>{focus(item.path);results.hidden=true;}));}});
 $('apply-raw').addEventListener('click',()=>{if(saving)return;try{const imported=JSON.parse($('raw-config').value);if(!imported||Array.isArray(imported)||typeof imported!=='object')throw Error('请选择完整配置对象');const merged=merge(defaults(state.schema),imported);for(const path of Object.keys(registry))state.set(path,get(merged,path));render();bridge.notice('已导入配置草稿，请校验后保存。秘密输入不包含在 JSON 中。');}catch(e){bridge.notice(e.message,true);}});
 $('export-config').addEventListener('click',()=>{if(!state)return;const link=el('a'),url=URL.createObjectURL(new Blob([JSON.stringify(state.draft,null,2)],{type:'application/json'}));link.href=url;link.download='spm-cloud-config.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
 $('owner-password-form').addEventListener('submit',async event=>{event.preventDefault();if(saving)return;const password=new FormData(event.target).get('password');try{await bridge.request(`/admin/api/accounts/${encodeURIComponent(bridge.getSession().account_id)}`,{method:'PUT',json:{role:'owner',disabled:false,password}});event.target.reset();bridge.notice('密码已更新，旧账号会话已撤销，请重新登录。');bridge.onPasswordChanged();}catch(e){event.target.reset();bridge.notice(e.message,true);}});
  root.addEventListener('beforeunload',event=>{if(saving||state?.dirtyCount||invalidInputs.size){event.preventDefault();event.returnValue='';}});
  function clear(){state?.clear();state=null;loadedPanels.clear();providerChecks.clear();invalidInputs.clear();secretModes.clear();connectionResults.clear();for(const input of doc.querySelectorAll('#tab-settings input[type=password]'))input.value='';$('raw-config').value='';for(const container of doc.querySelectorAll('[data-settings-fields]'))container.replaceChildren();for(const modal of [...activeDialogs])modal.finish(null);}
 return {open,save,validate,reload,clear,beforeMaintenance,get saving(){return saving;},get dirty(){return !!state?.dirtyCount;}};
}
const api={tabs,registry,providerFields,defaults,merge,get,set,toDisplay,fromDisplay,search,logRows,providerForm,providerValue,providerFingerprint,storageChanges,Draft,mount};
if(typeof module!=='undefined'&&module.exports)module.exports=api;else root.SPMSettings=api;
})(typeof globalThis==='undefined'?this:globalThis);
