import { requireVisualContext, visualDefaults, type VisualEnv } from "./projectile-snapshots";

type DisplayEnv = VisualEnv & { SPM_CLOUD_PLAYER_DISPLAY_STATE?: string };
export type DisplayUpdate = {
  scope_id: string; world_epoch: string; dimension_id: string; state: Record<string, unknown>;
};
export function displayEnabled(env: DisplayEnv): boolean { return env.SPM_CLOUD_PLAYER_DISPLAY_STATE === "true"; }
function invalid(code = "INVALID_METADATA", status = 400): never {
  throw new Response(JSON.stringify({code}), {status, headers:{"content-type":"application/json", "cache-control":"no-store"}});
}
function object(value: unknown, fields?: string[]): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) invalid();
  const result = value as Record<string, unknown>;
  if (fields && Object.keys(result).some(key => !fields.includes(key))) invalid();
  return result;
}
export async function prepareDisplay(env: DisplayEnv, account: string, value: unknown): Promise<string | null> {
  if (value == null) return null;
  if (!displayEnabled(env)) invalid("PROTOCOL_UNSUPPORTED", 400);
  const dto = object(value, ["scope_id", "world_epoch", "dimension_id", "state"]);
  if (typeof dto.scope_id !== "string" || typeof dto.world_epoch !== "string" || typeof dto.dimension_id !== "string") invalid();
  await requireVisualContext(env, account, dto.scope_id, dto.world_epoch, dto.dimension_id);
  const fields = ["experience_level", "health", "max_health", "food_level", "effect_amplifiers", "flying", "strafe_input", "vertical_input", "forward_input", "shield_blocking"];
  const raw = object(dto.state, fields), state: Record<string, unknown> = {};
  for (const key of fields) {
    const value = raw[key];
    if (value == null) { state[key] = null; continue; }
    if (key === "flying" || key === "shield_blocking") {
      if (typeof value !== "boolean") invalid();
      state[key] = value;
    } else if (key === "effect_amplifiers") {
      const effects = object(value);
      if (Object.keys(effects).length > visualDefaults.max_visual_variables) invalid();
      for (const [id, amplifier] of Object.entries(effects)) {
        if (id.length > 256 || !/^[a-z0-9_.-]+:[a-z0-9_./-]+$/.test(id)
            || typeof amplifier !== "number" || !Number.isInteger(amplifier) || amplifier < 1 || amplifier > 256) invalid();
      }
      state[key] = effects;
    } else {
      if (typeof value !== "number" || !Number.isFinite(value)) invalid();
      const integer = key === "experience_level" || key === "food_level";
      const min = key.endsWith("_input") ? -1 : 0;
      const max = key.endsWith("_input") ? 1 : key === "experience_level" ? 2147483647 : key === "food_level" ? 20 : 1000000;
      const normalized = integer ? value : Math.fround(value);
      if ((integer && !Number.isInteger(value)) || normalized < min || normalized > max) invalid();
      state[key] = normalized;
    }
  }
  const json = JSON.stringify({scope_id:dto.scope_id, world_epoch:dto.world_epoch, dimension_id:dto.dimension_id, state});
  if (new TextEncoder().encode(json).length > visualDefaults.max_visual_state_bytes) invalid("MESSAGE_TOO_LARGE", 413);
  return json;
}
export async function displayForReader(env: DisplayEnv, reader: string, publisher: string,
        json: string | null, updatedAt: number, time: number): Promise<unknown> {
  if (!displayEnabled(env) || json === null) return null;
  const dto = JSON.parse(json) as DisplayUpdate;
  try {
    await requireVisualContext(env, publisher, dto.scope_id, dto.world_epoch, dto.dimension_id);
    await requireVisualContext(env, reader, dto.scope_id, dto.world_epoch, dto.dimension_id);
  } catch { return null; }
  return {...dto, server_time_unix_ms: time, expires_at_unix_ms:(updatedAt + 60)*1000};
}
