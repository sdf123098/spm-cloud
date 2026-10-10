use crate::{
    CloudStore,
    api::{AppState, authenticate},
    error::CloudError,
};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct PlayerIdentityQuery {
    pub identity_id: String,
}
#[derive(Deserialize)]
pub struct PlayerAppearanceUpdate {
    pub identity_id: String,
    pub entity_uuid: uuid::Uuid,
    pub expected_revision: u64,
    pub asset_id: Option<String>,
    pub asset_revision: Option<u64>,
    pub raw_sha256: Option<String>,
    pub texture_id: Option<String>,
    pub profile_name_proof: Option<serde_json::Value>,
    pub motion: Option<serde_json::Value>,
    pub display_state: Option<crate::display::PlayerDisplayUpdate>,
}
#[derive(Deserialize)]
pub struct PlayerQuery {
    pub entity_uuids: Vec<uuid::Uuid>,
}
#[derive(Serialize)]
pub struct PlayerSelection {
    asset_id: String,
    asset_revision: u64,
    raw_sha256: String,
    format: String,
    texture_id: String,
    motion: Option<serde_json::Value>,
    display_state: Option<serde_json::Value>,
}
#[derive(Serialize)]
pub struct PlayerAppearance {
    entity_uuid: uuid::Uuid,
    revision: u64,
    selection: Option<PlayerSelection>,
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PlayerIdentityQuery>,
) -> Result<Json<serde_json::Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(
        serde_json::json!({"revision": state.store.player_revision(&account, &query.identity_id)?}),
    ))
}
pub async fn put(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<PlayerAppearanceUpdate>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<serde_json::Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    let Json(body) = payload.map_err(|error| {
        if error.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
            CloudError::MessageTooLarge
        } else {
            CloudError::invalid_metadata("invalid player appearance JSON")
        }
    })?;
    let (profile, _, kind) = {
        let conn = state
            .store
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        identity(&conn, &account, &body.identity_id)?
    };
    let name = if body.entity_uuid.to_string() != profile && kind == "official" {
        let proof = body
            .profile_name_proof
            .as_ref()
            .ok_or(CloudError::IdentityProfileMismatch)?;
        Some(
            crate::profile_proof::official_name(
                proof,
                uuid::Uuid::parse_str(&profile).map_err(|_| CloudError::IdentityProfileMismatch)?,
            )
            .await
            .ok_or(CloudError::IdentityProfileMismatch)?,
        )
    } else {
        None
    };
    Ok(Json(
        serde_json::json!({"revision": state.store.publish_player_with_name(&account, &body, name.as_deref())?}),
    ))
}
pub async fn query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PlayerQuery>,
) -> Result<Json<serde_json::Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(
        serde_json::json!({"entries": state.store.query_players(&account, &body.entity_uuids)?}),
    ))
}

fn identity(
    conn: &rusqlite::Connection,
    account: &str,
    id: &str,
) -> Result<(String, Option<String>, String), CloudError> {
    conn.query_row("SELECT i.profile_uuid, n.name, i.identity_kind FROM identities i LEFT JOIN player_canonical_names n ON n.identity_id=i.identity_id
        JOIN identity_providers provider ON provider.provider_id=COALESCE(i.provider_id,'official') AND provider.enabled=1
        WHERE i.identity_id=?1 AND i.account_id=?2 AND i.verified=1 AND i.identity_kind IN ('official','yggdrasil')",
        params![id,account], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?.ok_or(CloudError::IdentityNotVerified)
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn motion_text(value: Option<&serde_json::Value>, max: usize, allow_empty: bool) -> bool {
    value
        .and_then(serde_json::Value::as_str)
        .is_some_and(|s| (allow_empty || !s.is_empty()) && s.encode_utf16().count() <= max)
}

fn motion_time(value: Option<&serde_json::Value>) -> bool {
    value.and_then(serde_json::Value::as_f64).is_some_and(|n| {
        n.is_finite() && (0.0..=9_007_199_254_740_991.0).contains(&n) && n.fract() == 0.0
    })
}

fn motion_numbers(value: &serde_json::Value, key_limit: usize) -> bool {
    value.as_object().is_some_and(|map| {
        map.len() <= 64
            && map.iter().all(|(key, value)| {
                !key.is_empty()
                    && key.encode_utf16().count() <= key_limit
                    && value.as_f64().is_some_and(f64::is_finite)
            })
    })
}

fn validate_motion(value: &serde_json::Value) -> Result<(), CloudError> {
    let invalid = || CloudError::invalid_metadata("invalid player motion");
    if serde_json::to_vec(value).map_err(|_| invalid())?.len() > 65_536 {
        return Err(invalid());
    }
    let object = value.as_object().ok_or_else(invalid)?;
    if !motion_text(object.get("event_id"), 64, false)
        || !motion_text(object.get("animation_key"), 256, true)
        || !motion_time(object.get("started_at_unix_ms"))
    {
        return Err(invalid());
    }
    if object
        .get("roaming")
        .is_some_and(|roaming| !motion_numbers(roaming, 32))
    {
        return Err(invalid());
    }
    if let Some(expressions) = object.get("expressions") {
        let expressions = expressions.as_array().ok_or_else(invalid)?;
        if expressions.len() > 16 {
            return Err(invalid());
        }
        for expression in expressions {
            let expression = expression.as_object().ok_or_else(invalid)?;
            if !motion_text(expression.get("event_id"), 64, false)
                || !motion_time(expression.get("started_at_unix_ms"))
                || !motion_text(expression.get("expression"), 2048, true)
                || !expression
                    .get("values")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|values| {
                        values.len() <= 16
                            && values
                                .iter()
                                .all(|v| v.as_f64().is_some_and(f64::is_finite))
                    })
            {
                return Err(invalid());
            }
        }
    }
    if let Some(controllers) = object.get("controllers") {
        let controllers = controllers.as_object().ok_or_else(invalid)?;
        if controllers.len() > 64 {
            return Err(invalid());
        }
        for (key, controller) in controllers {
            let controller = controller.as_object().ok_or_else(invalid)?;
            if key.is_empty()
                || key.encode_utf16().count() > 128
                || !motion_text(controller.get("state"), 128, true)
                || !motion_time(controller.get("started_at_unix_ms"))
                || !controller
                    .get("variables")
                    .is_some_and(|value| motion_numbers(value, 64))
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

impl CloudStore {
    pub fn player_revision(&self, account: &str, id: &str) -> Result<u64, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        identity(&conn, account, id)?;
        Ok(conn
            .query_row(
                "SELECT revision FROM player_appearances WHERE identity_id=?1",
                [id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0) as u64)
    }
    pub fn publish_player(
        &self,
        account: &str,
        input: &PlayerAppearanceUpdate,
    ) -> Result<u64, CloudError> {
        self.publish_player_with_name(account, input, None)
    }
    fn publish_player_with_name(
        &self,
        account: &str,
        input: &PlayerAppearanceUpdate,
        verified_name: Option<&str>,
    ) -> Result<u64, CloudError> {
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        let (profile, name, kind) = identity(&tx, account, &input.identity_id)?;
        let name = if kind == "official" {
            verified_name
        } else {
            name.as_deref()
        };
        if input.entity_uuid.to_string() != profile
            && name.map(offline_uuid) != Some(input.entity_uuid)
        {
            return Err(CloudError::IdentityProfileMismatch);
        }
        if input.expected_revision >= i64::MAX as u64 {
            return Err(CloudError::invalid_metadata("invalid revision"));
        }
        if let Some(motion) = &input.motion {
            validate_motion(motion)?;
        }
        if let Some(display) = &input.display_state {
            if !self.visual.features.player_display_state {
                return Err(CloudError::ProtocolUnsupported);
            }
            crate::display::validate(display, &self.visual)?;
            crate::display::context(
                &tx,
                account,
                &display.scope_id,
                &display.world_epoch,
                &display.dimension_id,
            )?;
            crate::visual::rate(&tx, account, &self.visual, now() * 1000)?;
        }
        if let Some(id) = &input.asset_id {
            let allowed: Option<(String, String)>=tx.query_row("SELECT r.raw_sha256,r.format FROM asset_revisions r JOIN assets a ON a.asset_id=r.asset_id
                LEFT JOIN asset_acl acl ON acl.asset_id=a.asset_id AND acl.account_id=?1
                WHERE r.asset_id=?2 AND r.revision=?3 AND (a.visibility='PUBLIC' OR a.owner_account_id=?1 OR acl.permission IN ('manage','use','render_read'))",
                params![account,id,input.asset_revision.map(|v| v as i64)],|row| Ok((row.get(0)?,row.get(1)?))).optional()?;
            let (sha, format) = allowed.ok_or(CloudError::AccessDenied)?;
            if input.raw_sha256.as_ref() != Some(&sha) {
                return Err(CloudError::AccessDenied);
            }
            if !matches!(
                format.to_ascii_lowercase().as_str(),
                "ysm" | "zip" | "bbmodel" | "gltf" | "glb"
            ) {
                return Err(CloudError::invalid_metadata(
                    "appearance asset format is unsupported",
                ));
            }
            if input.texture_id.as_ref().is_none_or(|s| {
                s.is_empty() || s.chars().count() > 256 || s.chars().any(char::is_control)
            }) {
                return Err(CloudError::invalid_metadata("invalid texture"));
            }
        }
        let revision = tx
            .query_row(
                "SELECT revision FROM player_appearances WHERE identity_id=?1",
                [&input.identity_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0) as u64;
        if revision != input.expected_revision {
            return Err(CloudError::RevisionConflict);
        }
        let next = revision + 1;
        let asset_revision = if input.asset_id.is_some() {
            input.asset_revision
        } else {
            None
        };
        let sha = if input.asset_id.is_some() {
            input.raw_sha256.as_deref()
        } else {
            None
        };
        let texture = if input.asset_id.is_some() {
            input.texture_id.as_deref()
        } else {
            None
        };
        tx.execute("INSERT INTO player_appearances(identity_id,entity_uuid,revision,asset_id,asset_revision,raw_sha256,texture_id,updated_at)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(identity_id) DO UPDATE SET entity_uuid=excluded.entity_uuid,
            revision=excluded.revision,asset_id=excluded.asset_id,asset_revision=excluded.asset_revision,raw_sha256=excluded.raw_sha256,
            texture_id=excluded.texture_id,updated_at=excluded.updated_at",params![input.identity_id,input.entity_uuid.to_string(),next as i64,input.asset_id,asset_revision.map(|v| v as i64),sha,texture,now()])?;
        if let Some(motion) = input.motion.as_ref().filter(|_| input.asset_id.is_some()) {
            let json = serde_json::to_string(motion)
                .map_err(|_| CloudError::invalid_metadata("invalid player motion"))?;
            tx.execute("INSERT INTO player_motion(identity_id,motion_json) VALUES (?1,?2) ON CONFLICT(identity_id) DO UPDATE SET motion_json=excluded.motion_json", params![input.identity_id,json])?;
        } else {
            tx.execute(
                "DELETE FROM player_motion WHERE identity_id=?1",
                [&input.identity_id],
            )?;
        }
        if input.asset_id.is_none() {
            tx.execute("UPDATE projectile_snapshots SET withdrawn=1,expires_at_ms=?2 WHERE source_identity_id=?1",params![input.identity_id,now()*1000])?;
        }
        if let Some(display) = input
            .display_state
            .as_ref()
            .filter(|_| input.asset_id.is_some())
        {
            let text = serde_json::to_string(display)
                .map_err(|_| CloudError::invalid_metadata("invalid display JSON"))?;
            tx.execute("INSERT INTO player_display_states(identity_id,display_json) VALUES (?1,?2) ON CONFLICT(identity_id) DO UPDATE SET display_json=excluded.display_json",params![input.identity_id,text])?;
        } else {
            tx.execute(
                "DELETE FROM player_display_states WHERE identity_id=?1",
                [&input.identity_id],
            )?;
        }
        tx.commit()?;
        Ok(next)
    }
    pub fn query_players(
        &self,
        account: &str,
        ids: &[uuid::Uuid],
    ) -> Result<Vec<PlayerAppearance>, CloudError> {
        if ids.len() > 64 {
            return Err(CloudError::invalid_metadata(
                "player batches are limited to 64 UUIDs",
            ));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let mut statement=conn.prepare("SELECT p.revision,p.asset_id,p.asset_revision,p.raw_sha256,p.texture_id,r.format,a.visibility,a.owner_account_id,m.motion_json,d.display_json,p.updated_at,i.account_id
            FROM player_appearances p JOIN identities i ON i.identity_id=p.identity_id AND i.verified=1
            JOIN identity_providers provider ON provider.provider_id=COALESCE(i.provider_id,'official') AND provider.enabled=1
            LEFT JOIN assets a ON a.asset_id=p.asset_id LEFT JOIN asset_revisions r ON r.asset_id=p.asset_id AND r.revision=p.asset_revision AND r.raw_sha256=p.raw_sha256
            LEFT JOIN player_motion m ON m.identity_id=p.identity_id
            LEFT JOIN player_display_states d ON d.identity_id=p.identity_id
            WHERE p.entity_uuid=?1 AND p.updated_at>?2")?;
        let mut result = Vec::new();
        for id in ids {
            let rows = statement
                .query_map(params![id.to_string(), now() - 60], |row| {
                    let visibility: Option<String> = row.get(6)?;
                    let asset: Option<String> = row.get(1)?;
                    let format: Option<String> = row.get(5)?;
                    let selection = if let (Some("PUBLIC"), Some(asset), Some(format)) =
                        (visibility.as_deref(), asset, format)
                    {
                        Some(PlayerSelection {
                            asset_id: asset,
                            asset_revision: row.get::<_, i64>(2)? as u64,
                            raw_sha256: row.get(3)?,
                            texture_id: row.get(4)?,
                            format,
                            motion: row
                                .get::<_, Option<String>>(8)?
                                .and_then(|json| serde_json::from_str(&json).ok()),
                            display_state: row
                                .get::<_, Option<String>>(9)?
                                .filter(|_| self.visual.features.player_display_state)
                                .and_then(|text| {
                                    let display: crate::display::PlayerDisplayUpdate =
                                        serde_json::from_str(&text).ok()?;
                                    let publisher: String = row.get(11).ok()?;
                                    crate::display::context(
                                        &conn,
                                        &publisher,
                                        &display.scope_id,
                                        &display.world_epoch,
                                        &display.dimension_id,
                                    )
                                    .ok()?;
                                    crate::display::context(
                                        &conn,
                                        account,
                                        &display.scope_id,
                                        &display.world_epoch,
                                        &display.dimension_id,
                                    )
                                    .ok()?;
                                    let mut value = serde_json::to_value(display).ok()?;
                                    value["server_time_unix_ms"] = serde_json::json!(now() * 1000);
                                    value["expires_at_unix_ms"] = serde_json::json!(
                                        (row.get::<_, i64>(10).ok()? + 60) * 1000
                                    );
                                    Some(value)
                                }),
                        })
                    } else {
                        None
                    };
                    Ok((row.get::<_, i64>(0)? as u64, selection))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let (revision, selection) = if rows.len() == 1 {
                rows.into_iter().next().unwrap()
            } else {
                (0, None)
            };
            result.push(PlayerAppearance {
                entity_uuid: *id,
                revision,
                selection,
            });
        }
        Ok(result)
    }
}

pub fn offline_uuid(name: &str) -> uuid::Uuid {
    let input = format!("OfflinePlayer:{name}").into_bytes();
    let mut bytes = vec![0u8; (input.len() + 9).div_ceil(64) * 64];
    bytes[..input.len()].copy_from_slice(&input);
    bytes[input.len()] = 128;
    let len = bytes.len();
    bytes[len - 8..len].copy_from_slice(&((input.len() as u64) * 8).to_le_bytes());
    let shifts = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];
    let mut state = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for chunk in bytes.chunks_exact(64) {
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64usize {
            let round = i / 16;
            let (f, g) = match round {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let k = (((i + 1) as f64).sin().abs() * 4294967296.0).floor() as u32;
            let sum = a
                .wrapping_add(f)
                .wrapping_add(k)
                .wrapping_add(u32::from_le_bytes(
                    chunk[g * 4..g * 4 + 4].try_into().unwrap(),
                ));
            (a, b, c, d) = (
                d,
                b.wrapping_add(sum.rotate_left(shifts[round * 4 + i % 4])),
                b,
                c,
            );
        }
        for (s, v) in state.iter_mut().zip([a, b, c, d]) {
            *s = s.wrapping_add(v);
        }
    }
    let mut out = [0u8; 16];
    for (i, s) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&s.to_le_bytes());
    }
    out[6] = (out[6] & 15) | 48;
    out[8] = (out[8] & 63) | 128;
    uuid::Uuid::from_bytes(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_player_motion_is_visible_to_another_account() {
        let (_dir, store) = setup();
        let profile = uuid::Uuid::new_v4();
        {
            let conn = store.connection.lock().unwrap();
            conn.execute("INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('motion-player','account_local','official',?1,'Player',1)", [profile.to_string()]).unwrap();
            conn.execute("INSERT INTO assets(asset_id,owner_account_id,current_revision,visibility) VALUES ('motion-model','account_local',1,'PUBLIC')", []).unwrap();
            conn.execute("INSERT INTO asset_revisions(asset_id,revision,name,format,raw_sha256,byte_length,object_path,created_at) VALUES ('motion-model',1,'model','ysm','sha',5,'object','now')", []).unwrap();
        }
        let motion = serde_json::json!({
            "event_id":"wheel-1", "animation_key":"芙宁娜.轮盘动作", "started_at_unix_ms":1720000000000u64,
            "roaming":{"idle_variant":2},
            "expressions":[{"event_id":"setting-1","started_at_unix_ms":1720000000000u64,"expression":"variable.帽子 = 1;","values":[1]}],
            "controllers":{"controller.animation.待机":{"state":"音乐","started_at_unix_ms":1720000000000u64,"variables":{"variable.idle":2}}}
        });
        let body: PlayerAppearanceUpdate = serde_json::from_value(serde_json::json!({
            "identity_id":"motion-player","entity_uuid":profile,"expected_revision":0,
            "asset_id":"motion-model","asset_revision":1,"raw_sha256":"sha","texture_id":"中文配音","motion":motion
        })).unwrap();
        store.publish_player("account_local", &body).unwrap();
        let observed =
            serde_json::to_value(store.query_players("another-account", &[profile]).unwrap())
                .unwrap();
        assert_eq!(
            observed[0]["selection"]["motion"], motion,
            "Cloud must carry wheel, model settings and idle controller state to observers"
        );
    }
    #[test]
    fn invalid_motion_is_rejected_before_mutating_player_revision() {
        let (_dir, store) = setup();
        let profile = uuid::Uuid::new_v4();
        store.connection.lock().unwrap().execute("INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('invalid-motion','account_local','official',?1,'Player',1)", [profile.to_string()]).unwrap();
        let body: PlayerAppearanceUpdate = serde_json::from_value(serde_json::json!({
            "identity_id":"invalid-motion","entity_uuid":profile,"expected_revision":0,
            "motion":{"event_id":"wheel","animation_key":"dance","started_at_unix_ms":-1}
        }))
        .unwrap();
        assert!(matches!(
            store.publish_player("account_local", &body),
            Err(CloudError::InvalidMetadata(_))
        ));
        assert_eq!(
            store
                .player_revision("account_local", "invalid-motion")
                .unwrap(),
            0
        );
    }

    fn motion_fixture() -> (
        tempfile::TempDir,
        CloudStore,
        uuid::Uuid,
        PlayerAppearanceUpdate,
    ) {
        let (dir, store) = setup();
        let profile = uuid::Uuid::new_v4();
        {
            let conn = store.connection.lock().unwrap();
            conn.execute("INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('motion','account_local','official',?1,'Player',1)", [profile.to_string()]).unwrap();
            conn.execute("INSERT INTO assets(asset_id,owner_account_id,current_revision,visibility) VALUES ('motion-model','account_local',1,'PUBLIC')", []).unwrap();
            conn.execute("INSERT INTO asset_revisions(asset_id,revision,name,format,raw_sha256,byte_length,object_path,created_at) VALUES ('motion-model',1,'model','ysm','sha',5,'object','now')", []).unwrap();
        }
        let body = serde_json::from_value(serde_json::json!({
            "identity_id":"motion","entity_uuid":profile,"expected_revision":0,
            "asset_id":"motion-model","asset_revision":1,"raw_sha256":"sha","texture_id":"中文配音",
            "motion":{"event_id":"play","animation_key":"轮盘","started_at_unix_ms":0}
        }))
        .unwrap();
        (dir, store, profile, body)
    }

    fn observed_motion(store: &CloudStore, profile: uuid::Uuid) -> serde_json::Value {
        serde_json::to_value(store.query_players("observer", &[profile]).unwrap()).unwrap()[0]["selection"]["motion"].clone()
    }

    #[test]
    fn motion_stop_cas_old_clients_and_independent_players() {
        let (_dir, store, first, mut body) = motion_fixture();
        let second = uuid::Uuid::new_v4();
        store.connection.lock().unwrap().execute("INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('second','account_local','official',?1,'Second',1)", [second.to_string()]).unwrap();
        store.publish_player("account_local", &body).unwrap();
        let original = observed_motion(&store, first);
        let mut other: PlayerAppearanceUpdate = serde_json::from_value(serde_json::json!({
            "identity_id":"second","entity_uuid":second,"expected_revision":0,
            "asset_id":"motion-model","asset_revision":1,"raw_sha256":"sha","texture_id":"other",
            "motion":{"event_id":"second","animation_key":"待机舞蹈","started_at_unix_ms":123}
        }))
        .unwrap();
        store.publish_player("account_local", &other).unwrap();
        let independent = observed_motion(&store, second);
        body.motion =
            Some(serde_json::json!({"event_id":"stop","animation_key":"","started_at_unix_ms":0}));
        assert!(matches!(
            store.publish_player("account_local", &body),
            Err(CloudError::RevisionConflict)
        ));
        assert_eq!(observed_motion(&store, first), original);
        body.expected_revision = 1;
        store.publish_player("account_local", &body).unwrap();
        assert_eq!(observed_motion(&store, first), body.motion.clone().unwrap());
        assert_eq!(observed_motion(&store, second), independent);
        body.expected_revision = 2;
        body.motion = None;
        store.publish_player("account_local", &body).unwrap();
        assert!(
            observed_motion(&store, first).is_null(),
            "old clients must clear stale motion"
        );
        let selected =
            serde_json::to_value(store.query_players("observer", &[first]).unwrap()).unwrap();
        assert_eq!(
            selected[0]["selection"].get("motion"),
            Some(&serde_json::Value::Null),
            "empty motion must have the same null response shape as Worker"
        );
        other.expected_revision = 1;
        other.motion = None;
        store.publish_player("account_local", &other).unwrap();
        assert!(observed_motion(&store, second).is_null());
    }

    #[test]
    fn motion_is_hidden_with_private_expired_unverified_or_missing_models() {
        let (_dir, store, profile, mut body) = motion_fixture();
        store.publish_player("account_local", &body).unwrap();
        {
            let conn = store.connection.lock().unwrap();
            conn.execute("UPDATE assets SET visibility='PRIVATE'", [])
                .unwrap();
        }
        assert!(observed_motion(&store, profile).is_null());
        let own_query =
            serde_json::to_value(store.query_players("account_local", &[profile]).unwrap())
                .unwrap();
        assert!(own_query[0]["selection"].is_null());
        {
            let conn = store.connection.lock().unwrap();
            conn.execute("UPDATE assets SET visibility='PUBLIC'", [])
                .unwrap();
            conn.execute("UPDATE player_appearances SET updated_at=0", [])
                .unwrap();
        }
        assert!(observed_motion(&store, profile).is_null());
        body.expected_revision = 1;
        store.publish_player("account_local", &body).unwrap();
        {
            let conn = store.connection.lock().unwrap();
            conn.execute(
                "UPDATE identities SET verified=0 WHERE identity_id='motion'",
                [],
            )
            .unwrap();
        }
        assert!(observed_motion(&store, profile).is_null());
        {
            let conn = store.connection.lock().unwrap();
            conn.execute(
                "UPDATE identities SET verified=1 WHERE identity_id='motion'",
                [],
            )
            .unwrap();
            conn.execute("UPDATE asset_revisions SET raw_sha256='changed'", [])
                .unwrap();
        }
        assert!(
            observed_motion(&store, profile).is_null(),
            "motion must stay bound to the published revision hash"
        );
        body.asset_id = None;
        body.expected_revision = 2;
        store.publish_player("account_local", &body).unwrap();
        assert!(observed_motion(&store, profile).is_null());
        assert_eq!(
            store
                .connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM player_motion WHERE identity_id='motion'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0,
            "clearing the model must delete its motion even if the request supplies an old motion object"
        );
    }

    #[test]
    fn failed_motion_write_rolls_back_player_appearance_cas() {
        let (_dir, store, profile, body) = motion_fixture();
        store.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_motion BEFORE INSERT ON player_motion BEGIN SELECT RAISE(ABORT, 'test atomic rollback'); END;").unwrap();
        assert!(matches!(
            store.publish_player("account_local", &body),
            Err(CloudError::Sqlite(_))
        ));
        assert_eq!(store.player_revision("account_local", "motion").unwrap(), 0);
        assert!(observed_motion(&store, profile).is_null());
    }

    #[test]
    fn motion_payload_bounds_and_types_are_enforced() {
        let (_dir, store, _profile, mut body) = motion_fixture();
        let valid = body.motion.clone().unwrap();
        let mut invalid = vec![
            serde_json::json!("motion"),
            serde_json::json!({}),
            serde_json::json!([]),
        ];
        for (field, value) in [
            ("event_id", serde_json::json!("")),
            ("event_id", serde_json::json!("x".repeat(65))),
            ("event_id", serde_json::json!("🌊".repeat(33))),
            ("animation_key", serde_json::json!("x".repeat(257))),
            ("started_at_unix_ms", serde_json::json!(-1)),
            ("started_at_unix_ms", serde_json::json!(1.5)),
            ("started_at_unix_ms", serde_json::json!(9_007_199_254_740_992u64)),
            ("started_at_unix_ms", serde_json::json!("123")),
            ("roaming", serde_json::json!({"x":null})),
            ("roaming", serde_json::json!({"x":"1"})),
            ("roaming", serde_json::json!({"":1})),
            ("roaming", serde_json::json!({"x".repeat(33):1})),
            ("roaming", serde_json::Value::Object((0..65).map(|i| (format!("r{i}"), serde_json::json!(0))).collect())),
            ("expressions", serde_json::json!(null)),
            ("expressions", serde_json::json!([{"event_id":"e","started_at_unix_ms":0,"expression":"x","values":[null]}])),
            ("expressions", serde_json::json!([{"event_id":"e","started_at_unix_ms":0,"expression":"x".repeat(2049),"values":[]}])),
            ("expressions", serde_json::json!([{"event_id":"e","started_at_unix_ms":0,"expression":"x","values":vec![0;17]}])),
            ("expressions", serde_json::json!(vec![serde_json::json!({"event_id":"e","started_at_unix_ms":0,"expression":"x","values":[]});17])),
            ("controllers", serde_json::json!({"c":{"state":"x","started_at_unix_ms":0,"variables":{"x":null}}})),
            ("controllers", serde_json::json!({"c":{"state":"x".repeat(129),"started_at_unix_ms":0,"variables":{}}})),
            ("controllers", serde_json::json!({"c".repeat(129):{"state":"x","started_at_unix_ms":0,"variables":{}}})),
            ("controllers", serde_json::json!({"c":{"state":"x","started_at_unix_ms":0,"variables":{"v".repeat(65):0}}})),
            ("controllers", serde_json::Value::Object((0..65).map(|i| (format!("c{i}"), serde_json::json!({"state":"x","started_at_unix_ms":0,"variables":{}}))).collect())),
            ("extra", serde_json::json!("中".repeat(22_000))),
        ] {
            let mut motion = valid.clone();
            motion[field] = value;
            invalid.push(motion);
        }
        for motion in invalid {
            body.motion = Some(motion.clone());
            assert!(
                matches!(
                    store.publish_player("account_local", &body),
                    Err(CloudError::InvalidMetadata(_))
                ),
                "invalid payload accepted: {motion}"
            );
            assert_eq!(store.player_revision("account_local", "motion").unwrap(), 0);
        }
        body.motion = Some(serde_json::json!({
            "event_id":"x".repeat(64),"animation_key":"x".repeat(256),"started_at_unix_ms":9_007_199_254_740_991u64,
            "roaming":{"x":-1.25},"expressions":[{"event_id":"e","started_at_unix_ms":0,"expression":"x".repeat(2048),"values":vec![0;16]}],
            "controllers":{"x":{"state":"","started_at_unix_ms":0,"variables":{"v":2.5}}}
        }));
        store.publish_player("account_local", &body).unwrap();
    }
    fn setup() -> (tempfile::TempDir, CloudStore) {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::CloudConfig {
            instance_id: "test".into(),
            origin: "https://cloud.test".into(),
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            database_path: dir.path().join("test.db"),
            object_dir: dir.path().join("objects"),
            access_token: Some("secret".into()),
            bootstrap_account_id: "account_local".into(),
            bootstrap_password_hash: None,
            allow_self_registration: false,
            max_asset_bytes: 128 * 1024 * 1024,
            max_message_bytes: 64 * 1024,
            max_entity_query_count: 64,
            visual: Default::default(),
            trusted_proxy_ips: Vec::new(),
        };
        let store = CloudStore::open(&config).unwrap();
        (dir, store)
    }
    #[test]
    fn cached_official_name_never_authorizes_an_offline_alias() {
        let (_dir, store) = setup();
        let profile = uuid::Uuid::new_v4();
        let conn = store.connection.lock().unwrap();
        conn.execute("INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('alias','account_local','official',?1,'Player',1)",[profile.to_string()]).unwrap();
        conn.execute(
            "INSERT INTO player_canonical_names(identity_id,name) VALUES ('alias','Player')",
            [],
        )
        .unwrap();
        drop(conn);
        let body:PlayerAppearanceUpdate=serde_json::from_value(serde_json::json!({"identity_id":"alias","entity_uuid":offline_uuid("Player"),"expected_revision":0})).unwrap();
        assert!(matches!(
            store.publish_player("account_local", &body),
            Err(CloudError::IdentityProfileMismatch)
        ));
    }

    #[test]
    fn offline_uuid_matches_java() {
        assert_eq!(
            offline_uuid("Micaftic1").to_string(),
            "0d6ec007-cb46-305b-90b4-8402c15bd87b".to_string()
        );
    }
    #[test]
    fn repeated_verified_identity_proof_reuses_binding() {
        let (_dir, store) = setup();
        let profile = uuid::Uuid::new_v4().to_string();
        let input = crate::models::CreateIdentityChallenge {
            provider_id: "official".into(),
            username: "Player".into(),
            profile_uuid: Some(profile.clone()),
        };
        let first = store
            .create_identity_challenge("account_local", &input)
            .unwrap();
        let original = store
            .complete_identity_challenge("account_local", &first.challenge_id, &profile, "Player")
            .unwrap();
        let second = store
            .create_identity_challenge("account_local", &input)
            .unwrap();
        let repeated = store
            .complete_identity_challenge("account_local", &second.challenge_id, &profile, "Renamed")
            .unwrap();
        assert_eq!(
            original.identity_id, repeated.identity_id,
            "reconnect must not create an ambiguous second binding"
        );
        let conn = store.connection.lock().unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM identities WHERE profile_uuid=?1",
                [&profile],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT name FROM player_canonical_names WHERE identity_id=?1",
                [&original.identity_id],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "Renamed"
        );
        drop(conn);
        store
            .create_account("other_account", "test-password")
            .unwrap();
        let other = store
            .create_identity_challenge("other_account", &input)
            .unwrap();
        assert!(matches!(
            store.complete_identity_challenge(
                "other_account",
                &other.challenge_id,
                &profile,
                "Player"
            ),
            Err(CloudError::AccessDenied)
        ));
    }
    #[test]
    fn player_cas_privacy_expiry_and_identity_are_independent() {
        let (_dir, store) = setup();
        let uuid = uuid::Uuid::new_v4();
        {
            let conn = store.connection.lock().unwrap();
            conn.execute("INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('id','account_local','official',?1,'Untrusted',1)",[uuid.to_string()]).unwrap();
            conn.execute("INSERT INTO assets(asset_id,owner_account_id,current_revision,visibility) VALUES ('model','account_local',1,'PUBLIC')",[]).unwrap();
            conn.execute("INSERT INTO asset_revisions(asset_id,revision,name,format,raw_sha256,byte_length,object_path,created_at) VALUES ('model',1,'model','ysm','sha',5,'object','now')",[]).unwrap();
        }
        let mut body = PlayerAppearanceUpdate {
            profile_name_proof: None,
            motion: None,
            display_state: None,
            identity_id: "id".into(),
            entity_uuid: uuid,
            expected_revision: 0,
            asset_id: Some("model".into()),
            asset_revision: Some(1),
            raw_sha256: Some("sha".into()),
            texture_id: Some("贴图".into()),
        };
        assert_eq!(store.publish_player("account_local", &body).unwrap(), 1);
        assert!(
            store.query_players("observer", &[uuid]).unwrap()[0]
                .selection
                .is_some()
        );
        assert!(matches!(
            store.publish_player("account_local", &body),
            Err(CloudError::RevisionConflict)
        ));
        body.expected_revision = 1;
        {
            let conn = store.connection.lock().unwrap();
            conn.execute("UPDATE asset_revisions SET format='txt'", [])
                .unwrap();
        }
        assert!(
            store.publish_player("account_local", &body).is_err(),
            "non-model assets cannot break the entire player query batch"
        );
        {
            let conn = store.connection.lock().unwrap();
            conn.execute("UPDATE asset_revisions SET format='ysm'", [])
                .unwrap();
        }
        body.entity_uuid = offline_uuid("Untrusted");
        assert!(matches!(
            store.publish_player("account_local", &body),
            Err(CloudError::IdentityProfileMismatch)
        ));
        body.entity_uuid = uuid;
        {
            let conn = store.connection.lock().unwrap();
            conn.execute("UPDATE assets SET visibility='PRIVATE'", [])
                .unwrap();
        }
        assert!(
            store.query_players("observer", &[uuid]).unwrap()[0]
                .selection
                .is_none()
        );
        assert!(
            store.query_players("account_local", &[uuid]).unwrap()[0]
                .selection
                .is_none(),
            "even two clients sharing an account cannot discover private player appearances"
        );
        body.asset_id = None;
        assert_eq!(store.publish_player("account_local", &body).unwrap(), 2);
        assert!(
            store.query_players("account_local", &[uuid]).unwrap()[0]
                .selection
                .is_none()
        );
        body.asset_id = Some("model".into());
        body.expected_revision = 2;
        store.publish_player("account_local", &body).unwrap();
        {
            let conn = store.connection.lock().unwrap();
            conn.execute("UPDATE player_appearances SET updated_at=0", [])
                .unwrap();
        }
        assert!(
            store.query_players("account_local", &[uuid]).unwrap()[0]
                .selection
                .is_none()
        );
    }
}
