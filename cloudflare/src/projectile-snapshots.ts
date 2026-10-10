/** Authenticated display leases for native client entities in an explicitly chosen scope. */
export type VisualEnv = Pick<Env, "DB"> & { SPM_CLOUD_PROJECTILE_SNAPSHOTS?: string };
export type VisualLimits = {
  max_entity_query_count: number; max_visual_state_bytes: number; max_visual_variables: number;
  max_projectile_snapshots_per_publisher: number; max_projectile_snapshots_per_world: number;
  visual_publish_requests_per_second: number; visual_publish_burst: number;
  projectile_idle_ttl_seconds: number; projectile_max_lifetime_seconds: number;
};
export const visualDefaults: VisualLimits = {
  max_entity_query_count: 64, max_visual_state_bytes: 8192, max_visual_variables: 32,
  max_projectile_snapshots_per_publisher: 256, max_projectile_snapshots_per_world: 4096,
  visual_publish_requests_per_second: 20, visual_publish_burst: 40,
  projectile_idle_ttl_seconds: 600, projectile_max_lifetime_seconds: 86400,
};
export type ResourceRef = { asset_id: string; asset_revision: number; raw_sha256: string; texture_id: string; format:string };
export type ProjectileSnapshot = {
  world_epoch: string; dimension_id: string; entity_uuid: string; entity_kind: string;
  source_identity_id: string; source_entity_uuid: string; event_id: string;
  resource: ResourceRef; projectile_bundle_key: string; variables: Record<string, number>;
  firing_item_id: string | null;
};
type Row = {
  publisher_account_id: string; snapshot_json: string; lease_revision: number;
  received_at_ms: number; expires_at_ms: number; absolute_expires_at_ms: number; withdrawn: number;
};
const headers = {"content-type":"application/json; charset=utf-8", "cache-control":"no-store", "access-control-allow-origin":"*"};
const response = (body: unknown, status = 200) => new Response(JSON.stringify(body), {status, headers});
function fail(status: number, code: string): never { throw response({ok:false, code, retryable:false, message_key:`cloud.error.${code.toLowerCase()}`}, status); }
function object(value: unknown, keys?: string[]): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(400,"INVALID_METADATA");
  const result = value as Record<string, unknown>;
  if (keys && Object.keys(result).some(k => !keys.includes(k))) fail(400,"INVALID_METADATA");
  return result;
}
function text(value: unknown, max = 256): string {
  if (typeof value !== "string" || !value.length || value.length > max || /[\x00-\x1f\x7f]/.test(value)) fail(400,"INVALID_METADATA");
  return value;
}
function slug(value: unknown): string {
  const s = text(value,128);
  if (!/^[a-zA-Z0-9][a-zA-Z0-9_.-]*$/.test(s)) fail(400,"INVALID_METADATA");
  return s;
}
function identifier(value: unknown): string {
  const s=text(value);
  if (!/^[a-z0-9_.-]+:[a-z0-9_./-]+$/.test(s)) fail(400,"INVALID_METADATA");
  return s;
}
function uuid(value: unknown): string {
  const s=text(value,36).toLowerCase();
  if (!/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(s)) fail(400,"INVALID_METADATA");
  return s;
}
function integer(value: unknown, minimum = 0): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value<minimum || value>=Number.MAX_SAFE_INTEGER) fail(400,"INVALID_METADATA");
  return value;
}
function rejectDuplicateKeys(raw: string): void {
  let i=0;
  const space=()=>{while (/\s/.test(raw[i]??"") && i<raw.length) i++;};
  function string(): string {
    const start=i++;
    while (i<raw.length) { if (raw[i]==="\\") i+=2; else if (raw[i++]==='"') break; }
    return JSON.parse(raw.slice(start,i)) as string;
  }
  function value(depth: number): void {
    if (depth>32) fail(400,"INVALID_METADATA");
    space();
    if (raw[i]==="{") {
      i++;space();const keys=new Set<string>();
      while (raw[i]!=="}") {
        space();const key=string();if (keys.has(key)) fail(400,"INVALID_METADATA");keys.add(key);
        space();i++;value(depth+1);space();if (raw[i]!==",") break;i++;
      }
      i++;
    } else if (raw[i]==="[") {
      i++;space();while(raw[i]!=="]") {value(depth+1);space();if(raw[i]!==",")break;i++;}i++;
    } else if (raw[i]==='"') string();
    else while(i<raw.length && !/[\s,}\]]/.test(raw[i])) i++;
  }
  value(0);
}
async function body(request: Request, max: number): Promise<Record<string, unknown>> {
  const reader=request.body?.getReader();const chunks:Uint8Array[]=[];let size=0;
  if(reader)try {
    while(true){const next=await reader.read();if(next.done)break;size+=next.value.byteLength;
      if(size>max){await reader.cancel();fail(413,"MESSAGE_TOO_LARGE");}chunks.push(next.value);}
  }finally{reader.releaseLock();}
  const bytes=new Uint8Array(size);let offset=0;for(const chunk of chunks){bytes.set(chunk,offset);offset+=chunk.length;}
  let raw:string;try {raw=new TextDecoder("utf-8",{fatal:true}).decode(bytes);}catch{fail(400,"INVALID_METADATA");}
  let parsed:unknown;try {parsed=JSON.parse(raw);}catch{fail(400,"INVALID_METADATA");}
  rejectDuplicateKeys(raw);return object(parsed);
}
export function parseSnapshot(value: unknown, limits: VisualLimits): ProjectileSnapshot {
  const v=object(value,["world_epoch","dimension_id","entity_uuid","entity_kind","source_identity_id","source_entity_uuid","event_id","resource","projectile_bundle_key","variables","firing_item_id"]);
  const r=object(v.resource,["asset_id","asset_revision","raw_sha256","texture_id","format"]);
  const format=text(r.format);if(!["ysm","zip","bbmodel","gltf","glb"].includes(format))fail(400,"INVALID_METADATA");
  const sha=text(r.raw_sha256,64);if(!/^[a-f0-9]{64}$/.test(sha))fail(400,"INVALID_METADATA");
  const variables=object(v.variables===undefined?{}:v.variables);if(Object.keys(variables).length>limits.max_visual_variables)fail(400,"INVALID_METADATA");
  const numbers:Record<string,number>={};for(const key of Object.keys(variables).sort()) {
    text(key,32);const n=variables[key];if(typeof n!=="number"||!Number.isFinite(n)||!Number.isFinite(Math.fround(n)))fail(400,"INVALID_METADATA");numbers[key]=n;
  }
  const result:ProjectileSnapshot={world_epoch:slug(v.world_epoch),dimension_id:identifier(v.dimension_id),entity_uuid:uuid(v.entity_uuid),entity_kind:identifier(v.entity_kind),
    source_identity_id:slug(v.source_identity_id),source_entity_uuid:uuid(v.source_entity_uuid),event_id:slug(v.event_id),
    resource:{asset_id:slug(r.asset_id),asset_revision:integer(r.asset_revision,1),raw_sha256:sha,texture_id:text(r.texture_id),format},
    projectile_bundle_key:identifier(v.projectile_bundle_key),variables:numbers,firing_item_id:v.firing_item_id==null?null:identifier(v.firing_item_id)};
  if(new TextEncoder().encode(JSON.stringify(result)).length>limits.max_visual_state_bytes)fail(413,"MESSAGE_TOO_LARGE");
  return result;
}
async function context(env:VisualEnv,account:string,scope:string,epoch:string,dimension:string):Promise<void> {
  slug(scope);slug(epoch);identifier(dimension);
  const row=await env.DB.prepare("SELECT 1 FROM scopes s JOIN scope_acl acl ON acl.scope_id=s.scope_id WHERE s.scope_id=?1 AND s.world_epoch=?2 AND acl.account_id=?3").bind(scope,epoch,account).first();
  if(!row)fail(403,"SCOPE_ACCESS_DENIED");
}
async function identity(env:VisualEnv,account:string,snapshot:ProjectileSnapshot):Promise<boolean> {
  const row=await env.DB.prepare("SELECT 1 FROM identities i JOIN identity_providers p ON p.provider_id=COALESCE(i.provider_id,'official') AND p.enabled=1 WHERE i.identity_id=?1 AND i.account_id=?2 AND i.verified=1 AND i.profile_uuid=?3 AND i.identity_kind IN ('official','yggdrasil') AND NOT EXISTS(SELECT 1 FROM player_appearances WHERE identity_id=i.identity_id AND asset_id IS NULL)").bind(snapshot.source_identity_id,account,snapshot.source_entity_uuid).first();
  return !!row;
}
async function resource(env:VisualEnv,account:string,r:ResourceRef):Promise<boolean> {
  const row=await env.DB.prepare("SELECT 1 FROM asset_revisions r JOIN assets a ON a.asset_id=r.asset_id LEFT JOIN asset_acl acl ON acl.asset_id=a.asset_id AND acl.account_id=?4 WHERE r.asset_id=?1 AND r.revision=?2 AND r.raw_sha256=?3 AND r.format=?5 AND (a.visibility='PUBLIC' OR acl.permission IN ('manage','render_read'))").bind(r.asset_id,r.asset_revision,r.raw_sha256,account,r.format).first();
  return !!row;
}
function lease(row:Row,time:number):unknown {
  return {snapshot:JSON.parse(row.snapshot_json),lease_revision:row.lease_revision,received_at_unix_ms:row.received_at_ms,expires_at_unix_ms:row.expires_at_ms,absolute_expires_at_unix_ms:row.absolute_expires_at_ms,server_time_unix_ms:time};
}
async function row(env:VisualEnv,scope:string,epoch:string,dimension:string,entity:string):Promise<Row|null> {
  return env.DB.prepare("SELECT * FROM projectile_snapshots WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4").bind(scope,epoch,dimension,entity).first<Row>();
}
function bucket(env:VisualEnv,account:string,limits:VisualLimits,time:number):D1PreparedStatement {
  return env.DB.prepare("INSERT INTO visual_publish_buckets(account_id,tokens,updated_at_ms) VALUES (?1,?2-1,?3) ON CONFLICT(account_id) DO UPDATE SET tokens=MIN(?2,tokens+MAX(0,?3-updated_at_ms)*?4/1000.0)-1,updated_at_ms=?3 WHERE MIN(?2,tokens+MAX(0,?3-updated_at_ms)*?4/1000.0)>=1").bind(account,limits.visual_publish_burst,time,limits.visual_publish_requests_per_second);
}
async function publish(env:VisualEnv,account:string,scope:string,s:ProjectileSnapshot,limits:VisualLimits,time:number):Promise<Response> {
  await context(env,account,scope,s.world_epoch,s.dimension_id);
  if(!await identity(env,account,s))fail(403,"IDENTITY_PROFILE_MISMATCH");
  if(!await resource(env,account,s.resource))fail(403,"ASSET_ACCESS_DENIED");
  const retired=await env.DB.prepare("SELECT 1 FROM projectile_tombstones WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4").bind(scope,s.world_epoch,s.dimension_id,s.entity_uuid).first();
  if(retired)fail(404,"ASSET_NOT_FOUND");
  const reused=await env.DB.prepare("SELECT 1 FROM projectile_tombstones WHERE scope_id=?1 AND world_epoch=?2 AND publisher_account_id=?3 AND event_id=?4").bind(scope,s.world_epoch,account,s.event_id).first();
  if(reused)fail(409,"IDEMPOTENCY_CONFLICT");
  const expiry=time+limits.projectile_idle_ttl_seconds*1000, absolute=time+limits.projectile_max_lifetime_seconds*1000;
  const results=await env.DB.batch([bucket(env,account,limits,time),env.DB.prepare(`INSERT OR IGNORE INTO projectile_snapshots(scope_id,world_epoch,dimension_id,entity_uuid,publisher_account_id,source_identity_id,event_id,snapshot_json,received_at_ms,expires_at_ms,absolute_expires_at_ms)
    SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11 WHERE changes()=1
    AND EXISTS(SELECT 1 FROM scopes s JOIN scope_acl acl ON acl.scope_id=s.scope_id WHERE s.scope_id=?1 AND s.world_epoch=?2 AND acl.account_id=?5)
    AND EXISTS(SELECT 1 FROM identities i JOIN identity_providers p ON p.provider_id=COALESCE(i.provider_id,'official') AND p.enabled=1 WHERE i.identity_id=?6 AND i.account_id=?5 AND i.profile_uuid=?14 AND i.verified=1 AND i.identity_kind IN ('official','yggdrasil') AND NOT EXISTS(SELECT 1 FROM player_appearances WHERE identity_id=i.identity_id AND asset_id IS NULL))
    AND EXISTS(SELECT 1 FROM asset_revisions r JOIN assets a ON a.asset_id=r.asset_id LEFT JOIN asset_acl acl ON acl.asset_id=a.asset_id AND acl.account_id=?5 WHERE r.asset_id=?15 AND r.revision=?16 AND r.raw_sha256=?17 AND r.format=?18 AND (a.visibility='PUBLIC' OR acl.permission IN ('manage','render_read')))
    AND NOT EXISTS(SELECT 1 FROM projectile_tombstones WHERE scope_id=?1 AND world_epoch=?2 AND ((dimension_id=?3 AND entity_uuid=?4) OR (publisher_account_id=?5 AND event_id=?7)))
    AND (SELECT COUNT(*) FROM projectile_snapshots WHERE publisher_account_id=?5 AND withdrawn=0 AND expires_at_ms>?9)<?12
    AND (SELECT COUNT(*) FROM projectile_snapshots WHERE scope_id=?1 AND world_epoch=?2 AND withdrawn=0 AND expires_at_ms>?9)<?13`)
    .bind(scope,s.world_epoch,s.dimension_id,s.entity_uuid,account,s.source_identity_id,s.event_id,JSON.stringify(s),time,Math.min(expiry,absolute),absolute,limits.max_projectile_snapshots_per_publisher,limits.max_projectile_snapshots_per_world,s.source_entity_uuid,s.resource.asset_id,s.resource.asset_revision,s.resource.raw_sha256,s.resource.format)]);
  if(!results[0].meta.changes)fail(429,"RATE_LIMITED");
  const stored=await row(env,scope,s.world_epoch,s.dimension_id,s.entity_uuid);
  if(stored) {
    if(stored.publisher_account_id!==account||JSON.stringify(parseSnapshot(JSON.parse(stored.snapshot_json),limits))!==JSON.stringify(s))fail(409,"IDEMPOTENCY_CONFLICT");
    return response(lease(stored,time));
  }
  const duplicate=await env.DB.prepare("SELECT 1 FROM projectile_snapshots WHERE scope_id=?1 AND world_epoch=?2 AND publisher_account_id=?3 AND event_id=?4").bind(scope,s.world_epoch,account,s.event_id).first();
  fail(duplicate?409:429,duplicate?"IDEMPOTENCY_CONFLICT":"RATE_LIMITED");
}
export async function projectileRoute(request:Request,env:VisualEnv,account:string,limits:VisualLimits=visualDefaults,time=Date.now()):Promise<Response|null> {
  const path=new URL(request.url).pathname;
  const match=path.match(/^\/v1\/scopes\/([^/]+)\/projectiles(?:\/(query)|\/([^/]+)\/lease)?$/);
  if(!match)return null;
  if(env.SPM_CLOUD_PROJECTILE_SNAPSHOTS!=="true")return response({code:"ASSET_NOT_FOUND"},404);
  const scope=decodeURIComponent(match[1]);
  const v=await body(request,limits.max_visual_state_bytes);
  if(!match[2]&&!match[3]&&request.method==="PUT")return publish(env,account,scope,parseSnapshot(v,limits),limits,time);
  if(match[2]&&request.method==="POST") {
    object(v,["world_epoch","dimension_id","entity_uuids"]);
    const epoch=slug(v.world_epoch),dimension=identifier(v.dimension_id);
    if(!Array.isArray(v.entity_uuids)||v.entity_uuids.length>limits.max_entity_query_count)fail(400,"INVALID_METADATA");
    await context(env,account,scope,epoch,dimension);
    const entries:unknown[]=[];
    for(const id of v.entity_uuids) {
      const stored=await row(env,scope,epoch,dimension,uuid(id));if(!stored||stored.withdrawn||stored.expires_at_ms<=time||stored.absolute_expires_at_ms<=time)continue;
      const snapshot=parseSnapshot(JSON.parse(stored.snapshot_json),limits);
      const publisherMember=await env.DB.prepare("SELECT 1 FROM scope_acl WHERE scope_id=?1 AND account_id=?2").bind(scope,stored.publisher_account_id).first();
      if(publisherMember&&await identity(env,stored.publisher_account_id,snapshot)&&await resource(env,stored.publisher_account_id,snapshot.resource)&&await resource(env,account,snapshot.resource))entries.push(lease(stored,time));
    }
    return response({entries});
  }
  if(match[3]&&["PUT","DELETE"].includes(request.method)) {
    object(v,["world_epoch","dimension_id","event_id","expected_revision"]);
    const epoch=slug(v.world_epoch),dimension=identifier(v.dimension_id),event=slug(v.event_id),revision=integer(v.expected_revision),entity=uuid(match[3]);
    await context(env,account,scope,epoch,dimension);
    const stored=await row(env,scope,epoch,dimension,entity);if(!stored)fail(404,"ASSET_NOT_FOUND");
    if(stored.publisher_account_id!==account)fail(403,"ASSET_ACCESS_DENIED");
    const s=parseSnapshot(JSON.parse(stored.snapshot_json),limits);
    if(s.event_id!==event||stored.lease_revision!==revision)fail(409,"REVISION_CONFLICT");
    const withdraw=request.method==="DELETE";
    if(!withdraw) {
      if(stored.withdrawn||stored.expires_at_ms<=time||stored.absolute_expires_at_ms<=time)fail(404,"ASSET_NOT_FOUND");
      if(!await identity(env,account,s))fail(403,"IDENTITY_PROFILE_MISMATCH");
      if(!await resource(env,account,s.resource))fail(403,"ASSET_ACCESS_DENIED");
    }
    const expiry=withdraw?time:Math.min(time+limits.projectile_idle_ttl_seconds*1000,stored.absolute_expires_at_ms);
    const results=await env.DB.batch([bucket(env,account,limits,time),env.DB.prepare(`UPDATE projectile_snapshots SET lease_revision=lease_revision+1,expires_at_ms=?5,withdrawn=?6 WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4 AND publisher_account_id=?7 AND lease_revision=?8 AND changes()=1
      AND EXISTS(SELECT 1 FROM scopes s JOIN scope_acl acl ON acl.scope_id=s.scope_id WHERE s.scope_id=?1 AND s.world_epoch=?2 AND acl.account_id=?7)
      AND (?6=1 OR (withdrawn=0 AND expires_at_ms>?9 AND absolute_expires_at_ms>?9
        AND EXISTS(SELECT 1 FROM identities i JOIN identity_providers p ON p.provider_id=COALESCE(i.provider_id,'official') AND p.enabled=1 WHERE i.identity_id=source_identity_id AND i.account_id=?7 AND i.profile_uuid=json_extract(snapshot_json,'$.source_entity_uuid') AND i.verified=1 AND i.identity_kind IN ('official','yggdrasil') AND NOT EXISTS(SELECT 1 FROM player_appearances WHERE identity_id=i.identity_id AND asset_id IS NULL))
        AND EXISTS(SELECT 1 FROM asset_revisions r JOIN assets a ON a.asset_id=r.asset_id LEFT JOIN asset_acl acl ON acl.asset_id=a.asset_id AND acl.account_id=?7 WHERE r.asset_id=json_extract(snapshot_json,'$.resource.asset_id') AND r.revision=json_extract(snapshot_json,'$.resource.asset_revision') AND r.raw_sha256=json_extract(snapshot_json,'$.resource.raw_sha256') AND r.format=json_extract(snapshot_json,'$.resource.format') AND (a.visibility='PUBLIC' OR acl.permission IN ('manage','render_read')))))`)
      .bind(scope,epoch,dimension,entity,expiry,withdraw?1:0,account,revision,time)]);
    if(!results[0].meta.changes)fail(429,"RATE_LIMITED");if(!results[1].meta.changes)fail(409,"REVISION_CONFLICT");
    const updated=await row(env,scope,epoch,dimension,entity);if(!updated)fail(404,"ASSET_NOT_FOUND");
    return response(lease(updated,time));
  }
  return response({code:"ASSET_NOT_FOUND"},404);
}
export { body as readVisualJson, context as requireVisualContext, bucket as visualBucket,
  object as visualObject, slug as visualSlug, identifier as visualIdentifier, uuid as visualUuid,
  integer as visualInteger, text as visualText, resource as readableVisualResource };

/** Compact replay identifiers live until the administrator rotates the scope epoch. */
export async function cleanupVisualStates(env:VisualEnv,time=Date.now()):Promise<void> {
  await env.DB.batch([
    env.DB.prepare("INSERT OR IGNORE INTO projectile_tombstones(scope_id,world_epoch,dimension_id,entity_uuid,publisher_account_id,event_id) SELECT scope_id,world_epoch,dimension_id,entity_uuid,publisher_account_id,event_id FROM projectile_snapshots WHERE withdrawn=1 OR expires_at_ms<=?1 OR absolute_expires_at_ms<=?1").bind(time),
    env.DB.prepare("DELETE FROM projectile_snapshots WHERE withdrawn=1 OR expires_at_ms<=?1 OR absolute_expires_at_ms<=?1 OR NOT EXISTS(SELECT 1 FROM scopes s WHERE s.scope_id=projectile_snapshots.scope_id AND s.world_epoch=projectile_snapshots.world_epoch)").bind(time),
    env.DB.prepare("DELETE FROM projectile_tombstones WHERE NOT EXISTS(SELECT 1 FROM scopes s WHERE s.scope_id=projectile_tombstones.scope_id AND s.world_epoch=projectile_tombstones.world_epoch)"),
    env.DB.prepare("DELETE FROM vehicle_bindings WHERE NOT EXISTS(SELECT 1 FROM scopes s WHERE s.scope_id=vehicle_bindings.scope_id AND s.world_epoch=vehicle_bindings.world_epoch)"),
    env.DB.prepare("UPDATE entity_motion_states SET update_json=NULL,resource_json=NULL WHERE expires_at_ms<=?1").bind(time),
    env.DB.prepare("DELETE FROM entity_motion_states WHERE NOT EXISTS(SELECT 1 FROM scopes s WHERE s.scope_id=entity_motion_states.scope_id AND s.world_epoch=entity_motion_states.world_epoch)"),
    env.DB.prepare("UPDATE player_display_states SET display_json=NULL WHERE identity_id IN (SELECT identity_id FROM player_appearances WHERE asset_id IS NULL OR updated_at<=?1)").bind(Math.floor(time/1000)-60),
  ]);
}
