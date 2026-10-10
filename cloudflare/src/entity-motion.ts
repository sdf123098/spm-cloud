/** Explicit MAID/FAKE_PLAYER display events; native AI and game control are never replicated. */
import {readVisualJson,requireVisualContext,visualBucket,visualDefaults,visualObject,visualSlug,
  visualIdentifier,visualUuid,visualInteger,readableVisualResource,type ResourceRef,type VisualEnv,type VisualLimits} from './projectile-snapshots';
type MotionEnv=VisualEnv & {SPM_CLOUD_ENTITY_MOTION?:string};
export function entityMotionEnabled(env: MotionEnv): boolean { return env.SPM_CLOUD_ENTITY_MOTION === 'true'; }
type Controller={state:string;started_at_unix_ms:number;variables:Record<string,number>};
type Expression={event_id:string;started_at_unix_ms:number;expression:string;values:number[]};
type Motion={event_id:string;animation_key:string;started_at_unix_ms:number;roaming:Record<string,number>;expressions:Expression[];controllers:Record<string,Controller>};
export type EntityMotionUpdate={world_epoch:string;dimension_id:string;entity_kind:string;target_id:string;
  binding_revision:number;appearance_revision:number;expected_revision:number;motion:Motion};
type Row={publisher_account_id:string;update_json:string|null;resource_json:string|null;revision:number;expires_at_ms:number;event_id:string;started_at_ms:number};
type Bound={target_id:string;binding_revision:number;entity_kind:string;appearance_revision:number;
  asset_id:string|null;asset_revision:number|null;raw_sha256:string|null;texture_id:string|null;format:string|null;disabled:number;scope_role:string;target_role:string};
const response=(body:unknown,status=200)=>Response.json(body,{status,headers:{'cache-control':'no-store','access-control-allow-origin':'*'}});
function fail(status:number,code:string):never{throw response({code,retryable:false,message_key:`cloud.error.${code.toLowerCase()}`},status);}
function text(value:unknown,max:number,empty=false):string{
  if(typeof value!=='string'||value.length>max||!empty&&!value.trim().length||/[\x00-\x1f\x7f-\x9f]/.test(value))fail(400,'INVALID_METADATA');return value;
}
function finite(value:unknown):number{if(typeof value!=='number'||!Number.isFinite(value)||!Number.isFinite(Math.fround(value)))fail(400,'INVALID_METADATA');return value;}
function numbers(value:unknown,max:number):Record<string,number>{
  const object=visualObject(value===undefined?{}:value),result:Record<string,number>=Object.create(null);
  if(Object.keys(object).length>64)fail(400,'INVALID_METADATA');
  for(const key of Object.keys(object).sort())result[text(key,max)]=finite(object[key]);return result;
}
export function parseEntityMotion(value:unknown,limits:VisualLimits):EntityMotionUpdate{
  const v=visualObject(value,['world_epoch','dimension_id','entity_kind','target_id','binding_revision','appearance_revision','expected_revision','motion']);
  if(v.entity_kind!=='MAID'&&v.entity_kind!=='FAKE_PLAYER')fail(400,'INVALID_METADATA');
  const m=visualObject(v.motion,['event_id','animation_key','started_at_unix_ms','roaming','expressions','controllers']);
  const expressions:Expression[]=[];
  if(m.expressions!==undefined){if(!Array.isArray(m.expressions)||m.expressions.length>16)fail(400,'INVALID_METADATA');
    for(const raw of m.expressions){const e=visualObject(raw,['event_id','started_at_unix_ms','expression','values']);
      if(!Array.isArray(e.values)||e.values.length>16)fail(400,'INVALID_METADATA');
      expressions.push({event_id:text(e.event_id,64),started_at_unix_ms:visualInteger(e.started_at_unix_ms),expression:text(e.expression,2048,true),values:e.values.map(finite)});}}
  const controllers:Record<string,Controller>=Object.create(null),states=visualObject(m.controllers===undefined?{}:m.controllers);
  if(Object.keys(states).length>64)fail(400,'INVALID_METADATA');
  for(const key of Object.keys(states).sort()){const c=visualObject(states[key],['state','started_at_unix_ms','variables']);
    controllers[text(key,128)]={state:text(c.state,128,true),started_at_unix_ms:visualInteger(c.started_at_unix_ms),variables:numbers(c.variables,64)};}
  const motion:Motion={event_id:text(m.event_id,64),animation_key:text(m.animation_key,256,true),started_at_unix_ms:visualInteger(m.started_at_unix_ms),roaming:numbers(m.roaming,32),expressions,controllers};
  if(Object.keys(motion.roaming).length+Object.values(controllers).reduce((count,c)=>count+Object.keys(c.variables).length,0)>limits.max_visual_variables)fail(400,'INVALID_METADATA');
  const result:EntityMotionUpdate={world_epoch:visualSlug(v.world_epoch),dimension_id:visualIdentifier(v.dimension_id),entity_kind:v.entity_kind,target_id:visualSlug(v.target_id),binding_revision:visualInteger(v.binding_revision),appearance_revision:visualInteger(v.appearance_revision),expected_revision:visualInteger(v.expected_revision),motion};
  if(new TextEncoder().encode(JSON.stringify(result)).length>limits.max_visual_state_bytes)fail(413,'MESSAGE_TOO_LARGE');return result;
}
const authorizedSql=`SELECT b.target_id,b.revision binding_revision,b.entity_kind,a.revision appearance_revision,a.asset_id,a.asset_revision,a.raw_sha256,a.texture_id,r.format,a.disabled,s.role scope_role,acl.role target_role
  FROM entity_bindings b JOIN targets t ON t.target_id=b.target_id AND t.scope_id=b.scope_id AND t.target_kind=b.entity_kind
  JOIN scope_acl s ON s.scope_id=b.scope_id AND s.account_id=?4 JOIN target_acl acl ON acl.target_id=b.target_id AND acl.account_id=?4
  JOIN appearances a ON a.target_id=b.target_id LEFT JOIN asset_revisions r ON r.asset_id=a.asset_id AND r.revision=a.asset_revision AND r.raw_sha256=a.raw_sha256
  WHERE b.scope_id=?1 AND b.world_epoch=?2 AND b.entity_uuid=?3`;
async function authorized(env:MotionEnv,account:string,scope:string,entity:string,input:EntityMotionUpdate,edit:boolean):Promise<ResourceRef>{
  const row=await env.DB.prepare(authorizedSql).bind(scope,input.world_epoch,entity,account).first<Bound>();
  if(!row||row.disabled||!row.asset_id||!row.asset_revision||!row.raw_sha256||!row.texture_id||!row.format)fail(403,'ASSET_ACCESS_DENIED');
  if(row.target_id!==input.target_id||row.binding_revision!==input.binding_revision||row.entity_kind!==input.entity_kind||row.appearance_revision!==input.appearance_revision)fail(409,'REVISION_CONFLICT');
  if(edit&&![row.scope_role,row.target_role].every(role=>['edit','editor','manage','owner'].includes(role)))fail(403,'ASSET_ACCESS_DENIED');
  const resource:ResourceRef={asset_id:row.asset_id,asset_revision:row.asset_revision,raw_sha256:row.raw_sha256,texture_id:row.texture_id,format:row.format};
  if(!await readableVisualResource(env,account,resource))fail(403,'ASSET_ACCESS_DENIED');return resource;
}
async function row(env:MotionEnv,scope:string,epoch:string,dimension:string,entity:string):Promise<Row|null>{
  return env.DB.prepare('SELECT * FROM entity_motion_states WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4').bind(scope,epoch,dimension,entity).first<Row>();
}
function entry(entity:string,update:EntityMotionUpdate,resource:ResourceRef,revision:number,time:number,expiry:number):unknown{
  return {entity_uuid:entity,update,resource,revision,server_time_unix_ms:time,expires_at_unix_ms:expiry};
}
export async function entityMotionRoute(request:Request,env:MotionEnv,account:string,limits=visualDefaults,time=Date.now()):Promise<Response|null>{
  const match=new URL(request.url).pathname.match(/^\/v1\/scopes\/([^/]+)\/entity-motion\/([^/]+)$/);if(!match)return null;
  if(!entityMotionEnabled(env))return response({code:'ASSET_NOT_FOUND'},404);
  const scope=visualSlug(decodeURIComponent(match[1])),entity=match[2],v=await readVisualJson(request,limits.max_visual_state_bytes);
  if(entity==='revisions'&&request.method==='POST'){
    visualObject(v,['world_epoch','dimension_id','entity_uuids']);const epoch=visualSlug(v.world_epoch),dimension=visualIdentifier(v.dimension_id);
    if(!Array.isArray(v.entity_uuids)||v.entity_uuids.length>limits.max_entity_query_count)fail(400,'INVALID_METADATA');
    await requireVisualContext(env,account,scope,epoch,dimension);const entries:unknown[]=[];
    for(const raw of v.entity_uuids){const id=visualUuid(raw),found=await env.DB.prepare(`SELECT COALESCE(m.revision,0) revision FROM entity_bindings b
      JOIN targets t ON t.target_id=b.target_id AND t.scope_id=b.scope_id AND t.target_kind=b.entity_kind
      JOIN scope_acl s ON s.scope_id=b.scope_id AND s.account_id=?4 JOIN target_acl a ON a.target_id=b.target_id AND a.account_id=?4
      LEFT JOIN entity_motion_states m ON m.scope_id=b.scope_id AND m.world_epoch=b.world_epoch AND m.dimension_id=?5 AND m.entity_uuid=b.entity_uuid
      WHERE b.scope_id=?1 AND b.world_epoch=?2 AND b.entity_uuid=?3 AND b.entity_kind IN ('MAID','FAKE_PLAYER')
      AND s.role IN ('edit','editor','manage','owner') AND a.role IN ('edit','editor','manage','owner')`)
      .bind(scope,epoch,id,account,dimension).first<{revision:number}>();if(found)entries.push({entity_uuid:id,revision:found.revision});}
    return response({entries});
  }
  if(request.method==='DELETE'){
    visualObject(v,['world_epoch','dimension_id','expected_revision']);const id=visualUuid(entity),epoch=visualSlug(v.world_epoch),dimension=visualIdentifier(v.dimension_id),revision=visualInteger(v.expected_revision);
    const previous=await row(env,scope,epoch,dimension,id);if(!previous)fail(404,'ASSET_NOT_FOUND');
    if(previous.publisher_account_id!==account)fail(403,'ASSET_ACCESS_DENIED');if(previous.revision!==revision)fail(409,'REVISION_CONFLICT');
    const mutation=await env.DB.prepare('UPDATE entity_motion_states SET update_json=NULL,resource_json=NULL,expires_at_ms=0,revision=revision+1 WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4 AND publisher_account_id=?5 AND revision=?6')
      .bind(scope,epoch,dimension,id,account,revision).run();if(!mutation.meta.changes)fail(409,'REVISION_CONFLICT');return response({revision:revision+1});
  }
  if(entity==='query'&&request.method==='POST'){
    visualObject(v,['world_epoch','dimension_id','entity_uuids']);const epoch=visualSlug(v.world_epoch),dimension=visualIdentifier(v.dimension_id);
    if(!Array.isArray(v.entity_uuids)||v.entity_uuids.length>limits.max_entity_query_count)fail(400,'INVALID_METADATA');
    await requireVisualContext(env,account,scope,epoch,dimension);const entries:unknown[]=[];
    for(const raw of v.entity_uuids){const id=visualUuid(raw),stored=await row(env,scope,epoch,dimension,id);
      if(!stored||stored.expires_at_ms<=time||!stored.update_json||!stored.resource_json)continue;
      const update=parseEntityMotion(JSON.parse(stored.update_json),limits);
      try{await requireVisualContext(env,stored.publisher_account_id,scope,epoch,dimension);
        const resource=await authorized(env,stored.publisher_account_id,scope,id,update,true);
        await authorized(env,account,scope,id,update,false);
        if(JSON.stringify(resource)===stored.resource_json)entries.push(entry(id,update,resource,stored.revision,time,stored.expires_at_ms));
      }catch(error){if(!(error instanceof Response))throw error;}}
    return response({entries});
  }
  if(request.method!=='PUT')return response({code:'ASSET_NOT_FOUND'},404);
  const id=visualUuid(entity),input=parseEntityMotion(v,limits);
  await requireVisualContext(env,account,scope,input.world_epoch,input.dimension_id);
  const resource=await authorized(env,account,scope,id,input,true),previous=await row(env,scope,input.world_epoch,input.dimension_id,id);
  if((previous?.revision??0)!==input.expected_revision||previous&&previous.publisher_account_id!==account&&previous.expires_at_ms>time)fail(409,'REVISION_CONFLICT');
  if(previous){if(previous.event_id===input.motion.event_id&&(previous.expires_at_ms<=time||previous.started_at_ms!==input.motion.started_at_unix_ms))fail(409,'IDEMPOTENCY_CONFLICT');
    if(previous.update_json){const old=parseEntityMotion(JSON.parse(previous.update_json),limits);
      if(old.motion.event_id===input.motion.event_id&&old.motion.animation_key!==input.motion.animation_key)fail(409,'IDEMPOTENCY_CONFLICT');
      if(old.binding_revision===input.binding_revision&&old.appearance_revision===input.appearance_revision&&old.motion.started_at_unix_ms>input.motion.started_at_unix_ms)fail(409,'REVISION_CONFLICT');}}
  const expiry=time+60000,next=input.expected_revision+1;
  const results=await env.DB.batch([visualBucket(env,account,limits,time),env.DB.prepare(`INSERT INTO entity_motion_states(scope_id,world_epoch,dimension_id,entity_uuid,publisher_account_id,update_json,resource_json,revision,expires_at_ms,event_id,started_at_ms)
    SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11 WHERE changes()=1
    AND EXISTS(SELECT 1 FROM entity_bindings b JOIN targets t ON t.target_id=b.target_id AND t.scope_id=b.scope_id AND t.target_kind=b.entity_kind
      JOIN scopes w ON w.scope_id=b.scope_id AND w.world_epoch=b.world_epoch
      JOIN scope_acl s ON s.scope_id=b.scope_id AND s.account_id=?5 JOIN target_acl acl ON acl.target_id=b.target_id AND acl.account_id=?5
      JOIN appearances a ON a.target_id=b.target_id JOIN asset_revisions r ON r.asset_id=a.asset_id AND r.revision=a.asset_revision AND r.raw_sha256=a.raw_sha256
      JOIN assets asset ON asset.asset_id=r.asset_id LEFT JOIN asset_acl access ON access.asset_id=asset.asset_id AND access.account_id=?5
      WHERE b.scope_id=?1 AND b.world_epoch=?2 AND b.entity_uuid=?4 AND b.target_id=?12 AND b.revision=?13 AND b.entity_kind=?14
      AND a.revision=?15 AND a.disabled=0 AND r.asset_id=?16 AND r.revision=?17 AND r.raw_sha256=?18 AND r.format=?19 AND a.texture_id=?20
      AND s.role IN ('edit','editor','manage','owner') AND acl.role IN ('edit','editor','manage','owner') AND (asset.visibility='PUBLIC' OR access.permission IN ('manage','render_read')))
    AND (EXISTS(SELECT 1 FROM entity_motion_states WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4 AND revision=?21)
      OR (?21=0 AND NOT EXISTS(SELECT 1 FROM entity_motion_states WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4)))
    ON CONFLICT(scope_id,world_epoch,dimension_id,entity_uuid) DO UPDATE SET publisher_account_id=excluded.publisher_account_id,update_json=excluded.update_json,resource_json=excluded.resource_json,revision=excluded.revision,expires_at_ms=excluded.expires_at_ms,event_id=excluded.event_id,started_at_ms=excluded.started_at_ms
    WHERE revision=?21 AND (publisher_account_id=?5 OR expires_at_ms<=?22)
      AND (event_id<>?10 OR (expires_at_ms>?22 AND started_at_ms=?11 AND json_extract(update_json,'$.motion.animation_key')=?23))
      AND (json_extract(update_json,'$.binding_revision')<>?13 OR json_extract(update_json,'$.appearance_revision')<>?15 OR started_at_ms<=?11 OR update_json IS NULL)`)
    .bind(scope,input.world_epoch,input.dimension_id,id,account,JSON.stringify(input),JSON.stringify(resource),next,expiry,input.motion.event_id,input.motion.started_at_unix_ms,input.target_id,input.binding_revision,input.entity_kind,input.appearance_revision,resource.asset_id,resource.asset_revision,resource.raw_sha256,resource.format,resource.texture_id,input.expected_revision,time,input.motion.animation_key)]);
  if(!results[0].meta.changes)fail(429,'RATE_LIMITED');if(!results[1].meta.changes)fail(409,'REVISION_CONFLICT');
  return response(entry(id,input,resource,next,time,expiry));
}
