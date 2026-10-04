/** Client-only entity appearances. Public discovery never grants editing ACLs. */
type Binding = { entity_uuid: string; entity_kind: string; target_id: string; binding_revision: number };
type AppearanceRow = Binding & { revision: number; asset_id: string | null; asset_revision: number | null; raw_sha256: string | null; texture_id: string | null; disabled: number; format: string | null; allowed: number };
const headers = { "content-type": "application/json; charset=utf-8", "cache-control": "no-store", "access-control-allow-origin": "*" };
const response = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers });
function fail(status: number, code: string, message: string): never { throw response({ code, message }, status); }
function string(value: unknown, name: string, max = 256): string {
  if (typeof value !== "string" || !value.length || value.length > max || /[\x00-\x1f]/.test(value)) fail(400, "INVALID_METADATA", `${name} is invalid`);
  return value;
}
function uuid(value: unknown): string {
  const id = string(value, "entity_uuid", 36).toLowerCase();
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(id)) fail(400, "INVALID_METADATA", "entity_uuid must be canonical UUID");
  return id;
}
function integer(value: unknown, name: string, minimum: number): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < minimum || value >= Number.MAX_SAFE_INTEGER) fail(400, "INVALID_METADATA", `${name} is invalid`);
  return value;
}
async function body(request: Request): Promise<Record<string, unknown>> {
  // These requests carry metadata only, never model bytes.
  const reader = request.body?.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  if (reader) {
    try {
      while (true) {
        const next = await reader.read();
        if (next.done) break;
        size += next.value.byteLength;
        if (size > 16384) {
          await reader.cancel();
          fail(413, "MESSAGE_TOO_LARGE", "entity metadata is too large");
        }
        chunks.push(next.value);
      }
    } finally { reader.releaseLock(); }
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
  const raw = new TextDecoder().decode(bytes);
  let value: unknown;
  try { value = JSON.parse(raw); } catch { fail(400, "INVALID_METADATA", "invalid JSON"); }
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(400, "INVALID_METADATA", "JSON object required");
  return value as Record<string, unknown>;
}
async function world(env: Env, key: string, account: string, create: boolean): Promise<Response> {
  const scope = `entity_${key}`;
  if (create) await env.DB.batch([
    env.DB.prepare("INSERT OR IGNORE INTO scopes(scope_id,tenant_id,name,world_epoch,offline_policy) VALUES (?1,?2,'Shared entity world',?3,'STRICT_APPROVAL')").bind(scope, account, key),
    // changes() is evaluated in the same atomic batch: only the creator gains ACL.
    env.DB.prepare("INSERT INTO scope_acl(scope_id,account_id,role) SELECT ?1,?2,'manage' WHERE changes()=1").bind(scope, account),
  ]);
  const row = await env.DB.prepare("SELECT scope_id,name,world_epoch FROM scopes WHERE scope_id=?1 AND world_epoch=?2").bind(scope, key).first();
  if (!row) fail(404, "NOT_FOUND", "entity world not found");
  return response(row);
}
async function editor(env: Env, account: string, scope: string, target?: string): Promise<void> {
  const sql = target ? "SELECT role FROM target_acl WHERE target_id=?1 AND account_id=?2" : "SELECT role FROM scope_acl WHERE scope_id=?1 AND account_id=?2";
  const row = await env.DB.prepare(sql).bind(target ?? scope, account).first<{role: string}>();
  if (!row || !["editor", "edit", "manage", "owner"].includes(row.role)) fail(403, target ? "TARGET_ACCESS_DENIED" : "SCOPE_ACCESS_DENIED", "entity editing access denied");
}
async function binding(env: Env, key: string, id: string): Promise<Binding | null> {
  return env.DB.prepare("SELECT b.entity_uuid,b.entity_kind,b.target_id,b.revision AS binding_revision FROM entity_bindings b JOIN targets t ON t.target_id=b.target_id AND t.scope_id=b.scope_id AND t.target_kind=b.entity_kind WHERE b.scope_id=?1 AND b.world_epoch=?2 AND b.entity_uuid=?3 AND b.entity_kind IN ('MAID','FAKE_PLAYER')")
    .bind(`entity_${key}`, key, id).first<Binding>();
}
async function entries(env: Env, account: string, key: string, ids: string[]): Promise<unknown[]> {
  if (!ids.length) return [];
  // Permission predicate intentionally matches exact-revision content GET, not player presence privacy.
  const rows = await env.DB.prepare(`SELECT b.entity_uuid,b.entity_kind,b.target_id,b.revision AS binding_revision,
    p.revision,p.asset_id,p.asset_revision,p.raw_sha256,p.texture_id,p.disabled,r.format,
    CASE WHEN a.visibility='PUBLIC' OR acl.permission IN ('manage','use','render_read','discover')
      OR (sharing.target_id IS NOT NULL AND (a.owner_account_id=sharing.shared_by_account_id OR sharing_acl.permission='manage')) THEN 1 ELSE 0 END AS allowed
    FROM entity_bindings b JOIN targets t ON t.target_id=b.target_id AND t.scope_id=b.scope_id AND t.target_kind=b.entity_kind
    JOIN scopes s ON s.scope_id=b.scope_id AND s.world_epoch=b.world_epoch
    JOIN appearances p ON p.target_id=b.target_id
    LEFT JOIN assets a ON a.asset_id=p.asset_id
    LEFT JOIN asset_revisions r ON r.asset_id=p.asset_id AND r.revision=p.asset_revision AND r.raw_sha256=p.raw_sha256
    LEFT JOIN asset_acl acl ON acl.asset_id=p.asset_id AND acl.account_id=?1
    LEFT JOIN entity_model_shares sharing ON sharing.target_id=p.target_id AND sharing.asset_id=p.asset_id AND sharing.asset_revision=p.asset_revision AND sharing.raw_sha256=p.raw_sha256
    LEFT JOIN asset_acl sharing_acl ON sharing_acl.asset_id=sharing.asset_id AND sharing_acl.account_id=sharing.shared_by_account_id
    WHERE b.scope_id=?2 AND b.world_epoch=?3 AND b.entity_kind IN ('MAID','FAKE_PLAYER') AND b.entity_uuid IN (${ids.map(()=>"?").join(",")})`)
    .bind(account, `entity_${key}`, key, ...ids).all<AppearanceRow>();
  return rows.results.map(row => ({entity_uuid:row.entity_uuid,entity_kind:row.entity_kind,target_id:row.target_id,binding_revision:row.binding_revision,revision:row.revision,
    selection: row.allowed && row.format && row.asset_id && !row.disabled ? {asset_id:row.asset_id,asset_revision:row.asset_revision,raw_sha256:row.raw_sha256,format:row.format,texture_id:row.texture_id} : null}));
}
async function put(request: Request, env: Env, account: string, key: string, id: string): Promise<Response> {
  const input = await body(request);
  const kind = string(input.entity_kind, "entity_kind", 32);
  if (kind !== "MAID" && kind !== "FAKE_PLAYER") fail(400, "INVALID_METADATA", "only MAID and FAKE_PLAYER can be managed here");
  const display = string(input.display_name, "display_name");
  const expected = integer(input.expected_revision, "expected_revision", 0);
  if(input.share_model != null && typeof input.share_model !== "boolean") fail(400,"INVALID_METADATA","share_model must be boolean");
  const share = input.share_model === true;
  const scope = `entity_${key}`;
  await world(env, key, account, false);
  await editor(env, account, scope);
  const prior = await binding(env, key, id);
  if (prior && prior.entity_kind !== kind) fail(409, "ENTITY_KIND_CONFLICT", "entity kind cannot change");
  if (!prior && expected !== 0) fail(409, "REVISION_CONFLICT", "new entity revision is zero");
  if (prior) await editor(env, account, scope, prior.target_id);
  let asset: string | null = null, revision: number | null = null, sha: string | null = null, texture: string | null = null;
  if (input.asset_id != null) {
    asset = string(input.asset_id, "asset_id", 128);
    revision = integer(input.asset_revision, "asset_revision", 1);
    sha = string(input.raw_sha256, "raw_sha256", 64);
    texture = string(input.texture_id, "texture_id");
    if (!/^[0-9a-f]{64}$/.test(sha)) fail(400, "INVALID_METADATA", "raw_sha256 must be lowercase SHA256");
    const ref = await env.DB.prepare(`SELECT r.raw_sha256,r.format FROM asset_revisions r JOIN assets a ON a.asset_id=r.asset_id
      LEFT JOIN asset_acl acl ON acl.asset_id=a.asset_id AND acl.account_id=?1 WHERE r.asset_id=?2 AND r.revision=?3
      AND (a.visibility='PUBLIC' OR acl.permission IN ('manage','use','render_read','discover') OR (?4=1 AND a.owner_account_id=?1))`).bind(account,asset,revision,share?1:0).first<{raw_sha256:string;format:string}>();
    if (!ref || ref.raw_sha256 !== sha) fail(403, "ASSET_ACCESS_DENIED", "asset revision is unavailable");
    if (!["ysm","zip","bbmodel","gltf","glb"].includes(ref.format.toLowerCase())) fail(400,"INVALID_METADATA","unsupported entity asset format");
    if(share) {
      const distribution=await env.DB.prepare("SELECT a.visibility,a.owner_account_id,acl.permission FROM assets a LEFT JOIN asset_acl acl ON acl.asset_id=a.asset_id AND acl.account_id=?1 WHERE a.asset_id=?2").bind(account,asset).first<{visibility:string;owner_account_id:string;permission:string|null}>();
      if(distribution?.visibility!=="PUBLIC" && distribution?.owner_account_id!==account && distribution?.permission!=="manage") fail(403,"ASSET_SHARE_DENIED","only asset owner or manager can share a private model");
    }
  }
  const target = prior?.target_id ?? `entity_${key}_${id.replaceAll("-", "")}`;
  if (!prior) await env.DB.batch([
    env.DB.prepare("INSERT OR IGNORE INTO targets(target_id,scope_id,target_kind,display_name,owner_account_id) VALUES (?1,?2,?3,?4,?5)").bind(target,scope,kind,display,account),
    env.DB.prepare("INSERT INTO target_acl(target_id,account_id,role) SELECT ?1,?2,'manage' WHERE changes()=1").bind(target,account),
    env.DB.prepare("INSERT OR IGNORE INTO appearances(target_id) VALUES (?1)").bind(target),
    env.DB.prepare("INSERT OR IGNORE INTO entity_bindings(binding_id,scope_id,world_epoch,entity_uuid,entity_kind,target_id) VALUES (?1,?2,?3,?4,?5,?6)").bind(`binding_${key}_${id.replaceAll("-", "")}`,scope,key,id,kind,target),
  ]);
  const currentBinding = await binding(env,key,id);
  if (!currentBinding || currentBinding.entity_kind !== kind || currentBinding.target_id !== target) fail(409,"ENTITY_KIND_CONFLICT","entity binding changed");
  await editor(env,account,scope,target);
  const payload={target_id:target,revision:expected+1,asset_id:asset,asset_revision:revision,raw_sha256:sha,texture_id:texture,scale:null,disabled:false};
  const eventId=crypto.randomUUID();
  const result=await env.DB.batch([
    env.DB.prepare("UPDATE appearances SET revision=revision+1,asset_id=?1,asset_revision=?2,raw_sha256=?3,texture_id=?4,disabled=0 WHERE target_id=?5 AND revision=?6").bind(asset,revision,sha,texture,target,expected),
    env.DB.prepare("INSERT INTO outbox_events(event_id,tenant_id,scope_id,target_id,kind,payload_json) SELECT ?1,?2,?3,?4,'APPEARANCE_UPDATED',?5 WHERE changes()=1").bind(eventId,account,scope,target,JSON.stringify(payload)),
    env.DB.prepare("DELETE FROM entity_model_shares WHERE target_id=?1 AND EXISTS(SELECT 1 FROM outbox_events WHERE event_id=?2)").bind(target,eventId),
    env.DB.prepare("INSERT INTO entity_model_shares(target_id,shared_by_account_id,asset_id,asset_revision,raw_sha256) SELECT ?1,?2,?3,?4,?5 WHERE ?6=1 AND ?3 IS NOT NULL AND EXISTS(SELECT 1 FROM outbox_events WHERE event_id=?7)").bind(target,account,asset,revision,sha,share?1:0,eventId),
  ]);
  if(result[0].meta.changes!==1) fail(409,"REVISION_CONFLICT","entity appearance changed; query revision and retry");
  return response((await entries(env,account,key,[id]))[0]);
}
async function download(request:Request,env:Env,account:string,key:string,id:string):Promise<Response>{
  const url=new URL(request.url);
  const revision=integer(Number(url.searchParams.get("asset_revision")),"asset_revision",1);
  const sha=string(url.searchParams.get("raw_sha256"),"raw_sha256",64);
  if(!/^[0-9a-f]{64}$/.test(sha)) fail(400,"INVALID_METADATA","raw_sha256 must be lowercase SHA256");
  const bound=await binding(env,key,id);
  if(!bound) fail(404,"NOT_FOUND","entity binding not found");
  const current=await env.DB.prepare("SELECT asset_id,asset_revision,raw_sha256 FROM appearances WHERE target_id=?1").bind(bound.target_id).first<{asset_id:string|null;asset_revision:number|null;raw_sha256:string|null}>();
  if(!current?.asset_id || current.asset_revision!==revision || current.raw_sha256!==sha || (url.searchParams.has("asset_id")&&url.searchParams.get("asset_id")!==current.asset_id)) fail(409,"REVISION_CONFLICT","entity no longer selects this exact asset revision");
  const selected=(await entries(env,account,key,[id]))[0] as {selection:{asset_id:string;asset_revision:number;raw_sha256:string}|null}|undefined;
  if(!selected) fail(404,"NOT_FOUND","entity binding not found");
  if(!selected.selection) fail(403,"ASSET_ACCESS_DENIED","entity has no accessible model");
  const ref=selected.selection;
  if(ref.asset_revision!==revision || ref.raw_sha256!==sha || (url.searchParams.has("asset_id")&&url.searchParams.get("asset_id")!==ref.asset_id)) fail(409,"REVISION_CONFLICT","entity no longer selects this exact asset revision");
  const row=await env.DB.prepare("SELECT object_key FROM asset_revisions WHERE asset_id=?1 AND revision=?2 AND raw_sha256=?3").bind(ref.asset_id,revision,sha).first<{object_key:string}>();
  if(!row) fail(404,"NOT_FOUND","asset revision not found");
  const object=await env.ASSETS.get(row.object_key);
  if(!object) fail(404,"NOT_FOUND","asset content not found");
  return new Response(object.body,{headers:{"cache-control":"no-store","access-control-allow-origin":"*","content-type":"application/octet-stream","content-length":String(object.size),etag:`"${sha}"`}});
}
export async function entityWorldRoute(request: Request, env: Env, account: string): Promise<Response | null> {
  const match=new URL(request.url).pathname.match(/^\/v1\/entity-worlds\/([^/]+)(?:\/(appearances\/query|entities\/([^/]+)\/(appearance|asset)))?$/);
  if(!match) return null;
  const key=match[1];
  if(!/^[0-9a-f]{64}$/.test(key)) fail(400,"INVALID_METADATA","world key must be 64 lowercase hex characters");
  if(!match[2] && ["GET","POST"].includes(request.method)) return world(env,key,account,request.method==="POST");
  if(match[2]==="appearances/query" && request.method==="POST") {
    const input=await body(request);
    if(!Array.isArray(input.entity_uuids) || input.entity_uuids.length>64) fail(400,"INVALID_METADATA","entity_uuids must contain at most 64 UUIDs");
    return response({entries:await entries(env,account,key,[...new Set(input.entity_uuids.map(uuid))])});
  }
  if(match[3]) {
    const id=uuid(match[3]);
    if(match[4]==="asset") return request.method==="GET"?download(request,env,account,key,id):null;
    if(request.method==="PUT") return put(request,env,account,key,id);
    if(request.method==="GET") return response((await entries(env,account,key,[id]))[0] ?? {entity_uuid:id,entity_kind:null,target_id:null,binding_revision:0,revision:0,selection:null});
  }
  return null;
}
