//! Publication leases and opt-in discovery for experimental offline identities.
use crate::{
    CloudStore,
    api::{AppState, authenticate},
    database::Connection,
    error::CloudError,
    offline_identity::{active_account, digest, require_enabled},
    player::{PlayerAppearance, PlayerSelection},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::put,
};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use subtle::ConstantTimeEq;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/v1/offline-player-sessions/{session_id}/appearance",
            put(publish),
        )
        .route(
            "/v1/offline-player-sessions/{session_id}",
            axum::routing::delete(withdraw),
        )
        .route_layer(axum::middleware::from_fn(crate::offline_identity::no_store))
        .with_state(state)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineAppearanceUpdate {
    pub binding_revision: u64,
    pub expected_revision: u64,
    pub asset_id: Option<String>,
    pub asset_revision: Option<u64>,
    pub raw_sha256: Option<String>,
    pub texture_id: Option<String>,
    pub motion: Option<Value>,
    pub display_state: Option<crate::display::PlayerDisplayUpdate>,
}

fn token(headers: &HeaderMap) -> Result<&str, CloudError> {
    headers
        .get("x-spm-offline-session")
        .and_then(|v| v.to_str().ok())
        .filter(|s| s.len() == 43)
        .ok_or(CloudError::OfflineSessionExpired)
}

async fn publish(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session): Path<String>,
    payload: Result<Json<OfflineAppearanceUpdate>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    let body = crate::offline_identity::json_body(payload)?;
    let revision = state.store.publish_offline(
        &account,
        &session,
        token(&headers)?,
        &body,
        state.runtime_config().offline_identity.session_ttl_seconds,
    )?;
    Ok(Json(json!({"revision":revision})))
}
async fn withdraw(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session): Path<String>,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    state
        .store
        .withdraw_offline(&account, &session, token(&headers)?)?;
    Ok(Json(json!({"ok":true})))
}

type SessionAuthorizationRow = (
    String,
    String,
    i64,
    Option<String>,
    i64,
    Option<i64>,
    Option<i64>,
    i64,
    i64,
);

fn authorize(
    conn: &Connection,
    account: &str,
    session: &str,
    token: &str,
) -> Result<(String, i64), CloudError> {
    require_enabled(conn)?;
    active_account(conn, account)?;
    crate::offline_authorization::require_approved(conn, account)?;
    let row:Option<SessionAuthorizationRow> = conn.query_row(
        "SELECT i.identity_id,s.session_token_hash,s.binding_revision,i.active_session_id,s.expires_at,s.revoked_at,k.revoked_at,k.generation,i.key_generation FROM cloud_offline_sessions s JOIN cloud_offline_identities i USING(identity_id) JOIN cloud_offline_keys k ON k.key_id=s.key_id WHERE s.session_id=?1 AND i.account_id=?2",
        params![session,account],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).optional()?;
    let (
        identity,
        hash,
        binding,
        active,
        expires,
        revoked,
        key_revoked,
        generation,
        current_generation,
    ) = row.ok_or(CloudError::OfflineSessionExpired)?;
    if !bool::from(
        hash.as_bytes()
            .ct_eq(digest("offline-session", token).as_bytes()),
    ) {
        return Err(CloudError::OfflineSessionExpired);
    }
    if active.as_deref() != Some(session) {
        return Err(CloudError::OfflineBindingStale);
    }
    if expires <= crate::visual::now_ms()
        || revoked.is_some()
        || key_revoked.is_some()
        || generation != current_generation
    {
        return Err(CloudError::OfflineSessionExpired);
    }
    Ok((identity, binding))
}

impl CloudStore {
    pub fn publish_offline(
        &self,
        account: &str,
        session: &str,
        token: &str,
        input: &OfflineAppearanceUpdate,
        ttl: u32,
    ) -> Result<u64, CloudError> {
        if input.expected_revision >= i64::MAX as u64 || input.binding_revision >= i64::MAX as u64 {
            return Err(CloudError::invalid_metadata("invalid revision"));
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        let (identity, binding) = authorize(&tx, account, session, token)?;
        if binding != input.binding_revision as i64 {
            return Err(CloudError::OfflineBindingStale);
        }
        crate::player::validate_asset(
            &tx,
            account,
            input.asset_id.as_deref(),
            input.asset_revision,
            input.raw_sha256.as_deref(),
            input.texture_id.as_deref(),
        )?;
        if let Some(motion) = &input.motion {
            crate::player::validate_motion(motion)?;
        }
        let settings = self.visual_settings();
        crate::visual::rate(&tx, account, &settings, crate::visual::now_ms())?;
        let display = if let Some(display) = &input.display_state {
            if settings.features.player_display_state {
                crate::display::validate(display, &settings)?;
                if crate::display::context(
                    &tx,
                    account,
                    &display.scope_id,
                    &display.world_epoch,
                    &display.dimension_id,
                )
                .is_ok()
                {
                    Some(
                        serde_json::to_string(display)
                            .map_err(|_| CloudError::invalid_metadata("invalid display"))?,
                    )
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        let changed=tx.execute("UPDATE cloud_offline_identities SET appearance_revision=appearance_revision+1 WHERE identity_id=?1 AND active_session_id=?2 AND binding_revision=?3 AND appearance_revision=?4",params![identity,session,binding,input.expected_revision as i64])?;
        if changed != 1 {
            return Err(CloudError::RevisionConflict);
        }
        let revision = input.expected_revision as i64 + 1;
        let now = crate::visual::now_ms();
        let has_asset = input.asset_id.is_some();
        let motion = if has_asset {
            input
                .motion
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|_| CloudError::invalid_metadata("invalid motion"))?
        } else {
            None
        };
        tx.execute("INSERT INTO cloud_offline_appearances(identity_id,session_id,binding_revision,revision,asset_id,asset_revision,raw_sha256,texture_id,motion_json,display_json,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(identity_id) DO UPDATE SET session_id=excluded.session_id,binding_revision=excluded.binding_revision,revision=excluded.revision,asset_id=excluded.asset_id,asset_revision=excluded.asset_revision,raw_sha256=excluded.raw_sha256,texture_id=excluded.texture_id,motion_json=excluded.motion_json,display_json=excluded.display_json,updated_at=excluded.updated_at",
            params![identity,session,binding,revision,input.asset_id,if has_asset{input.asset_revision.map(|v|v as i64)}else{None},if has_asset{input.raw_sha256.as_deref()}else{None},if has_asset{input.texture_id.as_deref()}else{None},motion,if has_asset{display}else{None},now])?;
        tx.execute(
            "UPDATE cloud_offline_sessions SET expires_at=?2 WHERE session_id=?1",
            params![session, now + i64::from(ttl) * 1000],
        )?;
        tx.commit()?;
        Ok(revision as u64)
    }

    pub fn withdraw_offline(
        &self,
        account: &str,
        session: &str,
        token: &str,
    ) -> Result<(), CloudError> {
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        let (identity, binding) = authorize(&tx, account, session, token)?;
        let changed=tx.execute("UPDATE cloud_offline_identities SET active_session_id=NULL,binding_revision=binding_revision+1,appearance_revision=appearance_revision+1 WHERE identity_id=?1 AND active_session_id=?2 AND binding_revision=?3",params![identity,session,binding])?;
        if changed != 1 {
            return Err(CloudError::OfflineBindingStale);
        }
        tx.execute(
            "UPDATE cloud_offline_sessions SET revoked_at=?2 WHERE session_id=?1",
            params![session, crate::visual::now_ms()],
        )?;
        tx.execute(
            "DELETE FROM cloud_offline_appearances WHERE identity_id=?1",
            [identity],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn query_offline_players(
        &self,
        account: &str,
        ids: &[uuid::Uuid],
    ) -> Result<Vec<PlayerAppearance>, CloudError> {
        let mut result = self.query_players(account, ids)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        active_account(&conn, account)?;
        match require_enabled(&conn) {
            Ok(()) => {}
            Err(CloudError::OfflineIdentityDisabled) => return Ok(result),
            Err(error) => return Err(error),
        }
        let mut statement=conn.prepare("SELECT i.identity_id,i.appearance_revision,p.asset_id,p.asset_revision,p.raw_sha256,p.texture_id,r.format,a.visibility,p.motion_json,p.display_json,i.account_id,s.expires_at,i.binding_revision FROM cloud_offline_sessions s JOIN cloud_offline_identities i ON i.active_session_id=s.session_id AND i.identity_id=s.identity_id AND i.binding_revision=s.binding_revision JOIN cloud_offline_keys k ON k.key_id=s.key_id AND k.generation=i.key_generation AND k.revoked_at IS NULL JOIN cloud_offline_authorizations grant_status ON grant_status.account_id=i.account_id AND grant_status.status='APPROVED' JOIN accounts owner ON owner.account_id=i.account_id LEFT JOIN admin_accounts adm ON adm.account_id=owner.account_id LEFT JOIN cloud_offline_appearances p ON p.identity_id=i.identity_id AND p.session_id=s.session_id AND p.binding_revision=s.binding_revision LEFT JOIN assets a ON a.asset_id=p.asset_id LEFT JOIN available_asset_revisions r ON r.asset_id=p.asset_id AND r.revision=p.asset_revision AND r.raw_sha256=p.raw_sha256 WHERE s.entity_uuid=?1 AND s.expires_at>?2 AND s.revoked_at IS NULL AND COALESCE(adm.disabled,0)=0")?;
        for entry in &mut result {
            if entry.publisher_id.is_some()
                || entry.discovery_status.as_deref() == Some("AMBIGUOUS")
            {
                continue;
            }
            let rows = statement
                .query_map(
                    params![entry.entity_uuid.to_string(), crate::visual::now_ms()],
                    |r| {
                        let mut selection = None;
                        if r.get::<_, Option<String>>(7)?.as_deref() == Some("PUBLIC")
                            && let (Some(asset), Some(format)) = (
                                r.get::<_, Option<String>>(2)?,
                                r.get::<_, Option<String>>(6)?,
                            )
                        {
                            let display = r.get::<_, Option<String>>(9)?.and_then(|text| {
                                if !self.visual_settings().features.player_display_state {
                                    return None;
                                }
                                let display: crate::display::PlayerDisplayUpdate =
                                    serde_json::from_str(&text).ok()?;
                                let publisher: String = r.get(10).ok()?;
                                for viewer in [publisher.as_str(), account] {
                                    crate::display::context(
                                        &conn,
                                        viewer,
                                        &display.scope_id,
                                        &display.world_epoch,
                                        &display.dimension_id,
                                    )
                                    .ok()?;
                                }
                                let mut value = serde_json::to_value(display).ok()?;
                                value["server_time_unix_ms"] = json!(crate::visual::now_ms());
                                value["expires_at_unix_ms"] = json!(r.get::<_, i64>(11).ok()?);
                                Some(value)
                            });
                            selection = Some(PlayerSelection {
                                asset_id: asset,
                                asset_revision: r.get::<_, i64>(3)? as u64,
                                raw_sha256: r.get(4)?,
                                texture_id: r.get(5)?,
                                format,
                                motion: r
                                    .get::<_, Option<String>>(8)?
                                    .and_then(|s| serde_json::from_str(&s).ok()),
                                display_state: display,
                            });
                        }
                        Ok((
                            format!(
                                "cloud-offline:{}:{}",
                                r.get::<_, String>(0)?,
                                r.get::<_, i64>(12)?
                            ),
                            r.get::<_, i64>(1)? as u64,
                            selection,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            if rows.len() == 1 {
                let (publisher, revision, selection) = rows.into_iter().next().unwrap();
                entry.publisher_id = Some(publisher);
                entry.identity_kind = Some("CLOUD_OFFLINE".into());
                entry.revision = revision;
                entry.selection = selection;
                entry.discovery_status = Some("ACTIVE".into());
            } else if rows.len() > 1 {
                entry.discovery_status = Some("AMBIGUOUS".into());
            }
        }
        Ok(result)
    }
}
