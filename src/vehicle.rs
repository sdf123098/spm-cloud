//! Explicitly authorized vehicle appearance bindings. Riding never creates an ACL.
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleUpdate {
    pub world_epoch: String,
    pub dimension_id: String,
    pub entity_kind: String,
    pub target_id: String,
    pub expected_revision: u64,
    #[serde(deserialize_with = "required_resource")]
    pub resource: Option<ResourceRef>,
    #[serde(default)]
    pub variables: BTreeMap<String, f64>,
}
fn required_resource<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ResourceRef>, D::Error> {
    Option::<ResourceRef>::deserialize(deserializer)
}
#[derive(Serialize, Deserialize)]
pub struct VehicleEntry {
    pub entity_uuid: Uuid,
    pub binding: VehicleUpdate,
    pub revision: u64,
    pub server_time_unix_ms: i64,
    pub expires_at_unix_ms: i64,
}
pub(crate) fn migrate(conn: &Connection) -> Result<(), CloudError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS vehicle_bindings (
        scope_id TEXT NOT NULL REFERENCES scopes(scope_id), world_epoch TEXT NOT NULL,
        dimension_id TEXT NOT NULL, entity_uuid TEXT NOT NULL,
        target_id TEXT NOT NULL REFERENCES targets(target_id),
        publisher_account_id TEXT NOT NULL REFERENCES accounts(account_id),
        binding_json TEXT NOT NULL, revision INTEGER NOT NULL,
        PRIMARY KEY(scope_id,world_epoch,dimension_id,entity_uuid)
    );",
    )?;
    Ok(())
}
fn editable(conn: &Connection, account: &str, scope: &str, target: &str) -> Result<(), CloudError> {
    let allowed: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM targets t
        JOIN scope_acl s ON s.scope_id=t.scope_id AND s.account_id=?1
        JOIN target_acl a ON a.target_id=t.target_id AND a.account_id=?1
        WHERE t.target_id=?2 AND t.scope_id=?3 AND t.target_kind='VEHICLE'
        AND s.role IN ('editor','edit','manage','owner') AND a.role IN ('editor','edit','manage','owner'))",
        params![account, target, scope], |r|r.get(0))?;
    if !allowed {
        return Err(CloudError::AccessDenied);
    }
    Ok(())
}
fn validate(input: &VehicleUpdate, settings: &VisualSettings) -> Result<String, CloudError> {
    crate::config::validate_slug(&input.target_id, "target_id")?;
    if !visual::identifier(&input.entity_kind)
        || input.expected_revision >= 9_007_199_254_740_991
        || input.variables.len() > settings.limits.max_visual_variables
        || input.variables.iter().any(|(key, value)| {
            key.is_empty()
                || key.encode_utf16().count() > 32
                || key.chars().any(char::is_control)
                || !value.is_finite()
                || !(*value as f32).is_finite()
        })
        || input.resource.is_none() && !input.variables.is_empty()
    {
        return Err(CloudError::invalid_metadata("invalid vehicle appearance"));
    }
    let text = serde_json::to_string(input)
        .map_err(|_| CloudError::invalid_metadata("invalid vehicle JSON"))?;
    if text.len() > settings.limits.max_visual_state_bytes {
        return Err(CloudError::MessageTooLarge);
    }
    Ok(text)
}
impl CloudStore {
    pub fn publish_vehicle(
        &self,
        account: &str,
        scope: &str,
        entity: Uuid,
        input: &VehicleUpdate,
        settings: &VisualSettings,
        time: i64,
    ) -> Result<VehicleEntry, CloudError> {
        let text = validate(input, settings)?;
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        visual::context(&tx, account, scope, &input.world_epoch, &input.dimension_id)?;
        editable(&tx, account, scope, &input.target_id)?;
        let previous:Option<(String,String,u64)>=tx.query_row("SELECT target_id,binding_json,revision FROM vehicle_bindings WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4",
            params![scope,input.world_epoch,input.dimension_id,entity.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get::<_,i64>(2)? as u64))).optional()?;
        let revision = previous.as_ref().map_or(0, |r| r.2);
        if revision != input.expected_revision {
            return Err(CloudError::RevisionConflict);
        }
        if let Some((target, old, _)) = &previous {
            editable(&tx, account, scope, target)?;
            let old: VehicleUpdate = serde_json::from_str(old)
                .map_err(|_| CloudError::configuration("invalid stored vehicle binding"))?;
            if old.entity_kind != input.entity_kind {
                return Err(CloudError::RevisionConflict);
            }
        }
        if let Some(resource) = &input.resource {
            visual::resource(&tx, account, resource)?;
        }
        visual::rate(&tx, account, settings, time)?;
        let next = revision + 1;
        tx.execute("INSERT INTO vehicle_bindings(scope_id,world_epoch,dimension_id,entity_uuid,target_id,publisher_account_id,binding_json,revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
            ON CONFLICT(scope_id,world_epoch,dimension_id,entity_uuid) DO UPDATE SET target_id=excluded.target_id,publisher_account_id=excluded.publisher_account_id,binding_json=excluded.binding_json,revision=excluded.revision",
            params![scope,input.world_epoch,input.dimension_id,entity.to_string(),input.target_id,account,text,next as i64])?;
        tx.commit()?;
        Ok(VehicleEntry {
            entity_uuid: entity,
            binding: input.clone(),
            revision: next,
            server_time_unix_ms: time,
            expires_at_unix_ms: time + 60_000,
        })
    }
    pub fn query_vehicles(
        &self,
        account: &str,
        scope: &str,
        query: &SnapshotQuery,
        time: i64,
    ) -> Result<Vec<VehicleEntry>, CloudError> {
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
            let row:Option<(String,String,u64)>=conn.query_row("SELECT publisher_account_id,binding_json,revision FROM vehicle_bindings WHERE scope_id=?1 AND world_epoch=?2 AND dimension_id=?3 AND entity_uuid=?4",
                params![scope,query.world_epoch,query.dimension_id,entity.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get::<_,i64>(2)? as u64))).optional()?;
            let Some((publisher, text, revision)) = row else {
                continue;
            };
            let binding: VehicleUpdate = serde_json::from_str(&text)
                .map_err(|_| CloudError::configuration("invalid stored vehicle binding"))?;
            if editable(&conn, &publisher, scope, &binding.target_id).is_err() {
                continue;
            }
            if let Some(resource) = &binding.resource {
                if visual::resource(&conn, &publisher, resource).is_err()
                    || visual::resource(&conn, account, resource).is_err()
                {
                    continue;
                }
            }
            result.push(VehicleEntry {
                entity_uuid: *entity,
                binding,
                revision,
                server_time_unix_ms: time,
                expires_at_unix_ms: time + 60_000,
            });
        }
        Ok(result)
    }
}
pub async fn put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope, entity)): Path<(String, Uuid)>,
    payload: Result<crate::visual_json::VisualJson<VehicleUpdate>, CloudError>,
) -> Result<Json<VehicleEntry>, CloudError> {
    if !state.runtime_config().visual.features.vehicle_bindings {
        return Err(CloudError::NotFound);
    }
    let account = authenticate(&state, &headers)?;
    let crate::visual_json::VisualJson(body) = payload?;
    Ok(Json(state.store.publish_vehicle(
        &account,
        &scope,
        entity,
        &body,
        &state.runtime_config().visual,
        visual::now_ms(),
    )?))
}
pub async fn query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
    payload: Result<crate::visual_json::VisualJson<SnapshotQuery>, CloudError>,
) -> Result<Json<serde_json::Value>, CloudError> {
    if !state.runtime_config().visual.features.vehicle_bindings {
        return Err(CloudError::NotFound);
    }
    let account = authenticate(&state, &headers)?;
    let crate::visual_json::VisualJson(body) = payload?;
    Ok(Json(
        serde_json::json!({"entries":state.store.query_vehicles(&account,&scope,&body,visual::now_ms())?}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (
        tempfile::TempDir,
        CloudStore,
        VisualSettings,
        VehicleUpdate,
        Uuid,
    ) {
        let (dir, store, settings, shot) = crate::visual::tests::fixture();
        store.connection.lock().unwrap().execute_batch("UPDATE scope_acl SET role='manage' WHERE account_id='shooter';
            INSERT INTO targets(target_id,scope_id,target_kind,display_name,owner_account_id) VALUES ('target-horse','test-scope','VEHICLE','Horse','shooter');
            INSERT INTO target_acl(target_id,account_id,role) VALUES ('target-horse','shooter','manage');").unwrap();
        let update: VehicleUpdate =
            serde_json::from_str(include_str!("../fixtures/vehicle-appearance-v1.json")).unwrap();
        (dir, store, settings, update, shot.entity_uuid)
    }
    fn query_for(id: Uuid) -> SnapshotQuery {
        SnapshotQuery {
            world_epoch: "world-reset-3".into(),
            dimension_id: "minecraft:overworld".into(),
            entity_uuids: vec![id],
        }
    }
    #[test]
    fn omitted_resource_is_rejected_but_explicit_null_unbinds() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/vehicle-appearance-v1.json")).unwrap();
        value.as_object_mut().unwrap().remove("resource");
        assert!(serde_json::from_value::<VehicleUpdate>(value.clone()).is_err());
        value["resource"] = serde_json::Value::Null;
        value["variables"] = serde_json::json!({});
        assert!(
            serde_json::from_value::<VehicleUpdate>(value)
                .unwrap()
                .resource
                .is_none()
        );
    }
    #[test]
    fn explicit_binding_requires_scope_and_target_edit_rights_and_cas() {
        let (_dir, store, settings, mut update, id) = setup();
        assert!(matches!(
            store.publish_vehicle("observer", "test-scope", id, &update, &settings, 1_000),
            Err(CloudError::AccessDenied)
        ));
        assert_eq!(
            store
                .publish_vehicle("shooter", "test-scope", id, &update, &settings, 1_000)
                .unwrap()
                .revision,
            1
        );
        assert!(matches!(
            store.publish_vehicle("shooter", "test-scope", id, &update, &settings, 2_000),
            Err(CloudError::RevisionConflict)
        ));
        update.expected_revision = 1;
        update.entity_kind = "minecraft:pig".into();
        assert!(matches!(
            store.publish_vehicle("shooter", "test-scope", id, &update, &settings, 2_000),
            Err(CloudError::RevisionConflict)
        ));
        update.entity_kind = "minecraft:horse".into();
        update.resource = None;
        update.variables.clear();
        assert_eq!(
            store
                .publish_vehicle("shooter", "test-scope", id, &update, &settings, 3_000)
                .unwrap()
                .revision,
            2
        );
        assert!(
            store
                .query_vehicles("observer", "test-scope", &query_for(id), 3_001)
                .unwrap()[0]
                .binding
                .resource
                .is_none()
        );
        store
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM target_acl WHERE account_id='shooter'", [])
            .unwrap();
        assert!(
            store
                .query_vehicles("observer", "test-scope", &query_for(id), 3_002)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn exact_resource_acl_world_and_dimension_are_checked_on_each_read() {
        let (_dir, store, settings, update, id) = setup();
        store
            .publish_vehicle("shooter", "test-scope", id, &update, &settings, 1_000)
            .unwrap();
        assert_eq!(
            store
                .query_vehicles("observer", "test-scope", &query_for(id), 2_000)
                .unwrap()
                .len(),
            1
        );
        let mut query = query_for(id);
        query.dimension_id = "minecraft:the_nether".into();
        assert!(
            store
                .query_vehicles("observer", "test-scope", &query, 2_000)
                .unwrap()
                .is_empty()
        );
        query = query_for(id);
        query.world_epoch = "old-world".into();
        assert!(matches!(
            store.query_vehicles("observer", "test-scope", &query, 2_000),
            Err(CloudError::ScopeAccessDenied)
        ));
        assert!(matches!(
            store.query_vehicles("outsider", "test-scope", &query_for(id), 2_000),
            Err(CloudError::ScopeAccessDenied)
        ));
        store.connection.lock().unwrap().execute_batch("UPDATE assets SET visibility='PRIVATE',current_revision=2; INSERT INTO asset_acl(asset_id,account_id,permission) VALUES ('model-arrow','shooter','manage');").unwrap();
        assert!(
            store
                .query_vehicles("observer", "test-scope", &query_for(id), 3_000)
                .unwrap()
                .is_empty()
        );
        store.connection.lock().unwrap().execute("INSERT INTO asset_acl(asset_id,account_id,permission) VALUES ('model-arrow','observer','render_read')",[]).unwrap();
        assert_eq!(
            store
                .query_vehicles("observer", "test-scope", &query_for(id), 3_000)
                .unwrap()[0]
                .binding
                .resource
                .as_ref()
                .unwrap()
                .asset_revision,
            1
        );
        store
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM asset_acl WHERE account_id='shooter'", [])
            .unwrap();
        assert!(
            store
                .query_vehicles("observer", "test-scope", &query_for(id), 3_001)
                .unwrap()
                .is_empty()
        );
        let mut oversized = update.clone();
        oversized.variables.insert("bad".into(), f64::INFINITY);
        assert!(validate(&oversized, &settings).is_err());
    }
}
