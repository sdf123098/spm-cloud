//! Explicit-scope display metadata. This module never writes Minecraft game state.
use crate::{
    CloudStore,
    api::{AppState, authenticate},
    config::validate_slug,
    error::CloudError,
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct VisualSettings {
    pub features: crate::config_file::Features,
    pub limits: crate::config_file::Limits,
    pub retention: crate::config_file::Retention,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResourceRef {
    pub asset_id: String,
    pub asset_revision: u64,
    pub raw_sha256: String,
    pub texture_id: String,
    pub format: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectileSnapshot {
    pub world_epoch: String,
    pub dimension_id: String,
    pub entity_uuid: Uuid,
    pub entity_kind: String,
    pub source_identity_id: String,
    pub source_entity_uuid: Uuid,
    pub event_id: String,
    pub resource: ResourceRef,
    pub projectile_bundle_key: String,
    #[serde(default)]
    pub variables: BTreeMap<String, f64>,
    /// None means the client cannot prove the historical firing item.
    pub firing_item_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotLease {
    pub snapshot: ProjectileSnapshot,
    pub lease_revision: u64,
    pub received_at_unix_ms: i64,
    pub expires_at_unix_ms: i64,
    pub absolute_expires_at_unix_ms: i64,
    pub server_time_unix_ms: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotQuery {
    pub world_epoch: String,
    pub dimension_id: String,
    pub entity_uuids: Vec<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseUpdate {
    pub world_epoch: String,
    pub dimension_id: String,
    pub event_id: String,
    pub expected_revision: u64,
}

pub enum LeaseAction {
    Renew(Uuid),
    Withdraw(Uuid),
}

pub(crate) fn migrate(conn: &Connection) -> Result<(), CloudError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS projectile_snapshots (
        scope_id TEXT NOT NULL REFERENCES scopes(scope_id), world_epoch TEXT NOT NULL,
        dimension_id TEXT NOT NULL, entity_uuid TEXT NOT NULL,
        publisher_account_id TEXT NOT NULL REFERENCES accounts(account_id),
        source_identity_id TEXT NOT NULL REFERENCES identities(identity_id),
        event_id TEXT NOT NULL, snapshot_json TEXT NOT NULL,
        lease_revision INTEGER NOT NULL DEFAULT 1, received_at_ms INTEGER NOT NULL,
        expires_at_ms INTEGER NOT NULL, absolute_expires_at_ms INTEGER NOT NULL,
        withdrawn INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY(scope_id,world_epoch,dimension_id,entity_uuid),
        UNIQUE(scope_id,world_epoch,publisher_account_id,event_id)
    ); CREATE INDEX IF NOT EXISTS projectile_snapshots_publisher ON projectile_snapshots(publisher_account_id,expires_at_ms);
    CREATE INDEX IF NOT EXISTS projectile_snapshots_expiry ON projectile_snapshots(absolute_expires_at_ms);
    CREATE TABLE IF NOT EXISTS visual_publish_buckets(account_id TEXT PRIMARY KEY REFERENCES accounts(account_id), tokens REAL NOT NULL, updated_at_ms INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS projectile_tombstones (
        scope_id TEXT NOT NULL, world_epoch TEXT NOT NULL, dimension_id TEXT NOT NULL,
        entity_uuid TEXT NOT NULL, publisher_account_id TEXT NOT NULL, event_id TEXT NOT NULL,
        PRIMARY KEY(scope_id,world_epoch,dimension_id,entity_uuid),
        UNIQUE(scope_id,world_epoch,publisher_account_id,event_id)
    );")?;
    Ok(())
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(crate) fn identifier(value: &str) -> bool {
    value.len() <= 256
        && value.split_once(':').is_some_and(|(namespace, path)| {
            !namespace.is_empty()
                && !path.is_empty()
                && namespace
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
                && path
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_./-".contains(&b))
        })
}

pub(crate) fn context(
    conn: &Connection,
    account: &str,
    scope: &str,
    epoch: &str,
    dimension: &str,
) -> Result<(), CloudError> {
    validate_slug(scope, "scope_id")?;
    validate_slug(epoch, "world_epoch")?;
    if !identifier(dimension) {
        return Err(CloudError::invalid_metadata("invalid dimension_id"));
    }
    let allowed: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM scopes s JOIN scope_acl acl ON acl.scope_id=s.scope_id WHERE s.scope_id=?1 AND s.world_epoch=?2 AND acl.account_id=?3)", params![scope, epoch, account], |row| row.get(0))?;
    if !allowed {
        return Err(CloudError::ScopeAccessDenied);
    }
    Ok(())
}

fn identity(conn: &Connection, account: &str, id: &str, source: Uuid) -> Result<(), CloudError> {
    // An observer must additionally compare this identity with native owner on its loaded entity.
    // Knowing a projectile UUID is deliberately not treated as server proof of firing it.
    let allowed: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM identities i JOIN identity_providers p ON p.provider_id=COALESCE(i.provider_id,'official') AND p.enabled=1 WHERE i.identity_id=?1 AND i.account_id=?2 AND i.verified=1 AND i.profile_uuid=?3 AND i.identity_kind IN ('official','yggdrasil'))", params![id, account, source.to_string()], |row| row.get(0))?;
    if !allowed {
        return Err(CloudError::IdentityNotVerified);
    }
    let withdrawn: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM player_appearances WHERE identity_id=?1 AND asset_id IS NULL)",
        [id],
        |row| row.get(0),
    )?;
    if withdrawn {
        return Err(CloudError::AccessDenied);
    }
    Ok(())
}

pub(crate) fn resource(
    conn: &Connection,
    account: &str,
    value: &ResourceRef,
) -> Result<(), CloudError> {
    validate_slug(&value.asset_id, "asset_id")?;
    if value.asset_revision == 0
        || value.asset_revision >= 9_007_199_254_740_991
        || value.raw_sha256.len() != 64
        || !value
            .raw_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || value.texture_id.is_empty()
        || value.texture_id.encode_utf16().count() > 256
        || value.texture_id.chars().any(char::is_control)
        || !matches!(
            value.format.as_str(),
            "ysm" | "zip" | "bbmodel" | "gltf" | "glb"
        )
    {
        return Err(CloudError::invalid_metadata(
            "invalid exact resource reference",
        ));
    }
    let allowed: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM asset_revisions r JOIN assets a ON a.asset_id=r.asset_id LEFT JOIN asset_acl acl ON acl.asset_id=a.asset_id AND acl.account_id=?4 WHERE r.asset_id=?1 AND r.revision=?2 AND r.raw_sha256=?3 AND r.format=?5 AND (a.visibility='PUBLIC' OR acl.permission IN ('manage','render_read')))", params![value.asset_id, value.asset_revision as i64, value.raw_sha256, account, value.format], |row| row.get(0))?;
    if !allowed {
        return Err(CloudError::AccessDenied);
    }
    Ok(())
}

pub(crate) fn rate(
    conn: &Connection,
    account: &str,
    settings: &VisualSettings,
    time: i64,
) -> Result<(), CloudError> {
    let previous: Option<(f64, i64)> = conn
        .query_row(
            "SELECT tokens,updated_at_ms FROM visual_publish_buckets WHERE account_id=?1",
            [account],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let burst = settings.limits.visual_publish_burst as f64;
    let available = previous.map_or(burst, |(tokens, last)| {
        (tokens
            + (time - last).max(0) as f64 / 1000.0
                * settings.limits.visual_publish_requests_per_second as f64)
            .min(burst)
    });
    if available < 1.0 {
        return Err(CloudError::RateLimited);
    }
    conn.execute("INSERT INTO visual_publish_buckets(account_id,tokens,updated_at_ms) VALUES (?1,?2,?3) ON CONFLICT(account_id) DO UPDATE SET tokens=excluded.tokens,updated_at_ms=excluded.updated_at_ms", params![account, available-1.0, time])?;
    Ok(())
}

fn validate(value: &ProjectileSnapshot, settings: &VisualSettings) -> Result<String, CloudError> {
    validate_slug(&value.source_identity_id, "source_identity_id")?;
    validate_slug(&value.event_id, "event_id")?;
    if !identifier(&value.entity_kind)
        || !identifier(&value.projectile_bundle_key)
        || value
            .firing_item_id
            .as_deref()
            .is_some_and(|v| !identifier(v))
        || value.variables.len() > settings.limits.max_visual_variables
        || value.variables.iter().any(|(k, v)| {
            k.is_empty()
                || k.encode_utf16().count() > 32
                || k.chars().any(char::is_control)
                || !v.is_finite()
                || (*v as f32).is_infinite()
        })
    {
        return Err(CloudError::invalid_metadata(
            "invalid projectile display state",
        ));
    }
    let text = serde_json::to_string(value)
        .map_err(|_| CloudError::invalid_metadata("invalid projectile JSON"))?;
    if text.len() > settings.limits.max_visual_state_bytes {
        return Err(CloudError::MessageTooLarge);
    }
    Ok(text)
}

fn lease(
    conn: &Connection,
    scope: &str,
    epoch: &str,
    dimension: &str,
    entity: Uuid,
    time: i64,
) -> Result<Option<(String, SnapshotLease, bool)>, CloudError> {
    let row: Option<(String,String,i64,i64,i64,i64,bool)> = conn.query_row("SELECT publisher_account_id,snapshot_json,lease_revision,received_at_ms,expires_at_ms,absolute_expires_at_ms,withdrawn FROM projectile_snapshots WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4", params![scope,epoch,dimension,entity.to_string()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional()?;
    row.map(
        |(publisher, text, revision, received, expires, absolute, withdrawn)| {
            let snapshot = serde_json::from_str(&text)
                .map_err(|_| CloudError::configuration("invalid stored snapshot"))?;
            Ok((
                publisher,
                SnapshotLease {
                    snapshot,
                    lease_revision: revision as u64,
                    received_at_unix_ms: received,
                    expires_at_unix_ms: expires,
                    absolute_expires_at_unix_ms: absolute,
                    server_time_unix_ms: time,
                },
                withdrawn,
            ))
        },
    )
    .transpose()
}

impl CloudStore {
    /// Remove display payloads while retaining compact replay barriers until an explicit epoch rotation.
    /// These identifiers contain no model references, variables or player display state.
    pub fn cleanup_visual_states(&self, time: i64) -> Result<usize, CloudError> {
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("INSERT OR IGNORE INTO projectile_tombstones(scope_id,world_epoch,dimension_id,entity_uuid,publisher_account_id,event_id)
            SELECT scope_id,world_epoch,dimension_id,entity_uuid,publisher_account_id,event_id FROM projectile_snapshots
            WHERE withdrawn=1 OR expires_at_ms<=?1 OR absolute_expires_at_ms<=?1", [time])?;
        let removed = tx.execute("DELETE FROM projectile_snapshots WHERE withdrawn=1 OR expires_at_ms<=?1 OR absolute_expires_at_ms<=?1
            OR NOT EXISTS(SELECT 1 FROM scopes s WHERE s.scope_id=projectile_snapshots.scope_id AND s.world_epoch=projectile_snapshots.world_epoch)", [time])?;
        tx.execute("DELETE FROM projectile_tombstones WHERE NOT EXISTS(SELECT 1 FROM scopes s WHERE s.scope_id=projectile_tombstones.scope_id AND s.world_epoch=projectile_tombstones.world_epoch)", [])?;
        tx.execute("DELETE FROM vehicle_bindings WHERE NOT EXISTS(SELECT 1 FROM scopes s WHERE s.scope_id=vehicle_bindings.scope_id AND s.world_epoch=vehicle_bindings.world_epoch)", [])?;
        tx.execute("UPDATE entity_motion_states SET update_json=NULL,resource_json=NULL WHERE expires_at_ms<=?1", [time])?;
        tx.execute("DELETE FROM entity_motion_states WHERE NOT EXISTS(SELECT 1 FROM scopes s WHERE s.scope_id=entity_motion_states.scope_id AND s.world_epoch=entity_motion_states.world_epoch)", [])?;
        tx.execute("DELETE FROM player_display_states WHERE identity_id IN (SELECT identity_id FROM player_appearances WHERE asset_id IS NULL OR updated_at<=?1)", [time / 1000 - 60])?;
        tx.commit()?;
        Ok(removed)
    }

    pub fn publish_projectile(
        &self,
        account: &str,
        scope: &str,
        value: &ProjectileSnapshot,
        settings: &VisualSettings,
        time: i64,
    ) -> Result<SnapshotLease, CloudError> {
        let text = validate(value, settings)?;
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        context(&tx, account, scope, &value.world_epoch, &value.dimension_id)?;
        identity(
            &tx,
            account,
            &value.source_identity_id,
            value.source_entity_uuid,
        )?;
        resource(&tx, account, &value.resource)?;
        let retired: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM projectile_tombstones WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4)", params![scope,value.world_epoch,value.dimension_id,value.entity_uuid.to_string()], |r|r.get(0))?;
        if retired {
            return Err(CloudError::NotFound);
        }
        let reused_event: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM projectile_tombstones WHERE scope_id=?1 AND world_epoch=?2 AND publisher_account_id=?3 AND event_id=?4)", params![scope,value.world_epoch,account,value.event_id], |r|r.get(0))?;
        if reused_event {
            return Err(CloudError::IdempotencyConflict);
        }
        rate(&tx, account, settings, time)?;
        if let Some((publisher, existing, _)) = lease(
            &tx,
            scope,
            &value.world_epoch,
            &value.dimension_id,
            value.entity_uuid,
            time,
        )? {
            if publisher != account || existing.snapshot != *value {
                return Err(CloudError::IdempotencyConflict);
            }
            // Idempotent replay does not renew or revive expired/withdrawn snapshots.
            tx.commit()?;
            return Ok(existing);
        }
        let duplicate: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM projectile_snapshots WHERE scope_id=?1 AND world_epoch=?2 AND publisher_account_id=?3 AND event_id=?4)", params![scope,value.world_epoch,account,value.event_id], |r|r.get(0))?;
        if duplicate {
            return Err(CloudError::IdempotencyConflict);
        }
        let publisher_count: i64 = tx.query_row("SELECT COUNT(*) FROM projectile_snapshots WHERE publisher_account_id=?1 AND withdrawn=0 AND expires_at_ms>?2", params![account,time],|r|r.get(0))?;
        let world_count: i64 = tx.query_row("SELECT COUNT(*) FROM projectile_snapshots WHERE scope_id=?1 AND world_epoch=?2 AND withdrawn=0 AND expires_at_ms>?3", params![scope,value.world_epoch,time],|r|r.get(0))?;
        if publisher_count >= settings.limits.max_projectile_snapshots_per_publisher as i64
            || world_count >= settings.limits.max_projectile_snapshots_per_world as i64
        {
            return Err(CloudError::RateLimited);
        }
        let absolute = time + settings.retention.projectile_max_lifetime_seconds as i64 * 1000;
        let expiry =
            (time + settings.retention.projectile_idle_ttl_seconds as i64 * 1000).min(absolute);
        tx.execute("INSERT INTO projectile_snapshots(scope_id,world_epoch,dimension_id,entity_uuid,publisher_account_id,source_identity_id,event_id,snapshot_json,received_at_ms,expires_at_ms,absolute_expires_at_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![scope,value.world_epoch,value.dimension_id,value.entity_uuid.to_string(),account,value.source_identity_id,value.event_id,text,time,expiry,absolute])?;
        tx.commit()?;
        Ok(SnapshotLease {
            snapshot: value.clone(),
            lease_revision: 1,
            received_at_unix_ms: time,
            expires_at_unix_ms: expiry,
            absolute_expires_at_unix_ms: absolute,
            server_time_unix_ms: time,
        })
    }

    pub fn query_projectiles(
        &self,
        account: &str,
        scope: &str,
        query: &SnapshotQuery,
        settings: &VisualSettings,
        time: i64,
    ) -> Result<Vec<SnapshotLease>, CloudError> {
        if query.entity_uuids.len() > settings.limits.max_entity_query_count {
            return Err(CloudError::invalid_metadata("entity query limit exceeded"));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        context(
            &conn,
            account,
            scope,
            &query.world_epoch,
            &query.dimension_id,
        )?;
        let mut result = Vec::new();
        for entity in &query.entity_uuids {
            if let Some((publisher, entry, withdrawn)) = lease(
                &conn,
                scope,
                &query.world_epoch,
                &query.dimension_id,
                *entity,
                time,
            )? {
                if !withdrawn
                    && entry.expires_at_unix_ms > time
                    && entry.absolute_expires_at_unix_ms > time
                    && context(
                        &conn,
                        &publisher,
                        scope,
                        &query.world_epoch,
                        &query.dimension_id,
                    )
                    .is_ok()
                    && identity(
                        &conn,
                        &publisher,
                        &entry.snapshot.source_identity_id,
                        entry.snapshot.source_entity_uuid,
                    )
                    .is_ok()
                    && resource(&conn, &publisher, &entry.snapshot.resource).is_ok()
                    && resource(&conn, account, &entry.snapshot.resource).is_ok()
                {
                    result.push(entry);
                }
            }
        }
        Ok(result)
    }

    pub fn update_projectile_lease(
        &self,
        account: &str,
        scope: &str,
        input: &LeaseUpdate,
        settings: &VisualSettings,
        time: i64,
        action: LeaseAction,
    ) -> Result<SnapshotLease, CloudError> {
        let (entity, withdraw) = match action {
            LeaseAction::Renew(entity) => (entity, false),
            LeaseAction::Withdraw(entity) => (entity, true),
        };
        validate_slug(&input.event_id, "event_id")?;
        if input.expected_revision >= 9_007_199_254_740_991 {
            return Err(CloudError::invalid_metadata("invalid lease revision"));
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        context(&tx, account, scope, &input.world_epoch, &input.dimension_id)?;
        let (publisher, mut value, withdrawn) = lease(
            &tx,
            scope,
            &input.world_epoch,
            &input.dimension_id,
            entity,
            time,
        )?
        .ok_or(CloudError::NotFound)?;
        if publisher != account {
            return Err(CloudError::AccessDenied);
        }
        if value.snapshot.event_id != input.event_id
            || value.lease_revision != input.expected_revision
        {
            return Err(CloudError::RevisionConflict);
        }
        if !withdraw {
            if withdrawn
                || value.expires_at_unix_ms <= time
                || value.absolute_expires_at_unix_ms <= time
            {
                return Err(CloudError::NotFound);
            }
            identity(
                &tx,
                account,
                &value.snapshot.source_identity_id,
                value.snapshot.source_entity_uuid,
            )?;
            resource(&tx, account, &value.snapshot.resource)?;
        }
        rate(&tx, account, settings, time)?;
        value.lease_revision += 1;
        value.expires_at_unix_ms = if withdraw {
            time
        } else {
            (time + settings.retention.projectile_idle_ttl_seconds as i64 * 1000)
                .min(value.absolute_expires_at_unix_ms)
        };
        tx.execute("UPDATE projectile_snapshots SET lease_revision=?5,expires_at_ms=?6,withdrawn=?7 WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4",params![scope,input.world_epoch,input.dimension_id,entity.to_string(),value.lease_revision as i64,value.expires_at_unix_ms,withdraw])?;
        tx.commit()?;
        Ok(value)
    }
}

fn enabled(state: &AppState) -> Result<(), CloudError> {
    if !state.runtime_config().visual.features.projectile_snapshots {
        return Err(CloudError::NotFound);
    }
    Ok(())
}

pub async fn put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
    payload: Result<crate::visual_json::VisualJson<ProjectileSnapshot>, CloudError>,
) -> Result<Json<SnapshotLease>, CloudError> {
    enabled(&state)?;
    let account = authenticate(&state, &headers)?;
    let crate::visual_json::VisualJson(body) = payload?;
    Ok(Json(state.store.publish_projectile(
        &account,
        &scope,
        &body,
        &state.runtime_config().visual,
        now_ms(),
    )?))
}
pub async fn query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
    payload: Result<crate::visual_json::VisualJson<SnapshotQuery>, CloudError>,
) -> Result<Json<serde_json::Value>, CloudError> {
    enabled(&state)?;
    let account = authenticate(&state, &headers)?;
    let crate::visual_json::VisualJson(body) = payload?;
    Ok(Json(
        serde_json::json!({"entries":state.store.query_projectiles(&account,&scope,&body,&state.runtime_config().visual,now_ms())?}),
    ))
}
pub async fn renew(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope, entity)): Path<(String, Uuid)>,
    payload: Result<crate::visual_json::VisualJson<LeaseUpdate>, CloudError>,
) -> Result<Json<SnapshotLease>, CloudError> {
    enabled(&state)?;
    let account = authenticate(&state, &headers)?;
    let crate::visual_json::VisualJson(body) = payload?;
    Ok(Json(state.store.update_projectile_lease(
        &account,
        &scope,
        &body,
        &state.runtime_config().visual,
        now_ms(),
        LeaseAction::Renew(entity),
    )?))
}
pub async fn withdraw(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope, entity)): Path<(String, Uuid)>,
    payload: Result<crate::visual_json::VisualJson<LeaseUpdate>, CloudError>,
) -> Result<Json<SnapshotLease>, CloudError> {
    enabled(&state)?;
    let account = authenticate(&state, &headers)?;
    let crate::visual_json::VisualJson(body) = payload?;
    Ok(Json(state.store.update_projectile_lease(
        &account,
        &scope,
        &body,
        &state.runtime_config().visual,
        now_ms(),
        LeaseAction::Withdraw(entity),
    )?))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    #[test]
    fn cleanup_erases_payloads_without_restarting_replayed_leases() {
        let (_dir, store, settings, body) = fixture();
        store
            .publish_projectile("shooter", "test-scope", &body, &settings, 1_000)
            .unwrap();
        let time = 1_000 + settings.retention.projectile_idle_ttl_seconds as i64 * 1000;
        assert_eq!(store.cleanup_visual_states(time).unwrap(), 1);
        let conn = store.connection.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM projectile_snapshots", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM projectile_tombstones", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        drop(conn);
        assert!(matches!(
            store.publish_projectile("shooter", "test-scope", &body, &settings, time + 1),
            Err(CloudError::NotFound)
        ));
        let mut duplicate_event = body.clone();
        duplicate_event.entity_uuid = Uuid::new_v4();
        assert!(matches!(
            store.publish_projectile(
                "shooter",
                "test-scope",
                &duplicate_event,
                &settings,
                time + 1
            ),
            Err(CloudError::IdempotencyConflict)
        ));
        store
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE scopes SET world_epoch='reset-4'", [])
            .unwrap();
        store.cleanup_visual_states(time + 2).unwrap();
        assert_eq!(
            store
                .connection
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM projectile_tombstones", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(matches!(
            store.publish_projectile("shooter", "test-scope", &body, &settings, time + 3),
            Err(CloudError::ScopeAccessDenied)
        ));
    }
    pub(crate) fn fixture() -> (
        tempfile::TempDir,
        CloudStore,
        VisualSettings,
        ProjectileSnapshot,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let config =
            crate::config_file::LoadedConfig::load_with(None, dir.path(), &Default::default())
                .unwrap()
                .cloud;
        let store = CloudStore::open(&config).unwrap();
        let body: ProjectileSnapshot =
            serde_json::from_str(include_str!("../fixtures/projectile-snapshot-v1.json")).unwrap();
        store.connection.lock().unwrap().execute_batch("INSERT INTO accounts(account_id,created_at) VALUES ('shooter','now'),('observer','now'),('outsider','now');
            INSERT INTO scopes(scope_id,tenant_id,name,world_epoch,created_at) VALUES ('test-scope','shooter','Test','world-reset-3','now');
            INSERT INTO scope_acl(scope_id,account_id,role) VALUES ('test-scope','shooter','viewer'),('test-scope','observer','viewer');
            INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('identity-shooter','shooter','official','22222222-2222-4222-8222-222222222222','Shooter',1);
            INSERT INTO assets(asset_id,owner_account_id,current_revision,visibility) VALUES ('model-arrow','shooter',1,'PUBLIC');
            INSERT INTO asset_revisions(asset_id,revision,name,format,raw_sha256,byte_length,object_path,created_at) VALUES ('model-arrow',1,'Arrow','ysm','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',1,'test','now');").unwrap();
        (dir, store, VisualSettings::default(), body)
    }
    fn query_for(body: &ProjectileSnapshot) -> SnapshotQuery {
        SnapshotQuery {
            world_epoch: body.world_epoch.clone(),
            dimension_id: body.dimension_id.clone(),
            entity_uuids: vec![body.entity_uuid],
        }
    }
    fn renewal(body: &ProjectileSnapshot, revision: u64) -> LeaseUpdate {
        LeaseUpdate {
            world_epoch: body.world_epoch.clone(),
            dimension_id: body.dimension_id.clone(),
            event_id: body.event_id.clone(),
            expected_revision: revision,
        }
    }

    #[tokio::test]
    async fn authenticated_http_normalizes_invalid_json_and_enforces_raw_body_limit() {
        let (_dir, store, settings, body) = fixture();
        let mut config =
            crate::config_file::LoadedConfig::load_with(None, _dir.path(), &Default::default())
                .unwrap()
                .cloud;
        config.bootstrap_account_id = "shooter".into();
        config.access_token = Some("projectile-test-secret".into());
        config.visual = settings;
        config.visual.features.projectile_snapshots = true;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = crate::api::router(AppState::new(config, store));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let url = format!("{origin}/v1/scopes/test-scope/projectiles");
        assert_eq!(
            client.put(&url).json(&body).send().await.unwrap().status(),
            401
        );
        let valid = client
            .put(&url)
            .bearer_auth("projectile-test-secret")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(valid.status(), 200);
        let value: serde_json::Value = valid.json().await.unwrap();
        assert_eq!(value["snapshot"], serde_json::to_value(&body).unwrap());
        assert!(
            value["server_time_unix_ms"].as_i64().unwrap()
                >= value["received_at_unix_ms"].as_i64().unwrap()
        );
        let raw = serde_json::to_string(&body).unwrap();
        for invalid in [
            raw.replace(
                "\"variables\":{\"arrow_variant\":2.0}",
                "\"variables\":{\"arrow_variant\":1.0,\"arrow_variant\":2.0}",
            ),
            raw.replace(
                "\"variables\":{\"arrow_variant\":2.0}",
                "\"variables\":{\"arrow_variant\":1.0,\"\\u0061rrow_variant\":2.0}",
            ),
            raw.replace(
                "\"event_id\":\"shot-1\"",
                "\"event_id\":\"shot-1\",\"event_id\":\"shot-1\"",
            ),
            raw.replace(
                "\"variables\":{\"arrow_variant\":2.0}",
                "\"variables\":null",
            ),
            "{}".into(),
        ] {
            assert_ne!(invalid, raw, "negative fixture replacement did not match");
            let response = client
                .put(&url)
                .bearer_auth("projectile-test-secret")
                .header("content-type", "application/json")
                .body(invalid)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 400);
            assert_eq!(
                response.json::<serde_json::Value>().await.unwrap()["code"],
                "INVALID_METADATA"
            );
        }
        let response = client
            .put(&url)
            .bearer_auth("projectile-test-secret")
            .header("content-type", "application/json")
            .body(" ".repeat(8193) + &raw)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 413);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["code"],
            "MESSAGE_TOO_LARGE"
        );
        server.abort();
    }

    #[test]
    fn stopping_player_sharing_permanently_withdraws_existing_shots() {
        let (_dir, store, settings, body) = fixture();
        store
            .publish_projectile("shooter", "test-scope", &body, &settings, 1000)
            .unwrap();
        let publish = |revision, asset: Option<&str>| crate::player::PlayerAppearanceUpdate {
            identity_id: body.source_identity_id.clone(),
            entity_uuid: body.source_entity_uuid,
            expected_revision: revision,
            asset_id: asset.map(str::to_owned),
            asset_revision: Some(1),
            raw_sha256: Some(body.resource.raw_sha256.clone()),
            texture_id: Some("default".into()),
            profile_name_proof: None,
            motion: None,
            display_state: None,
        };
        store.publish_player("shooter", &publish(0, None)).unwrap();
        store
            .publish_player("shooter", &publish(1, Some("model-arrow")))
            .unwrap();
        assert!(
            store
                .query_projectiles("observer", "test-scope", &query_for(&body), &settings, 2000)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn immutable_replay_cas_and_backend_lifetime() {
        let (_dir, store, mut settings, body) = fixture();
        settings.retention.projectile_idle_ttl_seconds = 10;
        settings.retention.projectile_max_lifetime_seconds = 15;
        let first = store
            .publish_projectile("shooter", "test-scope", &body, &settings, 1_000)
            .unwrap();
        assert_eq!(first.expires_at_unix_ms, 11_000);
        assert_eq!(
            store
                .publish_projectile("shooter", "test-scope", &body, &settings, 2_000)
                .unwrap()
                .expires_at_unix_ms,
            11_000
        );
        let mut changed = body.clone();
        changed.resource.texture_id = "changed".into();
        assert!(matches!(
            store.publish_projectile("shooter", "test-scope", &changed, &settings, 3_000),
            Err(CloudError::IdempotencyConflict)
        ));
        assert!(matches!(
            store.update_projectile_lease(
                "observer",
                "test-scope",
                &renewal(&body, 1),
                &settings,
                3_000,
                LeaseAction::Withdraw(body.entity_uuid)
            ),
            Err(CloudError::AccessDenied)
        ));
        let renewed = store
            .update_projectile_lease(
                "shooter",
                "test-scope",
                &renewal(&body, 1),
                &settings,
                8_000,
                LeaseAction::Renew(body.entity_uuid),
            )
            .unwrap();
        assert_eq!(renewed.expires_at_unix_ms, 16_000);
        assert_eq!(renewed.received_at_unix_ms, 1_000);
        assert!(matches!(
            store.update_projectile_lease(
                "shooter",
                "test-scope",
                &renewal(&body, 1),
                &settings,
                9_000,
                LeaseAction::Renew(body.entity_uuid)
            ),
            Err(CloudError::RevisionConflict)
        ));
        assert!(
            store
                .query_projectiles(
                    "observer",
                    "test-scope",
                    &query_for(&body),
                    &settings,
                    16_000
                )
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            store.update_projectile_lease(
                "shooter",
                "test-scope",
                &renewal(&body, 2),
                &settings,
                16_000,
                LeaseAction::Renew(body.entity_uuid)
            ),
            Err(CloudError::NotFound)
        ));
        // Replaying the create after expiry cannot restart its absolute lifetime.
        assert_eq!(
            store
                .publish_projectile("shooter", "test-scope", &body, &settings, 20_000)
                .unwrap()
                .absolute_expires_at_unix_ms,
            16_000
        );
    }
    #[test]
    fn permissions_are_checked_even_after_source_changes_and_on_cache_hit() {
        let (_dir, store, settings, body) = fixture();
        store
            .publish_projectile("shooter", "test-scope", &body, &settings, 1_000)
            .unwrap();
        assert_eq!(
            store
                .query_projectiles(
                    "observer",
                    "test-scope",
                    &query_for(&body),
                    &settings,
                    2_000
                )
                .unwrap()
                .len(),
            1
        );
        assert!(matches!(
            store.query_projectiles(
                "outsider",
                "test-scope",
                &query_for(&body),
                &settings,
                2_000
            ),
            Err(CloudError::ScopeAccessDenied)
        ));
        let mut wrong = query_for(&body);
        wrong.world_epoch = "another-world".into();
        assert!(matches!(
            store.query_projectiles("observer", "test-scope", &wrong, &settings, 2_000),
            Err(CloudError::ScopeAccessDenied)
        ));
        wrong = query_for(&body);
        wrong.dimension_id = "minecraft:the_nether".into();
        assert!(
            store
                .query_projectiles("observer", "test-scope", &wrong, &settings, 2_000)
                .unwrap()
                .is_empty()
        );
        // Updating current revision does not revoke the exact old revision lease.
        store
            .connection
            .lock()
            .unwrap()
            .execute_batch("UPDATE assets SET current_revision=2;")
            .unwrap();
        assert_eq!(
            store
                .query_projectiles(
                    "observer",
                    "test-scope",
                    &query_for(&body),
                    &settings,
                    2_000
                )
                .unwrap()
                .len(),
            1
        );
        store.connection.lock().unwrap().execute_batch("UPDATE assets SET visibility='PRIVATE'; INSERT INTO asset_acl(asset_id,account_id,permission) VALUES ('model-arrow','shooter','manage');").unwrap();
        assert!(
            store
                .query_projectiles(
                    "observer",
                    "test-scope",
                    &query_for(&body),
                    &settings,
                    2_000
                )
                .unwrap()
                .is_empty()
        );
        store.connection.lock().unwrap().execute_batch("INSERT INTO asset_acl(asset_id,account_id,permission) VALUES ('model-arrow','observer','render_read');").unwrap();
        assert_eq!(
            store
                .query_projectiles(
                    "observer",
                    "test-scope",
                    &query_for(&body),
                    &settings,
                    2_000
                )
                .unwrap()
                .len(),
            1
        );
        store.connection.lock().unwrap().execute_batch("INSERT INTO player_appearances(identity_id,entity_uuid,revision) VALUES ('identity-shooter','22222222-2222-4222-8222-222222222222',3);").unwrap();
        assert!(
            store
                .query_projectiles(
                    "observer",
                    "test-scope",
                    &query_for(&body),
                    &settings,
                    2_000
                )
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn limits_identity_and_unknown_equipment() {
        let (_dir, store, mut settings, body) = fixture();
        assert_eq!(body.firing_item_id, None);
        let mut forged = body.clone();
        forged.source_entity_uuid = Uuid::new_v4();
        assert!(matches!(
            store.publish_projectile("shooter", "test-scope", &forged, &settings, 0),
            Err(CloudError::IdentityNotVerified)
        ));
        settings.limits.max_projectile_snapshots_per_publisher = 1;
        store
            .publish_projectile("shooter", "test-scope", &body, &settings, 1_000)
            .unwrap();
        let mut second = body.clone();
        second.entity_uuid = Uuid::new_v4();
        second.event_id = "shot-2".into();
        assert!(matches!(
            store.publish_projectile("shooter", "test-scope", &second, &settings, 1_001),
            Err(CloudError::RateLimited)
        ));
        settings.limits.max_entity_query_count = 1;
        let mut query = query_for(&body);
        query.entity_uuids.push(Uuid::new_v4());
        assert!(matches!(
            store.query_projectiles("observer", "test-scope", &query, &settings, 2_000),
            Err(CloudError::InvalidMetadata(_))
        ));
        settings.limits.max_visual_variables = 0;
        assert!(validate(&body, &settings).is_err());
        let mut empty = body.clone();
        empty.variables.clear();
        assert!(validate(&empty, &settings).is_ok());
        empty.variables.insert("bad".into(), f64::INFINITY);
        assert!(validate(&empty, &VisualSettings::default()).is_err());
    }
    #[test]
    fn publish_rate_limit_is_persisted_and_withdraw_does_not_delete_shared_rows() {
        let (_dir, store, mut settings, body) = fixture();
        settings.limits.visual_publish_burst = 1;
        settings.limits.visual_publish_requests_per_second = 1;
        store
            .publish_projectile("shooter", "test-scope", &body, &settings, 1_000)
            .unwrap();
        assert!(matches!(
            store.publish_projectile("shooter", "test-scope", &body, &settings, 1_001),
            Err(CloudError::RateLimited)
        ));
        store
            .update_projectile_lease(
                "shooter",
                "test-scope",
                &renewal(&body, 1),
                &settings,
                2_001,
                LeaseAction::Withdraw(body.entity_uuid),
            )
            .unwrap();
        assert!(
            store
                .query_projectiles(
                    "observer",
                    "test-scope",
                    &query_for(&body),
                    &settings,
                    2_002
                )
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store
                .publish_projectile("shooter", "test-scope", &body, &settings, 3_001)
                .unwrap()
                .lease_revision,
            2
        );
        assert!(
            store
                .query_projectiles(
                    "observer",
                    "test-scope",
                    &query_for(&body),
                    &settings,
                    3_002
                )
                .unwrap()
                .is_empty()
        );
    }
}
