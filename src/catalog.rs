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
    let cache = state.catalog_cache.read().unwrap().clone();
    let epoch = {
        let conn = state
            .store
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let external = conn.external_version()?;
        (conn.total_changes(), external)
    };
    let key = cache.key(&json!([account, scope, query, after, limit, epoch]));
    if let Some(page) = cache.get(&key).await {
        if cached_page_is_current(&state, &account, &scope, &page)? {
            return Ok(Json(page));
        }
    }
    let page = load_page(&state, &account, &scope, &query, &after, limit)?;
    cache.put(key, page.clone()).await;
    Ok(Json(page))
}

fn load_page(
    state: &AppState,
    account: &str,
    scope: &str,
    query: &str,
    after: &str,
    limit: i64,
) -> Result<Value, CloudError> {
    let needle = format!("%{}%", query.to_lowercase());
    let conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let mut statement=conn.prepare("SELECT a.asset_id,r.revision,r.name,r.format,r.raw_sha256,r.byte_length,a.visibility,a.owner_account_id
        FROM assets a JOIN available_asset_revisions r ON r.asset_id=a.asset_id AND r.revision=a.current_revision
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
                let owner: String = r.get(7)?;
                let mut entry = serde_json::to_value(AssetSummary {
                    asset_id: r.get(0)?,
                    revision: r.get::<_, i64>(1)? as u64,
                    name: r.get(2)?,
                    format: r.get(3)?,
                    raw_sha256: r.get(4)?,
                    byte_length: r.get::<_, i64>(5)? as u64,
                    visibility: r.get(6)?,
                })
                .expect("asset summary serializes");
                entry["owned"] = json!(owner == account);
                entry["owner_account_id"] = json!(owner);
                Ok(entry)
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let more = entries.len() > limit as usize;
    entries.truncate(limit as usize);
    let cursor = if more {
        entries
            .last()
            .and_then(|e| e["asset_id"].as_str())
            .map(str::to_owned)
    } else {
        None
    };
    Ok(json!({"entries":entries,"next_cursor":cursor,"has_more":more,"scope":scope,"query":query}))
}

/// Cached directory rows never grant access. Recheck current visibility, ACL, revision and moderation in one bounded query.
fn cached_page_is_current(
    state: &AppState,
    account: &str,
    scope: &str,
    page: &Value,
) -> Result<bool, CloudError> {
    let Some(entries) = page["entries"].as_array() else {
        return Ok(false);
    };
    if entries.len() > 80 {
        return Ok(false);
    }
    if entries.is_empty() {
        return Ok(true);
    }
    let mut parameters = vec![
        rusqlite::types::Value::Text(account.into()),
        rusqlite::types::Value::Text(scope.into()),
    ];
    for entry in entries {
        let Some(id) = entry["asset_id"].as_str() else {
            return Ok(false);
        };
        parameters.push(rusqlite::types::Value::Text(id.into()));
    }
    let placeholders = (3..parameters.len() + 1)
        .map(|n| format!("?{n}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT a.asset_id,r.revision,r.name,r.format,r.raw_sha256,r.byte_length,a.visibility,a.owner_account_id
        FROM assets a JOIN available_asset_revisions r ON r.asset_id=a.asset_id AND r.revision=a.current_revision
        LEFT JOIN asset_acl acl ON acl.asset_id=a.asset_id AND acl.account_id=?1
        WHERE a.asset_id IN ({placeholders}) AND ((?2='mine' AND a.owner_account_id=?1)
            OR (?2='shared' AND a.owner_account_id<>?1 AND acl.permission IN ('use','discover','render_read'))
            OR (?2='public' AND a.visibility='PUBLIC')
            OR (?2='accessible' AND (a.visibility='PUBLIC' OR a.owner_account_id=?1 OR acl.permission IN ('manage','use','discover','render_read'))))");
    let conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let rows = conn.prepare(&sql)?.query_map(parameters, |r| {
        let id: String=r.get(0)?;
        let matches=entries.iter().find(|entry|entry["asset_id"].as_str()==Some(id.as_str())).is_some_and(|entry| {
            let actual: rusqlite::Result<Value> = (|| Ok(json!({"asset_id":id,"revision":r.get::<_,i64>(1)?,"name":r.get::<_,String>(2)?,"format":r.get::<_,String>(3)?,"raw_sha256":r.get::<_,String>(4)?,"byte_length":r.get::<_,i64>(5)?,"visibility":r.get::<_,String>(6)?,"owner_account_id":r.get::<_,String>(7)?,"owned":r.get::<_,String>(7)?==account})))();
            actual.as_ref().is_ok_and(|actual|actual==entry)
        });
        Ok(matches)
    })?.collect::<Result<Vec<_>,_>>()?;
    Ok(rows.len() == entries.len() && rows.iter().all(|v| *v))
}
