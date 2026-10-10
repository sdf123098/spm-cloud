//! World-scoped, Cloud-ACL-managed entity appearances without a Minecraft server plugin.
use crate::{
    CloudStore,
    api::{AppState, authenticate},
    error::CloudError,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct EntityUpdate {
    pub entity_kind: String,
    pub display_name: String,
    pub expected_revision: u64,
    pub asset_id: Option<String>,
    pub asset_revision: Option<u64>,
    pub raw_sha256: Option<String>,
    pub texture_id: Option<String>,
    #[serde(default)]
    pub share_model: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CloudConfig;
    use tempfile::{TempDir, tempdir};

    const WORLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const SHA1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const SHA2: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    fn fixture() -> (TempDir, CloudStore, uuid::Uuid) {
        let dir = tempdir().unwrap();
        let config = CloudConfig {
            instance_id: "entity-test".into(),
            origin: "https://localhost".into(),
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            database_path: dir.path().join("test.db"),
            object_dir: dir.path().join("objects"),
            access_token: Some("test-only".into()),
            bootstrap_account_id: "owner".into(),
            bootstrap_password_hash: None,
            allow_self_registration: false,
            max_asset_bytes: 1024,
            max_message_bytes: 16384,
            max_entity_query_count: 64,
            visual: Default::default(),
            trusted_proxy_ips: vec![],
        };
        let store = CloudStore::open(&config).unwrap();
        store.connection.lock().unwrap().execute_batch("INSERT INTO accounts(account_id,created_at) VALUES ('observer',datetime('now')),('editor',datetime('now'));").unwrap();
        let old = dir.path().join("old");
        let new = dir.path().join("new");
        std::fs::write(&old, "old").unwrap();
        std::fs::write(&new, "new").unwrap();
        store
            .register_asset("owner", "private-model", "Model", "ysm", SHA1, 3, &old)
            .unwrap();
        store
            .register_asset("owner", "private-model", "Model", "ysm", SHA2, 3, &new)
            .unwrap();
        (
            dir,
            store,
            uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap(),
        )
    }
    fn update(revision: u64, share: bool) -> EntityUpdate {
        EntityUpdate {
            entity_kind: "FAKE_PLAYER".into(),
            display_name: "Test fake".into(),
            expected_revision: revision,
            asset_id: Some("private-model".into()),
            asset_revision: Some(1),
            raw_sha256: Some(SHA1.into()),
            texture_id: Some("default".into()),
            share_model: share,
        }
    }
    fn asset(revision: u64, sha: &str) -> AssetQuery {
        AssetQuery {
            asset_revision: revision,
            raw_sha256: sha.into(),
            asset_id: Some("private-model".into()),
        }
    }

    #[test]
    fn entity_world_discovery_never_grants_editing_and_cas_preserves_binding() {
        let (_dir, store, id) = fixture();
        assert!(
            store
                .entity_entries("observer", WORLD, &[id])
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            store.entity_world("observer", WORLD, false),
            Err(CloudError::NotFound)
        ));
        assert_eq!(
            store.entity_world("owner", WORLD, true).unwrap(),
            store.entity_world("observer", WORLD, true).unwrap()
        );
        assert!(matches!(
            store.publish_entity("observer", WORLD, id, &update(0, true)),
            Err(CloudError::AccessDenied)
        ));
        let first = store
            .publish_entity("owner", WORLD, id, &update(0, true))
            .unwrap();
        assert_eq!(first["revision"], 1);
        assert!(matches!(
            store.publish_entity("owner", WORLD, id, &update(0, true)),
            Err(CloudError::RevisionConflict)
        ));
        let mut wrong_kind = update(1, true);
        wrong_kind.entity_kind = "MAID".into();
        assert!(matches!(
            store.publish_entity("owner", WORLD, id, &wrong_kind),
            Err(CloudError::RevisionConflict)
        ));
        assert!(
            store
                .entity_entries("observer", OTHER, &[id])
                .unwrap()
                .is_empty()
        );
        let count: i64 = store
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM entity_bindings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        assert!(matches!(
            store.entity_entries("owner", WORLD, &vec![id; 65]),
            Err(CloudError::InvalidMetadata(_))
        ));
    }

    #[test]
    fn entity_share_is_exact_scoped_and_revoked_by_permission_or_selection_change() {
        let (dir, store, id) = fixture();
        store.entity_world("owner", WORLD, true).unwrap();
        store
            .publish_entity("owner", WORLD, id, &update(0, false))
            .unwrap();
        assert!(store.entity_entries("observer", WORLD, &[id]).unwrap()[0]["selection"].is_null());
        assert!(matches!(
            store.entity_asset("observer", WORLD, id, &asset(1, SHA1)),
            Err(CloudError::AccessDenied)
        ));
        store
            .publish_entity("owner", WORLD, id, &update(1, true))
            .unwrap();
        assert_eq!(
            store.entity_entries("observer", WORLD, &[id]).unwrap()[0]["selection"]["asset_revision"],
            1
        );
        assert_eq!(
            store
                .entity_asset("observer", WORLD, id, &asset(1, SHA1))
                .unwrap()
                .0,
            dir.path().join("old").to_string_lossy()
        );
        assert!(matches!(
            store.entity_asset("observer", OTHER, id, &asset(1, SHA1)),
            Err(CloudError::NotFound)
        ));
        store.connection.lock().unwrap().execute_batch("UPDATE assets SET owner_account_id='editor' WHERE asset_id='private-model'; DELETE FROM asset_acl WHERE account_id='owner';").unwrap();
        assert!(store.entity_entries("observer", WORLD, &[id]).unwrap()[0]["selection"].is_null());
        assert!(matches!(
            store.entity_asset("observer", WORLD, id, &asset(1, SHA1)),
            Err(CloudError::AccessDenied)
        ));
        store.connection.lock().unwrap().execute_batch("UPDATE assets SET owner_account_id='owner'; INSERT INTO asset_acl(asset_id,account_id,permission) VALUES ('private-model','owner','manage');").unwrap();
        let mut next = update(2, true);
        next.asset_revision = Some(2);
        next.raw_sha256 = Some(SHA2.into());
        store.publish_entity("owner", WORLD, id, &next).unwrap();
        assert!(matches!(
            store.entity_asset("observer", WORLD, id, &asset(1, SHA1)),
            Err(CloudError::RevisionConflict)
        ));
        next.expected_revision = 3;
        next.asset_id = None;
        assert_eq!(
            store.publish_entity("owner", WORLD, id, &next).unwrap()["revision"],
            4
        );
        assert!(store.entity_entries("observer", WORLD, &[id]).unwrap()[0]["selection"].is_null());
        assert!(matches!(
            store.entity_asset("observer", WORLD, id, &asset(2, SHA2)),
            Err(CloudError::RevisionConflict)
        ));
        store
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM asset_acl WHERE account_id='owner'", [])
            .unwrap();
        assert_eq!(
            store
                .publish_entity("owner", WORLD, id, &update(4, true))
                .unwrap()["revision"],
            5
        );
        assert_eq!(
            store.entity_entries("observer", WORLD, &[id]).unwrap()[0]["selection"]["asset_revision"],
            1
        );
    }

    #[test]
    fn entity_editor_needs_both_acls_and_cannot_redistribute_borrowed_private_asset() {
        let (_dir, store, id) = fixture();
        store.entity_world("owner", WORLD, true).unwrap();
        let target = store
            .publish_entity("owner", WORLD, id, &update(0, true))
            .unwrap()["target_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let conn = store.connection.lock().unwrap();
        conn.execute(
            "INSERT INTO scope_acl(scope_id,account_id,role) VALUES (?1,'editor','edit')",
            [format!("entity_{WORLD}")],
        )
        .unwrap();
        conn.execute("INSERT INTO asset_acl(asset_id,account_id,permission) VALUES ('private-model','editor','render_read')", []).unwrap();
        drop(conn);
        assert!(matches!(
            store.publish_entity("editor", WORLD, id, &update(1, false)),
            Err(CloudError::AccessDenied)
        ));
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO target_acl(target_id,account_id,role) VALUES (?1,'editor','edit')",
                [&target],
            )
            .unwrap();
        assert!(matches!(
            store.publish_entity("editor", WORLD, id, &update(1, true)),
            Err(CloudError::AccessDenied)
        ));
        assert_eq!(
            store
                .publish_entity("editor", WORLD, id, &update(1, false))
                .unwrap()["revision"],
            2
        );
        assert!(store.entity_entries("observer", WORLD, &[id]).unwrap()[0]["selection"].is_null());
    }
}
#[derive(Deserialize)]
pub struct EntityQuery {
    pub entity_uuids: Vec<uuid::Uuid>,
}
#[derive(Deserialize)]
pub struct AssetQuery {
    pub asset_revision: u64,
    pub raw_sha256: String,
    pub asset_id: Option<String>,
}

pub async fn get_world(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.entity_world(&account, &key, false)?))
}
pub async fn create_world(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.entity_world(&account, &key, true)?))
}
pub async fn query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
    Json(input): Json<EntityQuery>,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(
        json!({"entries":state.store.entity_entries(&account,&key,&input.entity_uuids)?}),
    ))
}
pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((key, id)): Path<(String, uuid::Uuid)>,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.entity_entries(&account,&key,&[id])?.into_iter().next().unwrap_or_else(||json!({"entity_uuid":id,"entity_kind":null,"target_id":null,"binding_revision":0,"revision":0,"selection":null}))))
}
pub async fn put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((key, id)): Path<(String, uuid::Uuid)>,
    Json(input): Json<EntityUpdate>,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(
        state.store.publish_entity(&account, &key, id, &input)?,
    ))
}
pub async fn asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((key, id)): Path<(String, uuid::Uuid)>,
    Query(input): Query<AssetQuery>,
) -> Result<Response, CloudError> {
    let account = authenticate(&state, &headers)?;
    let (path, sha) = state.store.entity_asset(&account, &key, id, &input)?;
    let file = tokio::fs::File::open(path).await?;
    let mut response =
        axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file)).into_response();
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("content-type", "application/octet-stream".parse().unwrap());
    response
        .headers_mut()
        .insert("etag", format!("\"{sha}\"").parse().unwrap());
    Ok(response)
}
fn key(value: &str) -> Result<String, CloudError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(CloudError::invalid_metadata(
            "world key must be 64 lowercase hex characters",
        ));
    }
    Ok(format!("entity_{value}"))
}
fn editor(
    conn: &Connection,
    account: &str,
    scope: &str,
    target: Option<&str>,
) -> Result<(), CloudError> {
    let sql = if target.is_some() {
        "SELECT role FROM target_acl WHERE target_id=?1 AND account_id=?2"
    } else {
        "SELECT role FROM scope_acl WHERE scope_id=?1 AND account_id=?2"
    };
    let role: Option<String> = conn
        .query_row(sql, params![target.unwrap_or(scope), account], |r| r.get(0))
        .optional()?;
    if !role
        .as_deref()
        .is_some_and(|v| matches!(v, "editor" | "edit" | "manage" | "owner"))
    {
        return Err(CloudError::AccessDenied);
    }
    Ok(())
}
fn bindings(
    conn: &Connection,
    scope: &str,
    epoch: &str,
    id: uuid::Uuid,
) -> Result<Option<(String, String, i64)>, CloudError> {
    Ok(conn.query_row("SELECT b.target_id,b.entity_kind,b.revision FROM entity_bindings b JOIN targets t ON t.target_id=b.target_id AND t.scope_id=b.scope_id AND t.target_kind=b.entity_kind WHERE b.scope_id=?1 AND b.world_epoch=?2 AND b.entity_uuid=?3 AND b.entity_kind IN ('MAID','FAKE_PLAYER')",params![scope,epoch,id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?)
}
fn selection(conn: &Connection, account: &str, target: &str) -> Result<(u64, Value), CloudError> {
    let row=conn.query_row("SELECT p.revision,p.asset_id,p.asset_revision,p.raw_sha256,p.texture_id,p.disabled,r.format,
        CASE WHEN a.visibility='PUBLIC' OR acl.permission IN ('manage','render_read') OR (sharing.target_id IS NOT NULL AND (a.owner_account_id=sharing.shared_by_account_id OR sharing_acl.permission='manage')) THEN 1 ELSE 0 END
        FROM appearances p LEFT JOIN assets a ON a.asset_id=p.asset_id
        LEFT JOIN asset_revisions r ON r.asset_id=p.asset_id AND r.revision=p.asset_revision AND r.raw_sha256=p.raw_sha256
        LEFT JOIN asset_acl acl ON acl.asset_id=p.asset_id AND acl.account_id=?1
        LEFT JOIN entity_model_shares sharing ON sharing.target_id=p.target_id AND sharing.asset_id=p.asset_id AND sharing.asset_revision=p.asset_revision AND sharing.raw_sha256=p.raw_sha256
        LEFT JOIN asset_acl sharing_acl ON sharing_acl.asset_id=sharing.asset_id AND sharing_acl.account_id=sharing.shared_by_account_id
        WHERE p.target_id=?2",params![account,target],|r|Ok((r.get::<_,i64>(0)? as u64,r.get::<_,Option<String>>(1)?,r.get::<_,Option<i64>>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,i64>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,i64>(7)?)))?;
    let selected = if row.5 == 0 && row.7 == 1 && row.1.is_some() && row.6.is_some() {
        json!({"asset_id":row.1,"asset_revision":row.2,"raw_sha256":row.3,"texture_id":row.4,"format":row.6})
    } else {
        Value::Null
    };
    Ok((row.0, selected))
}
impl CloudStore {
    pub fn entity_world(
        &self,
        account: &str,
        epoch: &str,
        create: bool,
    ) -> Result<Value, CloudError> {
        let scope = key(epoch)?;
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        if create {
            let created=tx.execute("INSERT OR IGNORE INTO scopes(scope_id,tenant_id,name,world_epoch,offline_policy,created_at) VALUES (?1,?2,'Shared entity world',?3,'STRICT_APPROVAL',datetime('now'))",params![scope,account,epoch])?;
            if created == 1 {
                tx.execute(
                    "INSERT INTO scope_acl(scope_id,account_id,role) VALUES (?1,?2,'manage')",
                    params![scope, account],
                )?;
            }
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM scopes WHERE scope_id=?1 AND world_epoch=?2)",
            params![scope, epoch],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(CloudError::NotFound);
        }
        tx.commit()?;
        Ok(json!({"scope_id":scope,"name":"Shared entity world","world_epoch":epoch}))
    }
    pub fn entity_entries(
        &self,
        account: &str,
        epoch: &str,
        ids: &[uuid::Uuid],
    ) -> Result<Vec<Value>, CloudError> {
        let scope = key(epoch)?;
        if ids.len() > self.max_entity_query_count {
            return Err(CloudError::invalid_metadata(format!(
                "entity_uuids must contain at most {} UUIDs",
                self.max_entity_query_count
            )));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let valid: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM scopes WHERE scope_id=?1 AND world_epoch=?2)",
            params![scope, epoch],
            |r| r.get(0),
        )?;
        if !valid {
            return Ok(vec![]);
        }
        let mut seen = std::collections::HashSet::new();
        let mut entries = vec![];
        for id in ids {
            if !seen.insert(id) {
                continue;
            }
            if let Some((target, kind, revision)) = bindings(&conn, &scope, epoch, *id)? {
                let (appearance, selected) = selection(&conn, account, &target)?;
                entries.push(json!({"entity_uuid":id,"entity_kind":kind,"target_id":target,"binding_revision":revision,"revision":appearance,"selection":selected}));
            }
        }
        Ok(entries)
    }
    pub fn publish_entity(
        &self,
        account: &str,
        epoch: &str,
        id: uuid::Uuid,
        input: &EntityUpdate,
    ) -> Result<Value, CloudError> {
        let scope = key(epoch)?;
        if !matches!(input.entity_kind.as_str(), "MAID" | "FAKE_PLAYER")
            || input.display_name.is_empty()
            || input.display_name.chars().count() > 256
            || input.display_name.chars().any(char::is_control)
            || input.expected_revision >= 9_007_199_254_740_991
        {
            return Err(CloudError::invalid_metadata("invalid entity appearance"));
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        let valid: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM scopes WHERE scope_id=?1 AND world_epoch=?2)",
            params![scope, epoch],
            |r| r.get(0),
        )?;
        if !valid {
            return Err(CloudError::NotFound);
        }
        editor(&tx, account, &scope, None)?;
        let previous = bindings(&tx, &scope, epoch, id)?;
        if previous.as_ref().is_some_and(|p| p.1 != input.entity_kind) {
            return Err(CloudError::RevisionConflict);
        }
        if previous.is_none() && input.expected_revision != 0 {
            return Err(CloudError::RevisionConflict);
        }
        if let Some(p) = &previous {
            editor(&tx, account, &scope, Some(&p.0))?;
        }
        let (asset, asset_revision, sha, texture) = if let Some(asset) = &input.asset_id {
            if asset.is_empty() || asset.len() > 128 || asset.chars().any(char::is_control) {
                return Err(CloudError::invalid_metadata("invalid asset id"));
            }
            let revision = input
                .asset_revision
                .filter(|v| *v > 0 && *v < 9_007_199_254_740_991)
                .ok_or_else(|| CloudError::invalid_metadata("asset revision required"))?;
            let sha = input
                .raw_sha256
                .as_ref()
                .filter(|s| {
                    s.len() == 64
                        && s.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
                .ok_or_else(|| CloudError::invalid_metadata("SHA256 required"))?;
            let texture = input
                .texture_id
                .as_ref()
                .filter(|s| {
                    !s.is_empty() && s.chars().count() <= 256 && !s.chars().any(char::is_control)
                })
                .ok_or_else(|| CloudError::invalid_metadata("texture required"))?;
            let refrow:Option<(String,String,String,String,Option<String>)>=tx.query_row("SELECT r.raw_sha256,r.format,a.visibility,a.owner_account_id,acl.permission FROM asset_revisions r JOIN assets a ON a.asset_id=r.asset_id LEFT JOIN asset_acl acl ON acl.asset_id=a.asset_id AND acl.account_id=?1 WHERE r.asset_id=?2 AND r.revision=?3",params![account,asset,revision as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
            let r = refrow.ok_or(CloudError::AccessDenied)?;
            if r.0 != *sha
                || (r.2 != "PUBLIC"
                    && !(input.share_model && r.3 == account)
                    && !r
                        .4
                        .as_deref()
                        .is_some_and(|p| matches!(p, "manage" | "render_read")))
            {
                return Err(CloudError::AccessDenied);
            }
            if !matches!(
                r.1.to_lowercase().as_str(),
                "ysm" | "zip" | "bbmodel" | "gltf" | "glb"
            ) {
                return Err(CloudError::invalid_metadata("unsupported model format"));
            }
            if input.share_model
                && r.2 != "PUBLIC"
                && r.3 != account
                && r.4.as_deref() != Some("manage")
            {
                return Err(CloudError::AccessDenied);
            }
            (
                Some(asset.as_str()),
                Some(revision as i64),
                Some(sha.as_str()),
                Some(texture.as_str()),
            )
        } else {
            (None, None, None, None)
        };
        let target = previous
            .as_ref()
            .map(|p| p.0.clone())
            .unwrap_or_else(|| format!("entity_{epoch}_{}", id.simple()));
        if previous.is_none() {
            tx.execute("INSERT INTO targets(target_id,scope_id,target_kind,display_name,owner_account_id) VALUES (?1,?2,?3,?4,?5)",params![target,scope,input.entity_kind,input.display_name,account])?;
            tx.execute(
                "INSERT INTO target_acl(target_id,account_id,role) VALUES (?1,?2,'manage')",
                params![target, account],
            )?;
            tx.execute("INSERT INTO appearances(target_id) VALUES (?1)", [&target])?;
            tx.execute("INSERT INTO entity_bindings(binding_id,scope_id,world_epoch,entity_uuid,entity_kind,target_id) VALUES (?1,?2,?3,?4,?5,?6)",params![format!("binding_{epoch}_{}",id.simple()),scope,epoch,id.to_string(),input.entity_kind,target])?;
        }
        let changed=tx.execute("UPDATE appearances SET revision=revision+1,asset_id=?1,asset_revision=?2,raw_sha256=?3,texture_id=?4,disabled=0 WHERE target_id=?5 AND revision=?6",params![asset,asset_revision,sha,texture,target,input.expected_revision as i64])?;
        if changed != 1 {
            return Err(CloudError::RevisionConflict);
        }
        tx.execute(
            "DELETE FROM entity_model_shares WHERE target_id=?1",
            [&target],
        )?;
        if input.share_model && asset.is_some() {
            tx.execute("INSERT INTO entity_model_shares(target_id,shared_by_account_id,asset_id,asset_revision,raw_sha256) VALUES (?1,?2,?3,?4,?5)",params![target,account,asset,asset_revision,sha])?;
        }
        let payload = json!({"target_id":target,"revision":input.expected_revision+1,"asset_id":asset,"asset_revision":asset_revision,"raw_sha256":sha,"texture_id":texture,"disabled":false,"scale":null});
        tx.execute("INSERT INTO outbox_events(event_id,tenant_id,scope_id,target_id,kind,payload_json,created_at) VALUES (?1,?2,?3,?4,'APPEARANCE_UPDATED',?5,datetime('now'))",params![uuid::Uuid::new_v4().to_string(),account,scope,target,payload.to_string()])?;
        let (revision, selected) = selection(&tx, account, &target)?;
        let result = json!({"entity_uuid":id,"entity_kind":input.entity_kind,"target_id":target,"binding_revision":previous.map(|p|p.2).unwrap_or(0),"revision":revision,"selection":selected});
        tx.commit()?;
        Ok(result)
    }
    pub fn entity_asset(
        &self,
        account: &str,
        epoch: &str,
        id: uuid::Uuid,
        input: &AssetQuery,
    ) -> Result<(String, String), CloudError> {
        let scope = key(epoch)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let (target, _, _) = bindings(&conn, &scope, epoch, id)?.ok_or(CloudError::NotFound)?;
        let current: (Option<String>, Option<u64>, Option<String>) = conn.query_row(
            "SELECT asset_id,asset_revision,raw_sha256 FROM appearances WHERE target_id=?1",
            [&target],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, Option<i64>>(1)?.map(|v| v as u64),
                    r.get(2)?,
                ))
            },
        )?;
        if current.0.is_none()
            || current.1 != Some(input.asset_revision)
            || current.2.as_deref() != Some(&input.raw_sha256)
            || input
                .asset_id
                .as_ref()
                .is_some_and(|a| Some(a) != current.0.as_ref())
        {
            return Err(CloudError::RevisionConflict);
        }
        if selection(&conn, account, &target)?.1.is_null() {
            return Err(CloudError::AccessDenied);
        }
        let path:String=conn.query_row("SELECT object_path FROM asset_revisions WHERE asset_id=?1 AND revision=?2 AND raw_sha256=?3",params![current.0,input.asset_revision as i64,input.raw_sha256],|r|r.get(0))?;
        Ok((path, input.raw_sha256.clone()))
    }
}
