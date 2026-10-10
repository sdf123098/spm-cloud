/** Independent bindings require explicit Cloud scope/target edit ACL; passengers grant none. */
import {readVisualJson,requireVisualContext,visualBucket,visualDefaults,visualObject,
  visualSlug,visualIdentifier,visualUuid,visualInteger,visualText,readableVisualResource,
  type ResourceRef,type VisualEnv,type VisualLimits} from './projectile-snapshots';
type VehicleEnv=VisualEnv & {SPM_CLOUD_VEHICLE_BINDINGS?:string};
export function vehicleEnabled(env:VehicleEnv):boolean{return env.SPM_CLOUD_VEHICLE_BINDINGS==='true';}
type Binding={world_epoch:string;dimension_id:string;entity_kind:string;target_id:string;
  expected_revision:number;resource:ResourceRef|null;variables:Record<string,number>};
type Row={publisher_account_id:string;binding_json:string;revision:number;target_id:string};
const response=(body:unknown,status=200)=>Response.json(body,{status,headers:{'cache-control':'no-store','access-control-allow-origin':'*'}});
function fail(status:number,code:string):never{throw response({code,retryable:false,message_key:`cloud.error.${code.toLowerCase()}`},status);}
export function parseVehicle(value:unknown,limits:VisualLimits):Binding {
  const v=visualObject(value,['world_epoch','dimension_id','entity_kind','target_id','expected_revision','resource','variables']);
  let resource:ResourceRef|null=null;
  if(v.resource!==null){const r=visualObject(v.resource,['asset_id','asset_revision','raw_sha256','texture_id','format']);
    const sha=visualText(r.raw_sha256,64),format=visualText(r.format);
    if(!/^[a-f0-9]{64}$/.test(sha)||!['ysm','zip','bbmodel','gltf','glb'].includes(format))fail(400,'INVALID_METADATA');
    resource={asset_id:visualSlug(r.asset_id),asset_revision:visualInteger(r.asset_revision,1),raw_sha256:sha,texture_id:visualText(r.texture_id),format};}
  const values=visualObject(v.variables===undefined?{}:v.variables);const variables:Record<string,number>={};
  if(Object.keys(values).length>limits.max_visual_variables||resource===null&&Object.keys(values).length)fail(400,'INVALID_METADATA');
  for(const key of Object.keys(values).sort()){visualText(key,32);const value=values[key];
    if(typeof value!=='number'||!Number.isFinite(value)||!Number.isFinite(Math.fround(value)))fail(400,'INVALID_METADATA');variables[key]=value;}
  const binding:Binding={world_epoch:visualSlug(v.world_epoch),dimension_id:visualIdentifier(v.dimension_id),entity_kind:visualIdentifier(v.entity_kind),target_id:visualSlug(v.target_id),expected_revision:visualInteger(v.expected_revision),resource,variables};
  if(new TextEncoder().encode(JSON.stringify(binding)).length>limits.max_visual_state_bytes)fail(413,'MESSAGE_TOO_LARGE');
  return binding;
}
async function editable(env:VehicleEnv,account:string,scope:string,target:string):Promise<boolean>{
  return !!await env.DB.prepare(`SELECT 1 FROM targets t JOIN scope_acl s ON s.scope_id=t.scope_id AND s.account_id=?1
    JOIN target_acl a ON a.target_id=t.target_id AND a.account_id=?1 WHERE t.target_id=?2 AND t.scope_id=?3 AND t.target_kind='VEHICLE'
    AND s.role IN ('editor','edit','manage','owner') AND a.role IN ('editor','edit','manage','owner')`).bind(account,target,scope).first();
}
const editPredicate=`EXISTS(SELECT 1 FROM targets t JOIN scope_acl s ON s.scope_id=t.scope_id AND s.account_id=?6
    JOIN target_acl a ON a.target_id=t.target_id AND a.account_id=?6 WHERE t.target_id=?5 AND t.scope_id=?1 AND t.target_kind='VEHICLE'
    AND s.role IN ('editor','edit','manage','owner') AND a.role IN ('editor','edit','manage','owner'))`;
async function row(env:VehicleEnv,scope:string,epoch:string,dimension:string,entity:string):Promise<Row|null>{
  return env.DB.prepare('SELECT * FROM vehicle_bindings WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4').bind(scope,epoch,dimension,entity).first<Row>();
}
function entry(entity:string,binding:Binding,revision:number,time:number):unknown {
  return {entity_uuid:entity,binding,revision,server_time_unix_ms:time,expires_at_unix_ms:time+60000};
}
export async function vehicleRoute(request:Request,env:VehicleEnv,account:string,limits=visualDefaults,time=Date.now()):Promise<Response|null>{
  const match=new URL(request.url).pathname.match(/^\/v1\/scopes\/([^/]+)\/vehicles\/([^/]+)$/);if(!match)return null;
  if(!vehicleEnabled(env))return response({code:'ASSET_NOT_FOUND'},404);
  const scope=visualSlug(decodeURIComponent(match[1])),entity=match[2];const v=await readVisualJson(request,limits.max_visual_state_bytes);
  if(entity==='query'&&request.method==='POST'){
    visualObject(v,['world_epoch','dimension_id','entity_uuids']);const epoch=visualSlug(v.world_epoch),dimension=visualIdentifier(v.dimension_id);
    if(!Array.isArray(v.entity_uuids)||v.entity_uuids.length>limits.max_entity_query_count)fail(400,'INVALID_METADATA');
    await requireVisualContext(env,account,scope,epoch,dimension);const entries:unknown[]=[];
    for(const raw of v.entity_uuids){const id=visualUuid(raw),stored=await row(env,scope,epoch,dimension,id);if(!stored)continue;
      const binding=parseVehicle(JSON.parse(stored.binding_json),limits);
      if(!await editable(env,stored.publisher_account_id,scope,binding.target_id))continue;
      if(binding.resource&&(!await readableVisualResource(env,stored.publisher_account_id,binding.resource)||!await readableVisualResource(env,account,binding.resource)))continue;
      entries.push(entry(id,binding,stored.revision,time));}
    return response({entries});
  }
  if(request.method!=='PUT')return response({code:'ASSET_NOT_FOUND'},404);
  const id=visualUuid(entity),binding=parseVehicle(v,limits);
  await requireVisualContext(env,account,scope,binding.world_epoch,binding.dimension_id);
  if(!await editable(env,account,scope,binding.target_id))fail(403,'ASSET_ACCESS_DENIED');
  const old=await row(env,scope,binding.world_epoch,binding.dimension_id,id);
  if((old?.revision??0)!==binding.expected_revision)fail(409,'REVISION_CONFLICT');
  if(old){if(!await editable(env,account,scope,old.target_id))fail(403,'ASSET_ACCESS_DENIED');
    if(parseVehicle(JSON.parse(old.binding_json),limits).entity_kind!==binding.entity_kind)fail(409,'REVISION_CONFLICT');}
  if(binding.resource&&!await readableVisualResource(env,account,binding.resource))fail(403,'ASSET_ACCESS_DENIED');
  const r=binding.resource;
  const result=await env.DB.batch([visualBucket(env,account,limits,time),env.DB.prepare(`INSERT INTO vehicle_bindings(scope_id,world_epoch,dimension_id,entity_uuid,target_id,publisher_account_id,binding_json,revision)
    SELECT ?1,?2,?3,?4,?5,?6,?7,?8+1 WHERE changes()=1 AND ${editPredicate}
    AND EXISTS(SELECT 1 FROM scopes s WHERE s.scope_id=?1 AND s.world_epoch=?2)
    AND (?9 IS NULL OR EXISTS(SELECT 1 FROM asset_revisions r JOIN assets a ON a.asset_id=r.asset_id LEFT JOIN asset_acl acl ON acl.asset_id=a.asset_id AND acl.account_id=?6
      WHERE r.asset_id=?9 AND r.revision=?10 AND r.raw_sha256=?11 AND r.format=?12 AND (a.visibility='PUBLIC' OR acl.permission IN ('manage','render_read'))))
    AND (NOT EXISTS(SELECT 1 FROM vehicle_bindings WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4)
      OR EXISTS(SELECT 1 FROM vehicle_bindings b JOIN target_acl a ON a.target_id=b.target_id AND a.account_id=?6
        WHERE b.scope_id=?1 AND b.world_epoch=?2 AND b.dimension_id=?3 AND b.entity_uuid=?4 AND b.revision=?8
        AND json_extract(b.binding_json,'$.entity_kind')=?13 AND a.role IN ('editor','edit','manage','owner')))
    ON CONFLICT(scope_id,world_epoch,dimension_id,entity_uuid) DO UPDATE SET target_id=excluded.target_id,publisher_account_id=excluded.publisher_account_id,binding_json=excluded.binding_json,revision=excluded.revision WHERE vehicle_bindings.revision=?8`)
    .bind(scope,binding.world_epoch,binding.dimension_id,id,binding.target_id,account,JSON.stringify(binding),binding.expected_revision,r?.asset_id??null,r?.asset_revision??null,r?.raw_sha256??null,r?.format??null,binding.entity_kind)]);
  if(!result[0].meta.changes)fail(429,'RATE_LIMITED');if(!result[1].meta.changes)fail(409,'REVISION_CONFLICT');
  return response(entry(id,binding,binding.expected_revision+1,time));
}
