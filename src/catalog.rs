//! Cursor discovery contract shared with the official Cloud.
use crate::{
    api::{AppState, authenticate},
    error::CloudError,
    models::AssetSummary,
};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Default, Deserialize)]
pub struct CatalogQuery {
    scope: Option<String>,
    q: Option<String>,
    after: Option<String>,
    limit: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<CatalogQuery>,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    let scope = input
        .scope
        .as_deref()
        .unwrap_or("accessible")
        .trim()
        .to_ascii_lowercase();
    if !matches!(scope.as_str(), "mine" | "shared" | "public" | "accessible") {
        return Err(CloudError::invalid_metadata("invalid asset scope"));
    }
    let query: String = input
        .q
        .as_deref()
        .unwrap_or("")
        .trim()
        .chars()
        .take(128)
        .collect();
    if scope == "public" && query.is_empty() {
        return Err(CloudError::SearchRequired);
    }
    let limit = input
        .limit
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(40)
        .clamp(1, 80);
    let after = input.after.unwrap_or_default();
    if after.len() > 1024 {
        return Err(CloudError::invalid_metadata("cursor is too long"));
    }
    let needle = format!("%{}%", query.to_lowercase());
    let conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let mut statement=conn.prepare("SELECT a.asset_id,r.revision,r.name,r.format,r.raw_sha256,r.byte_length,a.visibility
        FROM assets a JOIN asset_revisions r ON r.asset_id=a.asset_id AND r.revision=a.current_revision
        LEFT JOIN asset_acl acl ON acl.asset_id=a.asset_id AND acl.account_id=?1
        WHERE ((?2='mine' AND a.owner_account_id=?1)
            OR (?2='shared' AND a.owner_account_id<>?1 AND acl.permission IN ('use','discover','render_read'))
            OR (?2='public' AND a.visibility='PUBLIC')
            OR (?2='accessible' AND (a.visibility='PUBLIC' OR a.owner_account_id=?1 OR acl.permission IN ('manage','use','discover','render_read'))))
        AND (?3='' OR lower(a.asset_id) LIKE ?4 OR lower(r.name) LIKE ?4 OR lower(r.format) LIKE ?4)
        AND (?5='' OR a.asset_id>?5) ORDER BY a.asset_id LIMIT ?6")?;
    let mut entries = statement
        .query_map(
            params![account, scope, query, needle, after, limit + 1],
            |r| {
                Ok(AssetSummary {
                    asset_id: r.get(0)?,
                    revision: r.get::<_, i64>(1)? as u64,
                    name: r.get(2)?,
                    format: r.get(3)?,
                    raw_sha256: r.get(4)?,
                    byte_length: r.get::<_, i64>(5)? as u64,
                    visibility: r.get(6)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let more = entries.len() > limit as usize;
    entries.truncate(limit as usize);
    let cursor = if more {
        entries.last().map(|e| e.asset_id.clone())
    } else {
        None
    };
    Ok(Json(
        json!({"entries":entries,"next_cursor":cursor,"has_more":more,"scope":scope,"query":query}),
    ))
}
