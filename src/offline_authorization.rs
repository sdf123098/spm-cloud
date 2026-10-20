//! Account approval and automatic device enrollment. Secrets never appear in responses.
use crate::{
    CloudStore,
    api::{AppState, authenticate},
    database::Connection,
    error::CloudError,
    offline_identity::{OfflineIdentitySettings, active_account, digest, require_enabled},
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use subtle::ConstantTimeEq;

pub(crate) fn migrate(conn: &Connection) -> Result<(), CloudError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS cloud_offline_authorizations (
        account_id TEXT PRIMARY KEY REFERENCES accounts(account_id), status TEXT NOT NULL,
        requested_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS cloud_offline_authorization_policy (id INTEGER PRIMARY KEY, mode TEXT NOT NULL);
        INSERT OR IGNORE INTO cloud_offline_authorization_policy(id,mode) VALUES(1,'automatic');",
    )?;
    Ok(())
}
pub(crate) fn status(conn: &Connection, account: &str) -> Result<String, CloudError> {
    Ok(conn
        .query_row(
            "SELECT status FROM cloud_offline_authorizations WHERE account_id=?1",
            [account],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_else(|| "NONE".into()))
}
pub(crate) fn check_status(status: &str) -> Result<(), CloudError> {
    match status {
        "APPROVED" => Ok(()),
        "REVOKED" => Err(CloudError::OfflineAuthorizationDenied),
        _ => Err(CloudError::OfflineAuthorizationPending),
    }
}
pub(crate) fn require_approved(conn: &Connection, account: &str) -> Result<(), CloudError> {
    check_status(&status(conn, account)?)
}

impl CloudStore {
    pub fn request_offline_authorization(
        &self,
        account: &str,
        _settings: &OfflineIdentitySettings,
    ) -> Result<String, CloudError> {
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        require_enabled(&tx)?;
        active_account(&tx, account)?;
        tx.execute(
            "UPDATE accounts SET created_at=created_at WHERE account_id=?1",
            [account],
        )?;
        let previous = status(&tx, account)?;
        let mode: String = tx.query_row(
            "SELECT mode FROM cloud_offline_authorization_policy WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let next = match (previous.as_str(), mode.as_str()) {
            ("NONE" | "PENDING", "automatic") => "APPROVED",
            ("NONE", _) => "PENDING",
            _ => previous.as_str(),
        };
        let now = crate::visual::now_ms();
        if previous == "NONE" {
            tx.execute("INSERT INTO cloud_offline_authorizations(account_id,status,requested_at,updated_at) VALUES(?1,?2,?3,?3)",params![account,next,now])?;
        } else if previous != next {
            tx.execute("UPDATE cloud_offline_authorizations SET status=?2,updated_at=?3 WHERE account_id=?1",params![account,next,now])?;
        }
        if previous != next {
            crate::store::write_audit(
                &tx,
                account,
                "OFFLINE_AUTHORIZATION_REQUESTED",
                None,
                None,
                Some(account),
                json!({"status":next}),
            )?;
        }
        let result = next.to_owned();
        tx.commit()?;
        Ok(result)
    }
    pub fn enroll_offline_device(
        &self,
        account: &str,
        input: &EnrollRequest,
        settings: &OfflineIdentitySettings,
    ) -> Result<Value, CloudError> {
        let parts: Vec<_> = input.offline_key.split('.').collect();
        if parts.len() != 3
            || parts[0] != "spm-offline-v1"
            || parts[1].len() != 36
            || !parts[1].starts_with("cok_")
            || !parts[1][4..]
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            || parts[2].len() != 43
            || !parts[2]
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            || input.label.trim().is_empty()
            || input.label.encode_utf16().count() > 128
            || input.label.chars().any(char::is_control)
        {
            return Err(CloudError::invalid_metadata("invalid offline enrollment"));
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        require_enabled(&tx)?;
        active_account(&tx, account)?;
        require_approved(&tx, account)?;
        tx.execute(
            "UPDATE accounts SET created_at=created_at WHERE account_id=?1",
            [account],
        )?;
        let found:Option<(String,String,String,Option<i64>)>=tx.query_row("SELECT i.identity_id,i.account_id,k.secret_hash,k.revoked_at FROM cloud_offline_keys k JOIN cloud_offline_identities i USING(identity_id) WHERE k.key_id=?1",[parts[1]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        if let Some((identity, owner, hash, revoked)) = found {
            if owner != account
                || !bool::from(
                    hash.as_bytes()
                        .ct_eq(digest("offline-key", &input.offline_key).as_bytes()),
                )
            {
                return Err(CloudError::OfflineKeyInvalid);
            }
            if revoked.is_some() {
                return Err(CloudError::OfflineKeyRevoked);
            }
            tx.commit()?;
            return Ok(
                json!({"account_id":account,"authorization_status":"APPROVED","identity_id":identity}),
            );
        }
        crate::offline_identity::rate(&tx, account, settings, true)?;
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM cloud_offline_identities WHERE account_id=?1",
            [account],
            |r| r.get(0),
        )?;
        if count >= i64::from(settings.max_identities_per_account) {
            return Err(CloudError::OfflineIdentityQuota);
        }
        let identity = format!("coff_{}", uuid::Uuid::new_v4().simple());
        let now = crate::visual::now_ms();
        tx.execute("INSERT INTO cloud_offline_identities(identity_id,account_id,label,created_at) VALUES(?1,?2,?3,?4)",params![identity,account,input.label.trim(),now])?;
        tx.execute("INSERT INTO cloud_offline_keys(key_id,identity_id,secret_hash,generation,created_at) VALUES(?1,?2,?3,1,?4)",params![parts[1],identity,digest("offline-key",&input.offline_key),now])?;
        crate::store::write_audit(
            &tx,
            account,
            "OFFLINE_DEVICE_ENROLLED",
            None,
            None,
            Some(&identity),
            json!({}),
        )?;
        tx.commit()?;
        Ok(json!({"account_id":account,"authorization_status":"APPROVED","identity_id":identity}))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollRequest {
    pub offline_key: String,
    pub label: String,
}
pub(crate) async fn enroll(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<EnrollRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    let input = crate::offline_identity::json_body(payload)?;
    let settings = state.runtime_config().offline_identity;
    state.offline_requests.admit(&account, &settings, false)?;
    let status = state
        .store
        .request_offline_authorization(&account, &settings)?;
    if status != "APPROVED" {
        return Ok(Json(
            json!({"account_id":account,"authorization_status":status}),
        ));
    }
    Ok(Json(
        state
            .store
            .enroll_offline_device(&account, &input, &settings)?,
    ))
}

pub(crate) async fn admin_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, CloudError> {
    crate::admin::authorize(&state, &headers, false)?;
    let conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let mut query=conn.prepare("SELECT account_id,status,requested_at,updated_at FROM cloud_offline_authorizations ORDER BY requested_at DESC,account_id LIMIT 1000")?;
    let rows=query.query_map([],|r|Ok(json!({"account_id":r.get::<_,String>(0)?,"status":r.get::<_,String>(1)?,"requested_at":r.get::<_,i64>(2)?,"updated_at":r.get::<_,i64>(3)?})))?.collect::<Result<Vec<_>,_>>()?;
    Ok(Json(json!({"entries":rows})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Decision {
    status: String,
}
pub(crate) async fn admin_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(account): Path<String>,
    payload: Result<Json<Decision>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Value>, CloudError> {
    let (actor, _) = crate::admin::authorize(&state, &headers, false)?;
    let input = crate::offline_identity::json_body(payload)?;
    if !matches!(input.status.as_str(), "APPROVED" | "REVOKED") {
        return Err(CloudError::invalid_metadata(
            "invalid authorization decision",
        ));
    }
    let mut conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let tx = conn.transaction()?;
    active_account(&tx, &account)?;
    tx.execute(
        "UPDATE accounts SET created_at=created_at WHERE account_id=?1",
        [&account],
    )?;
    if status(&tx, &account)? == "NONE" {
        tx.execute("INSERT INTO cloud_offline_authorizations(account_id,status,requested_at,updated_at) VALUES(?1,?2,?3,?3)",params![account,input.status,crate::visual::now_ms()])?;
    } else {
        tx.execute(
            "UPDATE cloud_offline_authorizations SET status=?2,updated_at=?3 WHERE account_id=?1",
            params![account, input.status, crate::visual::now_ms()],
        )?;
    }
    if input.status == "REVOKED" {
        tx.execute("UPDATE cloud_offline_sessions SET revoked_at=?2 WHERE identity_id IN (SELECT identity_id FROM cloud_offline_identities WHERE account_id=?1) AND revoked_at IS NULL",params![account,crate::visual::now_ms()])?;
        tx.execute("DELETE FROM cloud_offline_appearances WHERE identity_id IN (SELECT identity_id FROM cloud_offline_identities WHERE account_id=?1)",[&account])?;
        tx.execute("UPDATE cloud_offline_identities SET active_session_id=NULL,binding_revision=binding_revision+1,appearance_revision=appearance_revision+1 WHERE account_id=?1 AND active_session_id IS NOT NULL",[&account])?;
    }
    crate::store::write_audit(
        &tx,
        &actor,
        "OFFLINE_AUTHORIZATION_DECIDED",
        None,
        None,
        Some(&account),
        json!({"status":input.status}),
    )?;
    tx.commit()?;
    Ok(Json(json!({"account_id":account,"status":input.status})))
}
