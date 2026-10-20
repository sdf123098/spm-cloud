//! Experimental Cloud-owned offline identities, separate from verified game identities.
use crate::{
    CloudStore,
    api::{AppState, authenticate},
};
use crate::{database::Connection, error::CloudError};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use base64::Engine;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type RequestBuckets = std::collections::BTreeMap<(String, bool), std::collections::VecDeque<i64>>;
#[derive(Default)]
pub(crate) struct OfflineRequestLimiter {
    attempts: std::sync::Mutex<RequestBuckets>,
}
impl OfflineRequestLimiter {
    pub(crate) fn admit(
        &self,
        account: &str,
        settings: &OfflineIdentitySettings,
        issuance: bool,
    ) -> Result<(), CloudError> {
        let now = crate::visual::now_ms();
        let mut attempts = self
            .attempts
            .lock()
            .map_err(|_| CloudError::configuration("offline request limiter lock poisoned"))?;
        attempts.retain(|(_, issue), times| {
            let window = if *issue { 3_600_000 } else { 60_000 };
            times.retain(|time| now - *time < window);
            !times.is_empty()
        });
        let key = (account.to_owned(), issuance);
        if attempts.len() >= 4096 && !attempts.contains_key(&key) {
            return Err(CloudError::RateLimited);
        }
        let times = attempts.entry(key).or_default();
        let limit = if issuance {
            settings.issuance_requests_per_hour
        } else {
            settings.activation_requests_per_minute
        };
        if times.len() >= limit as usize {
            return Err(CloudError::RateLimited);
        }
        times.push_back(now);
        Ok(())
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/account/api/offline-identities", get(web_list).post(issue))
        .route(
            "/account/api/offline-identities/{identity_id}/rotate",
            post(rotate),
        )
        .route(
            "/account/api/offline-identities/{identity_id}/key",
            delete(revoke),
        )
        .route("/v1/offline-identities/me", get(client_list))
        .route("/v1/offline-identities/activate", post(activate))
        .route(
            "/v1/offline-identities/request",
            post(crate::offline_authorization::enroll),
        )
        .route_layer(axum::middleware::from_fn(no_store))
        .with_state(state)
}

pub(crate) async fn no_store(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

pub(crate) fn json_body<T>(
    payload: Result<Json<T>, axum::extract::rejection::JsonRejection>,
) -> Result<T, CloudError> {
    payload.map(|Json(body)| body).map_err(|error| {
        if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
            CloudError::MessageTooLarge
        } else {
            CloudError::invalid_metadata("invalid offline identity JSON")
        }
    })
}

pub(crate) fn digest(purpose: &str, secret: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(purpose.as_bytes());
    hash.update([0]);
    hash.update(secret.as_bytes());
    hex::encode(hash.finalize())
}
fn random_secret() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}
fn id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

pub(crate) fn active_account(conn: &Connection, account: &str) -> Result<(), CloudError> {
    let active: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM accounts a LEFT JOIN admin_accounts m USING(account_id) WHERE a.account_id=?1 AND COALESCE(m.disabled,0)=0)", [account], |r| r.get(0))?;
    if active {
        Ok(())
    } else {
        Err(CloudError::Unauthenticated)
    }
}
fn web_account(state: &AppState, headers: &HeaderMap) -> Result<(String, String), CloudError> {
    let (web, _) = crate::account_links::profile_account(state, headers)?;
    let account = state.store.linked_cloud_account(&web)?;
    state.store.require_active_account(&account)?;
    Ok((web, account))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssueRequest {
    pub label: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotateRequest {
    pub expected_key_generation: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivateRequest {
    pub offline_key: String,
    pub entity_uuid: uuid::Uuid,
    pub profile_name: String,
    pub expected_binding_revision: u64,
}

async fn web_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, CloudError> {
    let (_, account) = web_account(&state, &headers)?;
    Ok(Json(state.store.offline_identities(&account)?))
}
async fn client_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.offline_identities(&account)?))
}
async fn issue(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<IssueRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, CloudError> {
    let (actor, account) = web_account(&state, &headers)?;
    let input = json_body(payload)?;
    state
        .offline_requests
        .admit(&account, &state.runtime_config().offline_identity, true)?;
    Ok((
        StatusCode::CREATED,
        Json(state.store.issue_offline_key(
            &actor,
            &account,
            &input.label,
            &state.runtime_config().offline_identity,
        )?),
    )
        .into_response())
}
async fn rotate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(identity): Path<String>,
    payload: Result<Json<RotateRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Value>, CloudError> {
    let (actor, account) = web_account(&state, &headers)?;
    let input = json_body(payload)?;
    state
        .offline_requests
        .admit(&account, &state.runtime_config().offline_identity, true)?;
    Ok(Json(state.store.rotate_offline_key(
        &actor,
        &account,
        &identity,
        input.expected_key_generation,
        &state.runtime_config().offline_identity,
    )?))
}
async fn revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(identity): Path<String>,
) -> Result<Json<Value>, CloudError> {
    let (actor, account) = web_account(&state, &headers)?;
    state
        .store
        .revoke_offline_key(&actor, &account, &identity)?;
    Ok(Json(json!({"ok":true})))
}
async fn activate(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ActivateRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    let input = json_body(payload)?;
    state
        .offline_requests
        .admit(&account, &state.runtime_config().offline_identity, false)?;
    Ok(Json(state.store.activate_offline(
        &account,
        &input,
        &state.runtime_config().offline_identity,
    )?))
}

pub(crate) fn rate(
    conn: &Connection,
    account: &str,
    settings: &OfflineIdentitySettings,
    issuance: bool,
) -> Result<(), CloudError> {
    // Serialize account-wide quota decisions across separate MySQL connections too.
    conn.execute(
        "UPDATE accounts SET created_at=created_at WHERE account_id=?1",
        [account],
    )?;
    let now = crate::visual::now_ms();
    let (sql, since, limit) = if issuance {
        (
            "SELECT COUNT(*) FROM cloud_offline_keys k JOIN cloud_offline_identities i USING(identity_id) WHERE i.account_id=?1 AND k.created_at>?2",
            now - 3_600_000,
            settings.issuance_requests_per_hour,
        )
    } else {
        (
            "SELECT COUNT(*) FROM cloud_offline_sessions s JOIN cloud_offline_identities i USING(identity_id) WHERE i.account_id=?1 AND s.created_at>?2",
            now - 60_000,
            settings.activation_requests_per_minute,
        )
    };
    let count: i64 = conn.query_row(sql, params![account, since], |r| r.get(0))?;
    if count >= i64::from(limit) {
        Err(CloudError::RateLimited)
    } else {
        Ok(())
    }
}

pub(crate) fn revoke_identity(conn: &Connection, identity: &str) -> Result<(), CloudError> {
    let now = crate::visual::now_ms();
    conn.execute(
        "UPDATE cloud_offline_keys SET revoked_at=?2 WHERE identity_id=?1 AND revoked_at IS NULL",
        params![identity, now],
    )?;
    conn.execute("UPDATE cloud_offline_sessions SET revoked_at=?2 WHERE identity_id=?1 AND revoked_at IS NULL", params![identity,now])?;
    conn.execute(
        "DELETE FROM cloud_offline_appearances WHERE identity_id=?1",
        [identity],
    )?;
    conn.execute("UPDATE cloud_offline_identities SET active_session_id=NULL,binding_revision=binding_revision+1,appearance_revision=appearance_revision+1 WHERE identity_id=?1 AND active_session_id IS NOT NULL", [identity])?;
    Ok(())
}
fn new_key(conn: &Connection, identity: &str, generation: i64) -> Result<Value, CloudError> {
    let key_id = id("cok");
    let key = format!("spm-offline-v1.{key_id}.{}", random_secret());
    conn.execute("INSERT INTO cloud_offline_keys(key_id,identity_id,secret_hash,generation,created_at) VALUES(?1,?2,?3,?4,?5)", params![key_id,identity,digest("offline-key",&key),generation,crate::visual::now_ms()])?;
    Ok(
        json!({"identity_id":identity,"key_id":key_id,"key_generation":generation,"offline_key":key}),
    )
}

impl CloudStore {
    pub fn offline_identities(&self, account: &str) -> Result<Value, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        active_account(&conn, account)?;
        let authorization = crate::offline_authorization::status(&conn, account)?;
        let enabled: bool = conn.query_row(
            "SELECT enabled FROM cloud_offline_runtime WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let mut statement = conn.prepare("SELECT i.identity_id,i.label,i.key_generation,i.binding_revision,i.appearance_revision,k.key_id,k.revoked_at,s.entity_uuid,s.profile_name,s.expires_at,s.revoked_at FROM cloud_offline_identities i LEFT JOIN cloud_offline_keys k ON k.identity_id=i.identity_id AND k.generation=i.key_generation LEFT JOIN cloud_offline_sessions s ON s.session_id=i.active_session_id WHERE i.account_id=?1 ORDER BY i.created_at,i.identity_id")?;
        let identities = statement.query_map([account], |r| {
            let active = r.get::<_,Option<i64>>(9)?.is_some_and(|v| v > crate::visual::now_ms()) && r.get::<_,Option<i64>>(10)?.is_none();
            Ok(json!({"identity_id":r.get::<_,String>(0)?,"label":r.get::<_,String>(1)?,"key_generation":r.get::<_,i64>(2)?,"binding_revision":r.get::<_,i64>(3)?,"appearance_revision":r.get::<_,i64>(4)?,"key_id":r.get::<_,Option<String>>(5)?,"key_status":if r.get::<_,Option<i64>>(6)?.is_some(){"REVOKED"}else{"ACTIVE"},"entity_uuid":if active{r.get::<_,Option<String>>(7)?}else{None},"profile_name":if active{r.get::<_,Option<String>>(8)?}else{None},"expires_at_unix_ms":if active{r.get::<_,Option<i64>>(9)?}else{None}}))
        })?.collect::<Result<Vec<_>,_>>()?;
        Ok(
            json!({"account_id":account,"enabled":enabled,"authorization_status":authorization,"identities":identities}),
        )
    }

    pub fn issue_offline_key(
        &self,
        actor: &str,
        account: &str,
        label: &str,
        settings: &OfflineIdentitySettings,
    ) -> Result<Value, CloudError> {
        let grant = self.request_offline_authorization(account, settings)?;
        crate::offline_authorization::check_status(&grant)?;
        if label.trim().is_empty()
            || label.encode_utf16().count() > 128
            || label.chars().any(char::is_control)
        {
            return Err(CloudError::invalid_metadata(
                "invalid offline identity label",
            ));
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        require_enabled(&tx)?;
        active_account(&tx, account)?;
        crate::offline_authorization::require_approved(&tx, account)?;
        rate(&tx, account, settings, true)?;
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM cloud_offline_identities WHERE account_id=?1",
            [account],
            |r| r.get(0),
        )?;
        if count >= i64::from(settings.max_identities_per_account) {
            return Err(CloudError::OfflineIdentityQuota);
        }
        crate::offline_authorization::require_approved(&tx, account)?;
        let identity = id("coff");
        tx.execute("INSERT INTO cloud_offline_identities(identity_id,account_id,label,created_at) VALUES(?1,?2,?3,?4)", params![identity,account,label.trim(),crate::visual::now_ms()])?;
        let response = new_key(&tx, &identity, 1)?;
        crate::store::write_audit(
            &tx,
            actor,
            "OFFLINE_KEY_ISSUED",
            None,
            None,
            Some(&identity),
            json!({}),
        )?;
        tx.commit()?;
        Ok(response)
    }

    pub fn rotate_offline_key(
        &self,
        actor: &str,
        account: &str,
        identity: &str,
        expected: u64,
        settings: &OfflineIdentitySettings,
    ) -> Result<Value, CloudError> {
        if expected >= i64::MAX as u64 {
            return Err(CloudError::invalid_metadata("invalid key generation"));
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        require_enabled(&tx)?;
        active_account(&tx, account)?;
        crate::offline_authorization::require_approved(&tx, account)?;
        rate(&tx, account, settings, true)?;
        let changed = tx.execute("UPDATE cloud_offline_identities SET key_generation=key_generation+1 WHERE identity_id=?1 AND account_id=?2 AND key_generation=?3",params![identity,account,expected as i64])?;
        if changed != 1 {
            return Err(CloudError::OfflineKeyInvalid);
        }
        revoke_identity(&tx, identity)?;
        let response = new_key(&tx, identity, expected as i64 + 1)?;
        crate::store::write_audit(
            &tx,
            actor,
            "OFFLINE_KEY_ROTATED",
            None,
            None,
            Some(identity),
            json!({}),
        )?;
        tx.commit()?;
        Ok(response)
    }

    pub fn revoke_offline_key(
        &self,
        actor: &str,
        account: &str,
        identity: &str,
    ) -> Result<(), CloudError> {
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        active_account(&tx, account)?;
        let owned:bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM cloud_offline_identities WHERE identity_id=?1 AND account_id=?2)",params![identity,account],|r|r.get(0))?;
        if !owned {
            return Err(CloudError::OfflineKeyInvalid);
        }
        revoke_identity(&tx, identity)?;
        crate::store::write_audit(
            &tx,
            actor,
            "OFFLINE_KEY_REVOKED",
            None,
            None,
            Some(identity),
            json!({}),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn activate_offline(
        &self,
        account: &str,
        input: &ActivateRequest,
        settings: &OfflineIdentitySettings,
    ) -> Result<Value, CloudError> {
        if input.offline_key.len() > 256
            || input.profile_name.is_empty()
            || input.profile_name.encode_utf16().count() > 64
            || input.profile_name.chars().any(char::is_control)
            || input.expected_binding_revision >= i64::MAX as u64
        {
            return Err(CloudError::OfflineKeyInvalid);
        }
        let parts: Vec<_> = input.offline_key.split('.').collect();
        if parts.len() != 3 || parts[0] != "spm-offline-v1" || parts[2].len() != 43 {
            return Err(CloudError::OfflineKeyInvalid);
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        require_enabled(&tx)?;
        active_account(&tx, account)?;
        rate(&tx, account, settings, false)?;
        let found: Option<(String,String)> = tx.query_row("SELECT i.identity_id,k.secret_hash FROM cloud_offline_keys k JOIN cloud_offline_identities i USING(identity_id) WHERE k.key_id=?1 AND i.account_id=?2 AND k.generation=i.key_generation AND k.revoked_at IS NULL",params![parts[1],account],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let (identity, expected_hash) = found.ok_or(CloudError::OfflineKeyInvalid)?;
        if !bool::from(
            expected_hash
                .as_bytes()
                .ct_eq(digest("offline-key", &input.offline_key).as_bytes()),
        ) {
            return Err(CloudError::OfflineKeyInvalid);
        }
        crate::offline_authorization::require_approved(&tx, account)?;
        let session = id("cos");
        let token = random_secret();
        let now = crate::visual::now_ms();
        let changed = tx.execute("UPDATE cloud_offline_identities SET binding_revision=binding_revision+1,appearance_revision=appearance_revision+1,active_session_id=?4 WHERE identity_id=?1 AND account_id=?2 AND binding_revision=?3",params![identity,account,input.expected_binding_revision as i64,session])?;
        if changed != 1 {
            return Err(CloudError::OfflineBindingStale);
        }
        tx.execute("UPDATE cloud_offline_sessions SET revoked_at=?2 WHERE identity_id=?1 AND revoked_at IS NULL",params![identity,now])?;
        tx.execute(
            "DELETE FROM cloud_offline_appearances WHERE identity_id=?1",
            [&identity],
        )?;
        let binding = input.expected_binding_revision as i64 + 1;
        let expires = now + i64::from(settings.session_ttl_seconds) * 1000;
        tx.execute("INSERT INTO cloud_offline_sessions(session_id,identity_id,key_id,binding_revision,session_token_hash,entity_uuid,profile_name,expires_at,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![session,identity,parts[1],binding,digest("offline-session",&token),input.entity_uuid.to_string(),input.profile_name,expires,now])?;
        let revision: i64 = tx.query_row(
            "SELECT appearance_revision FROM cloud_offline_identities WHERE identity_id=?1",
            [&identity],
            |r| r.get(0),
        )?;
        crate::store::write_audit(
            &tx,
            account,
            "OFFLINE_SESSION_ACTIVATED",
            None,
            None,
            Some(&identity),
            json!({"session_id":session,"binding_revision":binding}),
        )?;
        tx.commit()?;
        Ok(
            json!({"identity_id":identity,"session_id":session,"session_token":token,"binding_revision":binding,"appearance_revision":revision,"entity_uuid":input.entity_uuid,"expires_at_unix_ms":expires}),
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct OfflineIdentitySettings {
    pub authorization_mode: OfflineAuthorizationMode,
    pub max_identities_per_account: u32,
    pub session_ttl_seconds: u32,
    pub activation_requests_per_minute: u32,
    pub issuance_requests_per_hour: u32,
}

impl Default for OfflineIdentitySettings {
    fn default() -> Self {
        Self {
            authorization_mode: OfflineAuthorizationMode::Automatic,
            max_identities_per_account: 3,
            session_ttl_seconds: 60,
            activation_requests_per_minute: 10,
            issuance_requests_per_hour: 5,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfflineAuthorizationMode {
    Disabled,
    Manual,
    #[default]
    Automatic,
}
impl OfflineIdentitySettings {
    pub fn is_enabled(&self) -> bool {
        self.authorization_mode != OfflineAuthorizationMode::Disabled
    }

    pub fn validate(&self) -> Result<(), CloudError> {
        for (field, value, max) in [
            (
                "max_identities_per_account",
                self.max_identities_per_account,
                100,
            ),
            ("session_ttl_seconds", self.session_ttl_seconds, 300),
            (
                "activation_requests_per_minute",
                self.activation_requests_per_minute,
                1000,
            ),
            (
                "issuance_requests_per_hour",
                self.issuance_requests_per_hour,
                1000,
            ),
        ] {
            if value == 0 || value > max {
                return Err(CloudError::configuration(format!(
                    "offline_identity.{field}: must be in 1..={max}"
                )));
            }
        }
        Ok(())
    }
}

pub(crate) fn migrate(conn: &Connection) -> Result<(), CloudError> {
    crate::offline_authorization::migrate(conn)?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS cloud_offline_identities (
        identity_id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(account_id),
        label TEXT NOT NULL, key_generation INTEGER NOT NULL DEFAULT 1,
        binding_revision INTEGER NOT NULL DEFAULT 0, appearance_revision INTEGER NOT NULL DEFAULT 0,
        active_session_id TEXT, created_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS cloud_offline_identity_account ON cloud_offline_identities(account_id);
        CREATE TABLE IF NOT EXISTS cloud_offline_keys (
        key_id TEXT PRIMARY KEY, identity_id TEXT NOT NULL REFERENCES cloud_offline_identities(identity_id),
        secret_hash TEXT NOT NULL, generation INTEGER NOT NULL, created_at INTEGER NOT NULL, revoked_at INTEGER,
        UNIQUE(identity_id,generation));
        CREATE TABLE IF NOT EXISTS cloud_offline_sessions (
        session_id TEXT PRIMARY KEY, identity_id TEXT NOT NULL REFERENCES cloud_offline_identities(identity_id),
        key_id TEXT NOT NULL REFERENCES cloud_offline_keys(key_id), binding_revision INTEGER NOT NULL,
        session_token_hash TEXT NOT NULL, entity_uuid TEXT NOT NULL, profile_name TEXT NOT NULL,
        expires_at INTEGER NOT NULL, revoked_at INTEGER, created_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS cloud_offline_session_entity ON cloud_offline_sessions(entity_uuid,expires_at);
        CREATE INDEX IF NOT EXISTS cloud_offline_session_identity ON cloud_offline_sessions(identity_id);
        CREATE TABLE IF NOT EXISTS cloud_offline_appearances (
        identity_id TEXT PRIMARY KEY REFERENCES cloud_offline_identities(identity_id),
        session_id TEXT NOT NULL REFERENCES cloud_offline_sessions(session_id), binding_revision INTEGER NOT NULL,
        revision INTEGER NOT NULL, asset_id TEXT, asset_revision INTEGER, raw_sha256 TEXT, texture_id TEXT,
        motion_json TEXT, display_json TEXT, updated_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS cloud_offline_appearance_session ON cloud_offline_appearances(session_id);
        CREATE TABLE IF NOT EXISTS cloud_offline_runtime (id INTEGER PRIMARY KEY, enabled INTEGER NOT NULL);
        INSERT OR IGNORE INTO cloud_offline_runtime(id,enabled) VALUES(1,0);")?;
    Ok(())
}

pub(crate) fn configure(
    conn: &Connection,
    settings: &OfflineIdentitySettings,
) -> Result<(), CloudError> {
    let enabled = settings.is_enabled();
    let mode = serde_json::to_value(settings.authorization_mode)
        .expect("enum serialization")
        .as_str()
        .unwrap()
        .to_owned();
    conn.execute(
        "UPDATE cloud_offline_authorization_policy SET mode=?1 WHERE id=1",
        [mode],
    )?;
    if !enabled {
        invalidate_sessions(conn)?;
    }
    conn.execute(
        "UPDATE cloud_offline_runtime SET enabled=?1 WHERE id=1",
        [i64::from(enabled)],
    )?;
    Ok(())
}

pub(crate) fn require_enabled(conn: &Connection) -> Result<(), CloudError> {
    let enabled: bool = conn.query_row(
        "SELECT enabled FROM cloud_offline_runtime WHERE id=1",
        [],
        |r| r.get(0),
    )?;
    if enabled {
        Ok(())
    } else {
        Err(CloudError::OfflineIdentityDisabled)
    }
}

/// Called inside the caller's transaction so disabled/restarted/restored sessions cannot revive.
pub(crate) fn invalidate_sessions(conn: &Connection) -> Result<(), CloudError> {
    conn.execute(
        "UPDATE cloud_offline_sessions SET revoked_at=?1 WHERE revoked_at IS NULL",
        [crate::visual::now_ms()],
    )?;
    conn.execute("UPDATE cloud_offline_identities SET active_session_id=NULL,binding_revision=binding_revision+1,appearance_revision=appearance_revision+1 WHERE active_session_id IS NOT NULL", [])?;
    conn.execute("DELETE FROM cloud_offline_appearances", [])?;
    Ok(())
}
