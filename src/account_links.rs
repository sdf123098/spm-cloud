//! Self-service account connections; credentials prove ownership, never confer a role.
use crate::database::Connection;
use crate::{
    CloudStore,
    api::{AppState, authenticate},
    error::CloudError,
    models::LoginRequest,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) fn migrate(conn: &Connection) -> Result<(), CloudError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS web_cloud_links (
        web_account_id TEXT PRIMARY KEY REFERENCES accounts(account_id),
        cloud_account_id TEXT NOT NULL UNIQUE REFERENCES accounts(account_id),
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        CHECK(web_account_id<>cloud_account_id));",
    )?;
    Ok(())
}

impl CloudStore {
    pub(crate) fn require_active_account(&self, account: &str) -> Result<(), CloudError> {
        crate::config::validate_slug(account, "owner_account_id")?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM accounts a LEFT JOIN admin_accounts m USING(account_id) WHERE a.account_id=?1 AND COALESCE(m.disabled,0)=0)",[account],|r|r.get(0))?;
        if exists {
            Ok(())
        } else {
            Err(CloudError::NotFound)
        }
    }
    pub fn linked_cloud_account(&self, web: &str) -> Result<String, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        Ok(conn
            .query_row(
                "SELECT cloud_account_id FROM web_cloud_links WHERE web_account_id=?1",
                [web],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_else(|| web.into()))
    }
    pub(crate) fn linked_web_account(&self, cloud: &str) -> Result<String, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        Ok(conn
            .query_row(
                "SELECT web_account_id FROM web_cloud_links WHERE cloud_account_id=?1",
                [cloud],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_else(|| cloud.into()))
    }
    pub fn connect_owned_accounts(&self, web: &str, cloud: &str) -> Result<(), CloudError> {
        if web == cloud {
            return Ok(());
        }
        crate::config::validate_slug(cloud, "cloud_account_id")?;
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        if !tx.query_row("SELECT EXISTS(SELECT 1 FROM accounts a LEFT JOIN admin_accounts m USING(account_id) WHERE a.account_id=?1 AND COALESCE(m.disabled,0)=0)",[cloud],|r|r.get::<_,bool>(0))? { return Err(CloudError::NotFound); }
        // Connections are one-to-one, not chains or implicit account merges.
        let conflict: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM web_cloud_links WHERE (web_account_id=?1 AND cloud_account_id<>?2) OR (cloud_account_id=?2 AND web_account_id<>?1) OR web_account_id=?2 OR cloud_account_id=?1)",params![web,cloud],|r|r.get(0))?;
        if conflict {
            return Err(CloudError::IdentityAlreadyLinked);
        }
        // Preserve identity IDs and their player state while unifying verified global identities.
        let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM identities w JOIN identities c ON w.identity_kind=c.identity_kind AND COALESCE(w.provider_id,'')=COALESCE(c.provider_id,'') AND w.profile_uuid=c.profile_uuid WHERE w.account_id=?1 AND c.account_id=?2 AND w.verified=1 AND c.verified=1 AND w.identity_kind IN ('official','yggdrasil'))",params![web,cloud],|r|r.get(0))?;
        if duplicate {
            return Err(CloudError::IdentityAlreadyLinked);
        }
        let moved=tx.execute("UPDATE identities SET account_id=?1 WHERE account_id=?2 AND verified=1 AND identity_kind IN ('official','yggdrasil')",params![cloud,web])?;
        tx.execute(
            "INSERT OR IGNORE INTO web_cloud_links(web_account_id,cloud_account_id) VALUES(?1,?2)",
            params![web, cloud],
        )?;
        super::store::write_audit(
            &tx,
            web,
            "SELF_CLOUD_ACCOUNT_LINKED",
            None,
            None,
            Some(cloud),
            json!({"identities_unified":moved}),
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn assign_asset_owner(
        &self,
        actor: &str,
        asset: &str,
        owner: &str,
        expected: &str,
    ) -> Result<(), CloudError> {
        crate::config::validate_slug(owner, "owner_account_id")?;
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        if !tx.query_row("SELECT EXISTS(SELECT 1 FROM accounts a LEFT JOIN admin_accounts m USING(account_id) WHERE a.account_id=?1 AND COALESCE(m.disabled,0)=0)",[owner],|r|r.get::<_,bool>(0))? { return Err(CloudError::NotFound); }
        let previous: String = tx
            .query_row(
                "SELECT owner_account_id FROM assets WHERE asset_id=?1",
                [asset],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(CloudError::NotFound)?;
        if previous != expected {
            return Err(CloudError::RevisionConflict);
        }
        if previous == owner {
            return Ok(());
        }
        tx.execute(
            "UPDATE assets SET owner_account_id=?1 WHERE asset_id=?2",
            params![owner, asset],
        )?;
        // Transferring private resources must not leave the previous owner's grants behind.
        tx.execute("DELETE FROM asset_acl WHERE asset_id=?1", [asset])?;
        tx.execute(
            "INSERT INTO asset_acl(asset_id,account_id,permission) VALUES(?1,?2,'manage')",
            params![asset, owner],
        )?;
        super::store::write_audit(
            &tx,
            actor,
            "MODEL_OWNER_CHANGED",
            None,
            None,
            Some(asset),
            json!({"previous_owner":previous,"owner_account_id":owner}),
        )?;
        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn profile_account(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(String, String), CloudError> {
    let account = authenticate(state, headers)?;
    let role = state
        .store
        .admin_role(&account, &state.config.bootstrap_account_id)?;
    Ok((account, role))
}
pub async fn profile(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, CloudError> {
    let (web, role) = profile_account(&state, &headers)?;
    let cloud = state.store.linked_cloud_account(&web)?;
    state
        .store
        .admin_role(&cloud, &state.config.bootstrap_account_id)?;
    let identities = state.store.list_identities(&cloud)?;
    Ok(Json(
        json!({"account_id":web,"cloud_account_id":cloud,"role":role,"identities":identities,"linked":web!=cloud,"instance_id":state.config.instance_id}),
    ))
}
pub async fn login(
    State(state): State<AppState>,
    Json(input): Json<LoginRequest>,
) -> Result<Json<Value>, CloudError> {
    crate::admin::login_attempt(&state)?;
    let store = state.store.clone();
    let account = input.account_id.clone();
    let session = tokio::task::spawn_blocking(move || store.issue_session(&input))
        .await
        .map_err(|_| CloudError::Internal(anyhow::anyhow!("login worker failed")))??;
    let role = state
        .store
        .admin_role(&account, &state.config.bootstrap_account_id)?;
    let mut response = serde_json::to_value(session).unwrap();
    response["account_id"] = json!(account);
    response["role"] = json!(role);
    Ok(Json(response))
}
pub async fn connect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<LoginRequest>,
) -> Result<Json<Value>, CloudError> {
    crate::admin::login_attempt(&state)?;
    let (web, _) = profile_account(&state, &headers)?;
    let target = input.account_id.clone();
    let store = state.store.clone();
    let proof = tokio::task::spawn_blocking(move || store.issue_session(&input))
        .await
        .map_err(|_| CloudError::Internal(anyhow::anyhow!("login worker failed")))??;
    // Do not leave a reusable target-account session after a password proof.
    state.store.revoke_access_token(&proof.access_token)?;
    state.store.connect_owned_accounts(&web, &target)?;
    profile(State(state), headers).await
}
#[derive(Deserialize)]
pub struct OwnerQuery {
    #[serde(default)]
    q: String,
}
pub async fn owners(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OwnerQuery>,
) -> Result<Json<Value>, CloudError> {
    crate::admin::authorize(&state, &headers, false)?;
    let conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let mut stmt=conn.prepare("SELECT a.account_id FROM accounts a LEFT JOIN admin_accounts m USING(account_id) WHERE COALESCE(m.disabled,0)=0 AND a.account_id LIKE ?1 ORDER BY a.account_id LIMIT 40")?;
    let entries = stmt
        .query_map(
            [format!(
                "%{}%",
                query.q.chars().take(128).collect::<String>()
            )],
            |r| r.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({"entries":entries})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerUpdate {
    owner_account_id: String,
    expected_owner_account_id: String,
}
pub async fn assign(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
    Json(input): Json<OwnerUpdate>,
) -> Result<StatusCode, CloudError> {
    let (actor, _) = crate::admin::authorize(&state, &headers, false)?;
    state.store.assign_asset_owner(
        &actor,
        &asset,
        &input.owner_account_id,
        &input.expected_owner_account_id,
    )?;
    Ok(StatusCode::NO_CONTENT)
}
