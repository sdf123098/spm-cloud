//! Short-lived, explicitly edited MAID/FAKE_PLAYER display timelines.
use crate::{
    CloudStore,
    api::{AppState, authenticate},
    error::CloudError,
    visual::{self, ResourceRef, SnapshotQuery, VisualSettings},
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

type AuthorizedAppearanceRow = (
    String,
    u64,
    String,
    u64,
    Option<String>,
    Option<u64>,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
    String,
    String,
);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    pub event_id: String,
    pub animation_key: String,
    pub started_at_unix_ms: u64,
    #[serde(default)]
    pub roaming: BTreeMap<String, f64>,
    #[serde(default)]
    pub expressions: Vec<Expression>,
    #[serde(default)]
    pub controllers: BTreeMap<String, Controller>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Expression {
    pub event_id: String,
    pub started_at_unix_ms: u64,
    pub expression: String,
    pub values: Vec<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Controller {
    pub state: String,
    pub started_at_unix_ms: u64,
    pub variables: BTreeMap<String, f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityMotionUpdate {
    pub world_epoch: String,
    pub dimension_id: String,
    pub entity_kind: String,
    pub target_id: String,
    pub binding_revision: u64,
    pub appearance_revision: u64,
    pub expected_revision: u64,
    pub motion: Motion,
}
#[derive(Serialize, Deserialize)]
pub struct EntityMotionEntry {
    pub entity_uuid: Uuid,
    pub update: EntityMotionUpdate,
    pub resource: ResourceRef,
    pub revision: u64,
    pub server_time_unix_ms: i64,
    pub expires_at_unix_ms: i64,
}
#[derive(Serialize)]
pub struct MotionRevision {
    pub entity_uuid: Uuid,
    pub revision: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionWithdrawal {
    pub world_epoch: String,
    pub dimension_id: String,
    pub expected_revision: u64,
}
pub(crate) fn migrate(conn: &Connection) -> Result<(), CloudError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS entity_motion_states (
        scope_id TEXT NOT NULL,world_epoch TEXT NOT NULL,dimension_id TEXT NOT NULL,entity_uuid TEXT NOT NULL,
        publisher_account_id TEXT NOT NULL,update_json TEXT,resource_json TEXT,
        event_id TEXT NOT NULL,started_at_ms INTEGER NOT NULL,
        revision INTEGER NOT NULL,expires_at_ms INTEGER NOT NULL,
        PRIMARY KEY(scope_id,world_epoch,dimension_id,entity_uuid));")?;
    Ok(())
}
fn text(value: &str, max: usize, empty: bool) -> bool {
    (empty || !value.trim().is_empty())
        && value.encode_utf16().count() <= max
        && !value.chars().any(char::is_control)
}
fn numbers(values: &BTreeMap<String, f64>, key_limit: usize) -> bool {
    values.len() <= 64
        && values.iter().all(|(key, value)| {
            text(key, key_limit, false) && value.is_finite() && (*value as f32).is_finite()
        })
}
fn validate(input: &EntityMotionUpdate, settings: &VisualSettings) -> Result<String, CloudError> {
    let m = &input.motion;
    crate::config::validate_slug(&input.target_id, "target_id")?;
    let variables = m.roaming.len()
        + m.controllers
            .values()
            .map(|c| c.variables.len())
            .sum::<usize>();
    if !matches!(input.entity_kind.as_str(), "MAID" | "FAKE_PLAYER")
        || [
            input.binding_revision,
            input.appearance_revision,
            input.expected_revision,
            m.started_at_unix_ms,
        ]
        .iter()
        .any(|v| *v >= 9_007_199_254_740_991)
        || !text(&m.event_id, 64, false)
        || !text(&m.animation_key, 256, true)
        || !numbers(&m.roaming, 32)
        || variables > settings.limits.max_visual_variables
        || m.expressions.len() > 16
        || m.controllers.len() > 64
        || m.expressions.iter().any(|e| {
            !text(&e.event_id, 64, false)
                || !text(&e.expression, 2048, true)
                || e.started_at_unix_ms >= 9_007_199_254_740_991
                || e.values.len() > 16
                || e.values
                    .iter()
                    .any(|v| !v.is_finite() || !(*v as f32).is_finite())
        })
        || m.controllers.iter().any(|(name, c)| {
            !text(name, 128, false)
                || !text(&c.state, 128, true)
                || c.started_at_unix_ms >= 9_007_199_254_740_991
                || !numbers(&c.variables, 64)
        })
    {
        return Err(CloudError::invalid_metadata("invalid entity motion"));
    }
    let encoded = serde_json::to_string(input)
        .map_err(|_| CloudError::invalid_metadata("invalid motion JSON"))?;
    if encoded.len() > settings.limits.max_visual_state_bytes {
        return Err(CloudError::MessageTooLarge);
    }
    Ok(encoded)
}
fn authorized_resource(
    conn: &Connection,
    account: &str,
    scope: &str,
    entity: Uuid,
    input: &EntityMotionUpdate,
    edit: bool,
) -> Result<ResourceRef, CloudError> {
    let data:Option<AuthorizedAppearanceRow>=conn.query_row(
        "SELECT b.target_id,b.revision,b.entity_kind,a.revision,a.asset_id,a.asset_revision,a.raw_sha256,a.texture_id,r.format,a.disabled,s.role,acl.role
        FROM entity_bindings b JOIN targets t ON t.target_id=b.target_id AND t.scope_id=b.scope_id AND t.target_kind=b.entity_kind
        JOIN scope_acl s ON s.scope_id=b.scope_id AND s.account_id=?4 JOIN target_acl acl ON acl.target_id=b.target_id AND acl.account_id=?4
        JOIN appearances a ON a.target_id=b.target_id LEFT JOIN asset_revisions r ON r.asset_id=a.asset_id AND r.revision=a.asset_revision AND r.raw_sha256=a.raw_sha256
        WHERE b.scope_id=?1 AND b.world_epoch=?2 AND b.entity_uuid=?3",params![scope,input.world_epoch,entity.to_string(),account],
        |r|Ok((r.get(0)?,r.get::<_,i64>(1)? as u64,r.get(2)?,r.get::<_,i64>(3)? as u64,r.get(4)?,r.get::<_,Option<i64>>(5)?.map(|v|v as u64),r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?))).optional()?;
    let Some((
        target,
        binding,
        kind,
        appearance,
        Some(asset),
        Some(revision),
        Some(sha),
        Some(texture),
        Some(format),
        false,
        scope_role,
        target_role,
    )) = data
    else {
        return Err(CloudError::AccessDenied);
    };
    if target != input.target_id
        || binding != input.binding_revision
        || kind != input.entity_kind
        || appearance != input.appearance_revision
    {
        return Err(CloudError::RevisionConflict);
    }
    if edit
        && [scope_role.as_str(), target_role.as_str()]
            .iter()
            .any(|role| !matches!(*role, "edit" | "editor" | "manage" | "owner"))
    {
        return Err(CloudError::AccessDenied);
    }
    let resource = ResourceRef {
        asset_id: asset,
        asset_revision: revision,
        raw_sha256: sha,
        texture_id: texture,
        format,
    };
    visual::resource(conn, account, &resource)?;
    Ok(resource)
}
impl CloudStore {
    pub fn entity_motion_revisions(
        &self,
        account: &str,
        scope: &str,
        query: &SnapshotQuery,
    ) -> Result<Vec<MotionRevision>, CloudError> {
        if query.entity_uuids.len() > self.visual.limits.max_entity_query_count {
            return Err(CloudError::invalid_metadata("entity query limit exceeded"));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        visual::context(
            &conn,
            account,
            scope,
            &query.world_epoch,
            &query.dimension_id,
        )?;
        let mut result = Vec::new();
        for id in &query.entity_uuids {
            let revision:Option<i64>=conn.query_row("SELECT COALESCE(m.revision,0) FROM entity_bindings b
                JOIN targets t ON t.target_id=b.target_id AND t.scope_id=b.scope_id AND t.target_kind=b.entity_kind
                JOIN scope_acl s ON s.scope_id=b.scope_id AND s.account_id=?4 JOIN target_acl a ON a.target_id=b.target_id AND a.account_id=?4
                LEFT JOIN entity_motion_states m ON m.scope_id=b.scope_id AND m.world_epoch=b.world_epoch AND m.dimension_id=?5 AND m.entity_uuid=b.entity_uuid
                WHERE b.scope_id=?1 AND b.world_epoch=?2 AND b.entity_uuid=?3 AND b.entity_kind IN ('MAID','FAKE_PLAYER')
                AND s.role IN ('edit','editor','manage','owner') AND a.role IN ('edit','editor','manage','owner')",
                params![scope,query.world_epoch,id.to_string(),account,query.dimension_id],|r|r.get(0)).optional()?;
            if let Some(revision) = revision {
                result.push(MotionRevision {
                    entity_uuid: *id,
                    revision: revision as u64,
                });
            }
        }
        Ok(result)
    }
    pub fn withdraw_entity_motion(
        &self,
        account: &str,
        scope: &str,
        id: Uuid,
        input: &MotionWithdrawal,
    ) -> Result<u64, CloudError> {
        crate::config::validate_slug(scope, "scope_id")?;
        crate::config::validate_slug(&input.world_epoch, "world_epoch")?;
        if !visual::identifier(&input.dimension_id)
            || input.expected_revision >= 9_007_199_254_740_991
        {
            return Err(CloudError::invalid_metadata("invalid motion withdrawal"));
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let row:Option<(String,i64)>=tx.query_row("SELECT publisher_account_id,revision FROM entity_motion_states WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4",
            params![scope,input.world_epoch,input.dimension_id,id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((owner, revision)) = row else {
            return Err(CloudError::NotFound);
        };
        if owner != account {
            return Err(CloudError::AccessDenied);
        }
        if revision as u64 != input.expected_revision {
            return Err(CloudError::RevisionConflict);
        }
        tx.execute("UPDATE entity_motion_states SET update_json=NULL,resource_json=NULL,expires_at_ms=0,revision=?5 WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4",
            params![scope,input.world_epoch,input.dimension_id,id.to_string(),revision+1])?;
        tx.commit()?;
        Ok(revision as u64 + 1)
    }
    pub fn publish_entity_motion(
        &self,
        account: &str,
        scope: &str,
        entity: Uuid,
        input: &EntityMotionUpdate,
        time: i64,
    ) -> Result<EntityMotionEntry, CloudError> {
        let text = validate(input, &self.visual)?;
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        visual::context(&tx, account, scope, &input.world_epoch, &input.dimension_id)?;
        let resource = authorized_resource(&tx, account, scope, entity, input, true)?;
        let old:Option<(u64,String,Option<String>,i64,String,u64)>=tx.query_row("SELECT revision,publisher_account_id,update_json,expires_at_ms,event_id,started_at_ms FROM entity_motion_states WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4",
            params![scope,input.world_epoch,input.dimension_id,entity.to_string()],|r|Ok((r.get::<_,i64>(0)? as u64,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get::<_,i64>(5)? as u64))).optional()?;
        let revision = old.as_ref().map_or(0, |row| row.0);
        if revision != input.expected_revision {
            return Err(CloudError::RevisionConflict);
        }
        if let Some((_, owner, previous, expiry, event, started)) = &old {
            if owner != account && *expiry > time {
                return Err(CloudError::RevisionConflict);
            }
            if event == &input.motion.event_id
                && (*expiry <= time || started != &input.motion.started_at_unix_ms)
            {
                return Err(CloudError::IdempotencyConflict);
            }
            if let Some(previous) = previous {
                let previous: EntityMotionUpdate = serde_json::from_str(previous)
                    .map_err(|_| CloudError::configuration("invalid stored motion"))?;
                if previous.motion.event_id == input.motion.event_id
                    && (previous.motion.animation_key != input.motion.animation_key
                        || previous.motion.started_at_unix_ms != input.motion.started_at_unix_ms)
                {
                    return Err(CloudError::IdempotencyConflict);
                }
                if previous.binding_revision == input.binding_revision
                    && previous.appearance_revision == input.appearance_revision
                    && previous.motion.started_at_unix_ms > input.motion.started_at_unix_ms
                {
                    return Err(CloudError::RevisionConflict);
                }
            }
        }
        visual::rate(&tx, account, &self.visual, time)?;
        let next = revision + 1;
        let expiry = time + 60_000;
        let resource_json = serde_json::to_string(&resource)
            .map_err(|_| CloudError::invalid_metadata("invalid resource JSON"))?;
        tx.execute("INSERT INTO entity_motion_states(scope_id,world_epoch,dimension_id,entity_uuid,publisher_account_id,update_json,resource_json,revision,expires_at_ms,event_id,started_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
            ON CONFLICT(scope_id,world_epoch,dimension_id,entity_uuid) DO UPDATE SET publisher_account_id=excluded.publisher_account_id,update_json=excluded.update_json,resource_json=excluded.resource_json,revision=excluded.revision,expires_at_ms=excluded.expires_at_ms,event_id=excluded.event_id,started_at_ms=excluded.started_at_ms",
            params![scope,input.world_epoch,input.dimension_id,entity.to_string(),account,text,resource_json,next as i64,expiry,input.motion.event_id,input.motion.started_at_unix_ms as i64])?;
        tx.commit()?;
        Ok(EntityMotionEntry {
            entity_uuid: entity,
            update: input.clone(),
            resource,
            revision: next,
            server_time_unix_ms: time,
            expires_at_unix_ms: expiry,
        })
    }
    pub fn query_entity_motion(
        &self,
        account: &str,
        scope: &str,
        query: &SnapshotQuery,
        time: i64,
    ) -> Result<Vec<EntityMotionEntry>, CloudError> {
        if query.entity_uuids.len() > self.visual.limits.max_entity_query_count {
            return Err(CloudError::invalid_metadata("entity query limit exceeded"));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        visual::context(
            &conn,
            account,
            scope,
            &query.world_epoch,
            &query.dimension_id,
        )?;
        let mut result = Vec::new();
        for entity in &query.entity_uuids {
            let row:Option<(String,String,String,u64,i64)>=conn.query_row("SELECT publisher_account_id,update_json,resource_json,revision,expires_at_ms FROM entity_motion_states WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4 AND expires_at_ms>?5 AND update_json IS NOT NULL AND resource_json IS NOT NULL",
                params![scope,query.world_epoch,query.dimension_id,entity.to_string(),time],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get::<_,i64>(3)? as u64,r.get(4)?))).optional()?;
            let Some((publisher, text, resource_text, revision, expiry)) = row else {
                continue;
            };
            let update: EntityMotionUpdate = serde_json::from_str(&text)
                .map_err(|_| CloudError::configuration("invalid stored motion"))?;
            if visual::context(
                &conn,
                &publisher,
                scope,
                &query.world_epoch,
                &query.dimension_id,
            )
            .is_err()
            {
                continue;
            }
            let Ok(resource) =
                authorized_resource(&conn, &publisher, scope, *entity, &update, true)
            else {
                continue;
            };
            if authorized_resource(&conn, account, scope, *entity, &update, false).is_err()
                || serde_json::to_string(&resource).ok().as_deref() != Some(&resource_text)
            {
                continue;
            }
            result.push(EntityMotionEntry {
                entity_uuid: *entity,
                update,
                resource,
                revision,
                server_time_unix_ms: time,
                expires_at_unix_ms: expiry,
            });
        }
        Ok(result)
    }
}
pub async fn put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope, entity)): Path<(String, Uuid)>,
    payload: Result<crate::visual_json::VisualJson<EntityMotionUpdate>, CloudError>,
) -> Result<Json<EntityMotionEntry>, CloudError> {
    if !state.runtime_config().visual.features.entity_motion {
        return Err(CloudError::NotFound);
    }
    let account = authenticate(&state, &headers)?;
    let crate::visual_json::VisualJson(body) = payload?;
    Ok(Json(state.store.publish_entity_motion(
        &account,
        &scope,
        entity,
        &body,
        visual::now_ms(),
    )?))
}
pub async fn query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
    payload: Result<crate::visual_json::VisualJson<SnapshotQuery>, CloudError>,
) -> Result<Json<serde_json::Value>, CloudError> {
    if !state.runtime_config().visual.features.entity_motion {
        return Err(CloudError::NotFound);
    }
    let account = authenticate(&state, &headers)?;
    let crate::visual_json::VisualJson(body) = payload?;
    Ok(Json(
        serde_json::json!({"entries":state.store.query_entity_motion(&account,&scope,&body,visual::now_ms())?}),
    ))
}
pub async fn revisions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
    payload: Result<crate::visual_json::VisualJson<SnapshotQuery>, CloudError>,
) -> Result<Json<serde_json::Value>, CloudError> {
    if !state.runtime_config().visual.features.entity_motion {
        return Err(CloudError::NotFound);
    }
    let account = authenticate(&state, &headers)?;
    let crate::visual_json::VisualJson(body) = payload?;
    Ok(Json(
        serde_json::json!({"entries":state.store.entity_motion_revisions(&account,&scope,&body)?}),
    ))
}
pub async fn withdraw(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope, id)): Path<(String, Uuid)>,
    payload: Result<crate::visual_json::VisualJson<MotionWithdrawal>, CloudError>,
) -> Result<Json<serde_json::Value>, CloudError> {
    if !state.runtime_config().visual.features.entity_motion {
        return Err(CloudError::NotFound);
    }
    let account = authenticate(&state, &headers)?;
    let crate::visual_json::VisualJson(body) = payload?;
    Ok(Json(
        serde_json::json!({"revision":state.store.withdraw_entity_motion(&account,&scope,id,&body)?}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, CloudStore, EntityMotionUpdate, Uuid) {
        let (dir, store, _, shot) = crate::visual::tests::fixture();
        let id = shot.entity_uuid;
        store.connection.lock().unwrap().execute_batch(&format!("UPDATE scope_acl SET role='edit' WHERE account_id='shooter';
            INSERT INTO targets(target_id,scope_id,target_kind,display_name,owner_account_id) VALUES('target-maid','test-scope','MAID','Maid','shooter');
            INSERT INTO target_acl(target_id,account_id,role) VALUES('target-maid','shooter','edit'),('target-maid','observer','view');
            INSERT INTO appearances(target_id,revision,asset_id,asset_revision,raw_sha256,texture_id) VALUES('target-maid',1,'model-arrow',1,'{}','default');
            INSERT INTO entity_bindings(binding_id,scope_id,world_epoch,entity_uuid,entity_kind,target_id,revision) VALUES('binding-maid','test-scope','world-reset-3','{}','MAID','target-maid',1);", "a".repeat(64),id)).unwrap();
        let update =
            serde_json::from_str(include_str!("../fixtures/entity-motion-v1.json")).unwrap();
        (dir, store, update, id)
    }
    fn query_for(id: Uuid) -> SnapshotQuery {
        SnapshotQuery {
            world_epoch: "world-reset-3".into(),
            dimension_id: "minecraft:overworld".into(),
            entity_uuids: vec![id],
        }
    }
    #[test]
    fn timeline_is_bound_to_target_binding_appearance_and_explicit_edit_rights() {
        let (_dir, store, mut update, id) = setup();
        assert!(matches!(
            store.publish_entity_motion("observer", "test-scope", id, &update, 1000),
            Err(CloudError::AccessDenied)
        ));
        assert_eq!(
            store
                .publish_entity_motion("shooter", "test-scope", id, &update, 1000)
                .unwrap()
                .revision,
            1
        );
        assert!(matches!(
            store.publish_entity_motion("shooter", "test-scope", id, &update, 1001),
            Err(CloudError::RevisionConflict)
        ));
        update.expected_revision = 1;
        update.motion.roaming.insert("variable.pose".into(), 2.0);
        let heartbeat = store
            .publish_entity_motion("shooter", "test-scope", id, &update, 2000)
            .unwrap();
        assert_eq!(heartbeat.update.motion.started_at_unix_ms, 1000);
        assert_eq!(heartbeat.update.motion.event_id, "play-1");
        update.expected_revision = 2;
        update.motion.animation_key = "other".into();
        assert!(matches!(
            store.publish_entity_motion("shooter", "test-scope", id, &update, 2001),
            Err(CloudError::IdempotencyConflict)
        ));
        update.motion.animation_key = "wave".into();
        update.binding_revision = 2;
        assert!(matches!(
            store.publish_entity_motion("shooter", "test-scope", id, &update, 2001),
            Err(CloudError::RevisionConflict)
        ));
        assert_eq!(
            store
                .query_entity_motion("observer", "test-scope", &query_for(id), 2001)
                .unwrap()
                .len(),
            1
        );
        store
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE appearances SET revision=2", [])
            .unwrap();
        assert!(
            store
                .query_entity_motion("observer", "test-scope", &query_for(id), 2001)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn expiry_privacy_acl_and_epoch_prevent_stale_replay_and_cross_world_reads() {
        let (_dir, store, mut update, id) = setup();
        store
            .publish_entity_motion("shooter", "test-scope", id, &update, 1000)
            .unwrap();
        let mut query = query_for(id);
        query.dimension_id = "minecraft:the_nether".into();
        assert!(
            store
                .query_entity_motion("observer", "test-scope", &query, 2000)
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            store.query_entity_motion("outsider", "test-scope", &query_for(id), 2000),
            Err(CloudError::ScopeAccessDenied)
        ));
        store.connection.lock().unwrap().execute_batch("UPDATE assets SET visibility='PRIVATE'; INSERT INTO asset_acl(asset_id,account_id,permission) VALUES('model-arrow','shooter','manage');").unwrap();
        assert!(
            store
                .query_entity_motion("observer", "test-scope", &query_for(id), 2000)
                .unwrap()
                .is_empty()
        );
        store.connection.lock().unwrap().execute("INSERT INTO asset_acl(asset_id,account_id,permission) VALUES('model-arrow','observer','render_read')",[]).unwrap();
        assert_eq!(
            store
                .query_entity_motion("observer", "test-scope", &query_for(id), 2000)
                .unwrap()
                .len(),
            1
        );
        store
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM target_acl WHERE account_id='shooter'", [])
            .unwrap();
        assert!(
            store
                .query_entity_motion("observer", "test-scope", &query_for(id), 2000)
                .unwrap()
                .is_empty()
        );
        store.connection.lock().unwrap().execute("INSERT INTO target_acl(target_id,account_id,role) VALUES('target-maid','shooter','edit')",[]).unwrap();
        store.cleanup_visual_states(61000).unwrap();
        let state:(i64,bool)=store.connection.lock().unwrap().query_row("SELECT revision,update_json IS NULL AND resource_json IS NULL FROM entity_motion_states",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(state, (1, true));
        assert!(
            store
                .query_entity_motion("observer", "test-scope", &query_for(id), 61000)
                .unwrap()
                .is_empty()
        );
        update.expected_revision = 1;
        assert!(matches!(
            store.publish_entity_motion("shooter", "test-scope", id, &update, 61001),
            Err(CloudError::IdempotencyConflict)
        ));
        update.motion.event_id = "stop-2".into();
        update.motion.animation_key.clear();
        update.motion.started_at_unix_ms = 61001;
        assert_eq!(
            store
                .publish_entity_motion("shooter", "test-scope", id, &update, 61001)
                .unwrap()
                .revision,
            2
        );
        store
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE scopes SET world_epoch='reset-4'", [])
            .unwrap();
        store.cleanup_visual_states(62000).unwrap();
        assert!(matches!(
            store.query_entity_motion("observer", "test-scope", &query_for(id), 62000),
            Err(CloudError::ScopeAccessDenied)
        ));
    }
    #[test]
    fn shared_fixture_rejects_coercion_unknown_fields_and_non_finite_variables() {
        let (_dir, store, mut update, _) = setup();
        let mut json: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/entity-motion-v1.json")).unwrap();
        json["motion"]["server_ai"] = serde_json::json!(true);
        assert!(serde_json::from_value::<EntityMotionUpdate>(json).is_err());
        update
            .motion
            .roaming
            .insert("variable.pose".into(), f64::MAX);
        assert!(validate(&update, &store.visual).is_err());
        update.motion.roaming.clear();
        update.entity_kind = "PLAYER".into();
        assert!(validate(&update, &store.visual).is_err());
    }
    #[test]
    fn explicit_privacy_withdrawal_clears_payload_and_preserves_authorized_cas() {
        let (_dir, store, update, id) = setup();
        assert_eq!(
            store
                .entity_motion_revisions("shooter", "test-scope", &query_for(id))
                .unwrap()[0]
                .revision,
            0
        );
        assert!(
            store
                .entity_motion_revisions("observer", "test-scope", &query_for(id))
                .unwrap()
                .is_empty()
        );
        store
            .publish_entity_motion("shooter", "test-scope", id, &update, 1000)
            .unwrap();
        let withdrawal = MotionWithdrawal {
            world_epoch: update.world_epoch.clone(),
            dimension_id: update.dimension_id.clone(),
            expected_revision: 1,
        };
        assert!(matches!(
            store.withdraw_entity_motion("observer", "test-scope", id, &withdrawal),
            Err(CloudError::AccessDenied)
        ));
        // Withdrawing one's own publication remains possible after rendering rights are revoked.
        store
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM target_acl WHERE account_id='shooter'", [])
            .unwrap();
        assert_eq!(
            store
                .withdraw_entity_motion("shooter", "test-scope", id, &withdrawal)
                .unwrap(),
            2
        );
        assert!(
            store
                .query_entity_motion("observer", "test-scope", &query_for(id), 1001)
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            store.withdraw_entity_motion("shooter", "test-scope", id, &withdrawal),
            Err(CloudError::RevisionConflict)
        ));
    }
}
