//! Embedded administration UI. Only the bootstrap owner can distribute administrators.
use crate::database::Connection;
use crate::{
    CloudStore,
    api::{AppState, authenticate},
    error::CloudError,
    models::LoginRequest,
};
use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString},
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post, put},
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[path = "admin_fonts.rs"]
mod fonts;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebUiSettings {
    pub enabled: bool,
    pub microsoft_client_id: Option<String>,
    pub microsoft_client_secret_file: Option<std::path::PathBuf>,
    pub microsoft_redirect_uri: Option<String>,
}
impl Default for WebUiSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            microsoft_client_id: None,
            microsoft_client_secret_file: None,
            microsoft_redirect_uri: None,
        }
    }
}

pub(crate) fn migrate(conn: &Connection) -> Result<(), CloudError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS admin_accounts(account_id TEXT PRIMARY KEY REFERENCES accounts(account_id),role TEXT NOT NULL CHECK(role IN ('admin','member')),disabled INTEGER NOT NULL DEFAULT 0);")?;
    crate::account_links::migrate(conn)?;
    crate::browser_auth::migrate(conn)?;
    Ok(())
}
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/admin", get(page))
        .route("/admin/", get(page))
        .route("/account/", get(page))
        .route("/account/api/login", post(crate::account_links::login))
        .route("/account/api/profile", get(crate::account_links::profile))
        .route(
            "/account/api/link-cloud",
            post(crate::account_links::connect),
        )
        .route(
            "/account/api/providers",
            get(crate::browser_auth::providers),
        )
        .route("/account/api/game", post(crate::browser_auth::game))
        .route(
            "/admin/oauth/microsoft/start",
            post(crate::browser_auth::microsoft_start),
        )
        .route(
            "/admin/oauth/microsoft/callback",
            get(crate::browser_auth::microsoft_callback),
        )
        .route(
            "/admin/oauth/microsoft/redeem",
            post(crate::browser_auth::microsoft_redeem),
        )
        .route("/admin/app.js", get(script))
        .route("/admin/settings.js", get(settings_script))
        .route("/admin/style.css", get(style))
        .route("/admin/fonts.css", get(font_styles))
        .route("/admin/fonts/{file}", get(font))
        .route("/admin/icons.svg", get(icons))
        .route("/admin/credits", get(credits))
        .route("/admin/api/login", post(login))
        .route("/admin/api/me", get(me))
        .route(
            "/admin/api/config",
            get(crate::admin_settings::get).put(crate::admin_settings::save),
        )
        .route(
            "/admin/api/config/validate",
            post(crate::admin_settings::validate),
        )
        .route(
            "/admin/api/providers/check",
            post(crate::admin_settings::provider_check),
        )
        .route(
            "/admin/api/config/check-connection",
            post(crate::admin_settings::check_connection),
        )
        .route("/admin/api/accounts", get(accounts).post(create_account))
        .route("/admin/api/accounts/{account}", put(update_account))
        .route("/admin/api/assets", get(assets).post(upload))
        .route("/admin/api/model-owners", get(crate::account_links::owners))
        .route(
            "/admin/api/assets/{asset}/owner",
            put(crate::account_links::assign),
        )
        .route(
            "/admin/api/assets/{asset}",
            axum::routing::delete(delete_asset),
        )
        .route("/admin/api/assets/{asset}/restore", post(restore_asset))
        .route("/admin/api/assets/{asset}/visibility", put(visibility))
        .route("/admin/api/assets/{asset}/moderation", put(mark))
        .route("/admin/api/assets/{asset}/scan", post(rescan))
        .route("/admin/api/audit", get(audit))
        .merge(crate::maintenance::routes())
        .merge(crate::preview::routes())
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state, guard))
}
async fn guard(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if !state.runtime_config().web_ui.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    let upload =
        request.method() == axum::http::Method::POST && request.uri().path() == "/admin/api/assets";
    let path = request.uri().path().to_owned();
    if path.starts_with("/admin/api/") && path != "/admin/api/login" {
        if let Err(error) = authorize(&state, request.headers(), false) {
            return error.into_response();
        }
    }
    let mut response = if upload {
        next.run(request).await
    } else {
        let (parts, body) = request.into_parts();
        // Administrative settings must remain editable even with a small client message budget.
        match axum::body::to_bytes(body, 1024 * 1024).await {
            Ok(bytes) => {
                next.run(axum::extract::Request::from_parts(parts, Body::from(bytes)))
                    .await
            }
            Err(_) => CloudError::MessageTooLarge.into_response(),
        }
    };
    let headers = response.headers_mut();
    if path.starts_with("/admin/fonts/") {
        // Fonts contain no account data; the CSS URLs include their content hashes.
        headers.insert("cache-control", "public, max-age=604800".parse().unwrap());
    } else {
        headers.insert("cache-control", "no-store".parse().unwrap());
    }
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("referrer-policy", "no-referrer".parse().unwrap());
    headers.insert("content-security-policy","default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; connect-src 'self'; img-src 'self' data: blob:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'".parse().unwrap());
    response
}
pub(crate) fn authorize(
    state: &AppState,
    headers: &HeaderMap,
    owner: bool,
) -> Result<(String, String), CloudError> {
    let account = authenticate(state, headers)?;
    let role = state
        .store
        .admin_role(&account, &state.config.bootstrap_account_id)?;
    if role == "owner" || (!owner && role == "admin") {
        Ok((account, role))
    } else {
        Err(CloudError::AccessDenied)
    }
}
async fn page() -> Html<&'static str> {
    Html(include_str!("../web/admin.html"))
}
async fn script() -> impl IntoResponse {
    (
        [("content-type", "text/javascript; charset=utf-8")],
        include_str!("../web/admin.js"),
    )
}
async fn settings_script() -> impl IntoResponse {
    (
        [("content-type", "text/javascript; charset=utf-8")],
        include_str!("../web/settings.js"),
    )
}
async fn style() -> impl IntoResponse {
    (
        [("content-type", "text/css; charset=utf-8")],
        include_str!("../web/admin.css"),
    )
}

async fn font_styles() -> impl IntoResponse {
    (
        [("content-type", "text/css; charset=utf-8")],
        include_str!("../web/fonts.css"),
    )
}

async fn font(Path(file): Path<String>) -> Response {
    match fonts::font(&file) {
        Some(bytes) => ([("content-type", "font/ttf")], bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn icons() -> impl IntoResponse {
    (
        [("content-type", "image/svg+xml")],
        include_str!("../web/icons.svg"),
    )
}

async fn credits() -> impl IntoResponse {
    (
        [("content-type", "text/plain; charset=utf-8")],
        include_str!("../web/credits.txt"),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    #[serde(default)]
    account_id: String,
    #[serde(default)]
    password: String,
    bootstrap_token: Option<String>,
}
async fn login(
    State(state): State<AppState>,
    Json(input): Json<Login>,
) -> Result<Json<Value>, CloudError> {
    login_attempt(&state)?;
    if let Some(token) = input.bootstrap_token {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {token}")
                .parse()
                .map_err(|_| CloudError::Unauthenticated)?,
        );
        let (account, role) = authorize(&state, &headers, true)?;
        return Ok(Json(
            json!({"access_token":token,"account_id":account,"role":role}),
        ));
    }
    let store = state.store.clone();
    let account_id = input.account_id.clone();
    let session = tokio::task::spawn_blocking(move || {
        store.issue_session(&LoginRequest {
            account_id: input.account_id,
            password: input.password,
        })
    })
    .await
    .map_err(|_| CloudError::Internal(anyhow::anyhow!("login worker failed")))??;
    let role = state
        .store
        .admin_role(&account_id, &state.config.bootstrap_account_id)?;
    if !matches!(role.as_str(), "owner" | "admin") {
        state.store.revoke_access_token(&session.access_token)?;
        return Err(CloudError::AccessDenied);
    }
    let mut result = serde_json::to_value(&session).unwrap();
    result["role"] = json!(role);
    result["account_id"] = json!(account_id);
    Ok(Json(result))
}
pub(crate) fn login_attempt(state: &AppState) -> Result<(), CloudError> {
    {
        let mut attempts = state
            .admin_login_attempts
            .lock()
            .map_err(|_| CloudError::configuration("login lock poisoned"))?;
        let now = crate::api::now_unix_ms();
        while attempts.front().is_some_and(|time| now - time > 60_000) {
            attempts.pop_front();
        }
        if attempts.len() >= 30 {
            return Err(CloudError::RateLimited);
        }
        attempts.push_back(now);
    }
    Ok(())
}
async fn me(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, CloudError> {
    let (account, role) = authorize(&state, &headers, false)?;
    Ok(Json(
        json!({"account_id":account,"role":role,"instance_id":state.config.instance_id}),
    ))
}
#[derive(Deserialize)]
struct AccountQuery {
    page: Option<usize>,
    page_size: Option<usize>,
    after: Option<String>,
    q: Option<String>,
}
async fn accounts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AccountQuery>,
) -> Result<Json<Value>, CloudError> {
    authorize(&state, &headers, true)?;
    let conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let search = format!(
        "%{}%",
        query
            .q
            .unwrap_or_default()
            .chars()
            .take(128)
            .collect::<String>()
    );
    let size = query
        .page_size
        .unwrap_or(if query.page.is_some() { 24 } else { 80 })
        .clamp(1, 80);
    let page = query.page.unwrap_or(1).clamp(1, 1_000_000);
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM accounts WHERE account_id LIKE ?1",
        [&search],
        |r| r.get(0),
    )?;
    let after = query.after.unwrap_or_default();
    let mut stmt=conn.prepare("SELECT a.account_id,a.created_at,COALESCE(m.role,'member'),COALESCE(m.disabled,0) FROM accounts a LEFT JOIN admin_accounts m ON m.account_id=a.account_id WHERE a.account_id LIKE ?1 AND (?2='' OR (a.account_id>?2 AND a.account_id<>?3)) ORDER BY a.account_id=?3 DESC,a.account_id LIMIT ?4 OFFSET ?5")?;
    let mut entries=stmt.query_map(params![search,after,state.config.bootstrap_account_id,(size+1) as i64,((page-1)*size) as i64],|r|{let id:String=r.get(0)?;Ok(json!({"role":if id==state.config.bootstrap_account_id{"owner".into()}else{r.get::<_,String>(2)?},"account_id":id,"created_at":r.get::<_,String>(1)?,"disabled":r.get::<_,bool>(3)?}))})?.collect::<Result<Vec<_>,_>>()?;
    let more = entries.len() > size;
    entries.truncate(size);
    let cursor = if more {
        entries.last().map(|e| e["account_id"].clone())
    } else {
        None
    };
    Ok(Json(
        json!({"entries":entries,"next_cursor":cursor,"has_more":more,"total":total,"page":page,"page_size":size}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountInput {
    account_id: String,
    password: String,
    role: String,
}
async fn create_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AccountInput>,
) -> Result<Json<Value>, CloudError> {
    let (actor, _) = authorize(&state, &headers, true)?;
    validate_role(&input.role)?;
    let store = state.store.clone();
    let id = input.account_id.clone();
    tokio::task::spawn_blocking(move || store.create_account(&id, &input.password))
        .await
        .map_err(|_| CloudError::Internal(anyhow::anyhow!("account worker failed")))??;
    state.store.set_admin_account(
        &actor,
        &input.account_id,
        &state.config.bootstrap_account_id,
        &input.role,
        false,
        None,
    )?;
    Ok(Json(
        json!({"account_id":input.account_id,"role":input.role}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountUpdate {
    role: String,
    disabled: bool,
    password: Option<String>,
}
async fn update_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(account): Path<String>,
    Json(input): Json<AccountUpdate>,
) -> Result<StatusCode, CloudError> {
    let (actor, _) = authorize(&state, &headers, true)?;
    let store = state.store.clone();
    let owner = state.config.bootstrap_account_id.clone();
    tokio::task::spawn_blocking(move || {
        store.set_admin_account(
            &actor,
            &account,
            &owner,
            &input.role,
            input.disabled,
            input.password.as_deref(),
        )
    })
    .await
    .map_err(|_| CloudError::Internal(anyhow::anyhow!("account worker failed")))??;
    Ok(StatusCode::NO_CONTENT)
}
fn validate_role(role: &str) -> Result<(), CloudError> {
    if matches!(role, "admin" | "member") {
        Ok(())
    } else {
        Err(CloudError::invalid_metadata(
            "role must be admin or member; only bootstrap can be owner",
        ))
    }
}

#[derive(Deserialize)]
struct AssetQuery {
    page: Option<usize>,
    page_size: Option<usize>,
    after: Option<String>,
    q: Option<String>,
    #[serde(default)]
    deleted: bool,
}
async fn assets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AssetQuery>,
) -> Result<Json<Value>, CloudError> {
    authorize(&state, &headers, false)?;
    let conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let search = format!(
        "%{}%",
        query
            .q
            .unwrap_or_default()
            .chars()
            .take(128)
            .collect::<String>()
    );
    let size = query
        .page_size
        .unwrap_or(if query.page.is_some() { 24 } else { 80 })
        .clamp(1, 80);
    let page = query.page.unwrap_or(1).clamp(1, 1_000_000);
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM assets a JOIN asset_revisions r ON r.asset_id=a.asset_id AND r.revision=a.current_revision LEFT JOIN deleted_assets d ON d.asset_id=a.asset_id WHERE (a.asset_id LIKE ?1 OR r.name LIKE ?1 OR a.owner_account_id LIKE ?1 OR r.format LIKE ?1) AND (?2=1 OR d.asset_id IS NULL)",params![search,query.deleted],|r|r.get(0))?;
    let mut stmt=conn.prepare("SELECT a.asset_id,a.owner_account_id,a.visibility,r.revision,r.name,r.format,r.byte_length,r.raw_sha256,COALESCE(m.manual_status,m.status,'UNKNOWN'),m.reason,m.scan_json,d.asset_id IS NOT NULL,m.manual_status
        FROM assets a JOIN asset_revisions r ON r.asset_id=a.asset_id AND r.revision=a.current_revision
        LEFT JOIN model_moderation m ON m.raw_sha256=r.raw_sha256 LEFT JOIN deleted_assets d ON d.asset_id=a.asset_id
        WHERE a.asset_id>?1 AND (a.asset_id LIKE ?2 OR r.name LIKE ?2 OR a.owner_account_id LIKE ?2 OR r.format LIKE ?2) AND (?3=1 OR d.asset_id IS NULL) ORDER BY a.asset_id LIMIT ?4 OFFSET ?5")?;
    let mut entries=stmt.query_map(params![query.after.unwrap_or_default(),search,query.deleted,(size+1) as i64,((page-1)*size) as i64],|r|Ok(json!({"asset_id":r.get::<_,String>(0)?,"owner_account_id":r.get::<_,String>(1)?,"visibility":r.get::<_,String>(2)?,"revision":r.get::<_,i64>(3)?,"name":r.get::<_,String>(4)?,"format":r.get::<_,String>(5)?,"byte_length":r.get::<_,i64>(6)?,"raw_sha256":r.get::<_,String>(7)?,"moderation":r.get::<_,String>(8)?,"reason":r.get::<_,Option<String>>(9)?,"scan":r.get::<_,Option<String>>(10)?.and_then(|s|serde_json::from_str::<Value>(&s).ok()),"deleted":r.get::<_,bool>(11)?,"manual_status":r.get::<_,Option<String>>(12)?})))?.collect::<Result<Vec<_>,_>>()?;
    let more = entries.len() > size;
    entries.truncate(size);
    let cursor = if more {
        entries.last().map(|e| e["asset_id"].clone())
    } else {
        None
    };
    Ok(Json(
        json!({"entries":entries,"next_cursor":cursor,"has_more":more,"total":total,"page":page,"page_size":size}),
    ))
}
async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, CloudError> {
    let (actor, _) = authorize(&state, &headers, false)?;
    let visibility = headers
        .get("x-asset-visibility")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("PUBLIC");
    let requested_owner = headers
        .get("x-asset-owner-account")
        .map(|v| {
            v.to_str()
                .map(str::to_owned)
                .map_err(|_| CloudError::invalid_metadata("invalid model owner"))
        })
        .transpose()?;
    if visibility.eq_ignore_ascii_case("PRIVATE") && requested_owner.is_none() {
        return Err(CloudError::invalid_metadata(
            "私有模型必须明确选择归属 Cloud 账号",
        ));
    }
    let owner = match requested_owner {
        Some(owner) => owner,
        None => state.store.linked_cloud_account(&actor)?,
    };
    state.store.require_active_account(&owner)?;
    crate::api::upload_asset_as(state, headers, body, Some(owner)).await
}
async fn delete_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
) -> Result<StatusCode, CloudError> {
    let (actor, _) = authorize(&state, &headers, false)?;
    state.store.admin_delete_asset(&actor, &asset, false)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn restore_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
) -> Result<StatusCode, CloudError> {
    let (actor, _) = authorize(&state, &headers, false)?;
    state.store.admin_delete_asset(&actor, &asset, true)?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Visibility {
    visibility: String,
}
async fn visibility(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
    Json(input): Json<Visibility>,
) -> Result<Json<Value>, CloudError> {
    let (actor, _) = authorize(&state, &headers, false)?;
    let (owner, _, _) = state.store.admin_asset(&asset)?;
    let result = state
        .store
        .set_asset_visibility(&owner, &asset, &input.visibility)?;
    state.store.admin_audit(
        &actor,
        "MODEL_VISIBILITY",
        Some(&asset),
        json!({"visibility":input.visibility}),
    )?;
    Ok(Json(json!(result)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mark {
    status: Option<String>,
}
async fn mark(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
    Json(input): Json<Mark>,
) -> Result<StatusCode, CloudError> {
    let (actor, _) = authorize(&state, &headers, false)?;
    if input
        .status
        .as_deref()
        .is_some_and(|s| !matches!(s, "R18" | "CLEAN" | "UNKNOWN"))
    {
        return Err(CloudError::invalid_metadata("invalid moderation status"));
    }
    let (_, sha, _) = state.store.admin_asset(&asset)?;
    let conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    conn.execute("INSERT INTO model_moderation(raw_sha256,status,reason,scan_json,manual_status,updated_at) VALUES(?1,'UNKNOWN','Not scanned','{}',?2,datetime('now')) ON CONFLICT(raw_sha256) DO UPDATE SET manual_status=excluded.manual_status,updated_at=excluded.updated_at",params![sha,input.status])?;
    drop(conn);
    state.store.admin_audit(
        &actor,
        "MODEL_MARKED",
        Some(&asset),
        json!({"status":input.status,"sha256":sha}),
    )?;
    Ok(StatusCode::NO_CONTENT)
}
async fn rescan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
) -> Result<Json<Value>, CloudError> {
    let (actor, _) = authorize(&state, &headers, false)?;
    let (_, sha, name) = state.store.admin_asset(&asset)?;
    let result = crate::moderation::scan_limited(
        &state.store.object_path_for_sha(&sha),
        &name,
        &state.runtime_config().moderation,
        state.processing_slots.clone(),
    )
    .await;
    state.store.save_scan(&sha, &result)?;
    state.store.admin_audit(
        &actor,
        "MODEL_SCANNED",
        Some(&asset),
        json!({"status":result.status}),
    )?;
    Ok(Json(json!(result)))
}
#[derive(Deserialize, Default)]
pub(crate) struct AuditQuery {
    pub q: Option<String>,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
}
pub(crate) fn audit_entries(
    state: &AppState,
    query: &AuditQuery,
    export: bool,
) -> Result<Value, CloudError> {
    let conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let search = format!(
        "%{}%",
        query
            .q
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(128)
            .collect::<String>()
    );
    let filter = "scope_id IS NULL AND (actor_account_id LIKE ?1 OR action LIKE ?1 OR COALESCE(subject_id,'') LIKE ?1 OR details_json LIKE ?1 OR created_at LIKE ?1)";
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM audit_events WHERE {filter}"),
        [&search],
        |r| r.get(0),
    )?;
    let size = if export {
        total as usize
    } else {
        query.page_size.unwrap_or(100).clamp(1, 100)
    };
    let page = query.page.unwrap_or(1).clamp(1, 1_000_000);
    let mut stmt = conn.prepare(&format!("SELECT actor_account_id,action,subject_id,details_json,created_at FROM audit_events WHERE {filter} ORDER BY created_at DESC,rowid DESC LIMIT ?2 OFFSET ?3"))?;
    let entries=stmt.query_map(params![search,size as i64,if export {0} else {((page-1)*size) as i64}],|r|Ok(json!({"actor":r.get::<_,String>(0)?,"action":r.get::<_,String>(1)?,"subject":r.get::<_,Option<String>>(2)?,"details":serde_json::from_str::<Value>(&r.get::<_,String>(3)?).unwrap_or(Value::Null),"created_at":r.get::<_,String>(4)?})))?.collect::<Result<Vec<_>,_>>()?;
    Ok(json!({"entries":entries,"total":total,"page":page,"page_size":size}))
}
async fn audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Value>, CloudError> {
    authorize(&state, &headers, false)?;
    Ok(Json(audit_entries(&state, &query, false)?))
}

impl CloudStore {
    pub fn admin_role(&self, account: &str, owner: &str) -> Result<String, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let entry: Option<(String, bool)> = conn
            .query_row(
                "SELECT role,disabled FROM admin_accounts WHERE account_id=?1",
                [account],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if entry.as_ref().is_some_and(|e| e.1) {
            return Err(CloudError::AccessDenied);
        }
        Ok(if account == owner {
            "owner".into()
        } else {
            entry.map(|e| e.0).unwrap_or("member".into())
        })
    }
    pub fn set_admin_account(
        &self,
        actor: &str,
        account: &str,
        owner: &str,
        role: &str,
        disabled: bool,
        password: Option<&str>,
    ) -> Result<(), CloudError> {
        if actor != owner {
            return Err(CloudError::AccessDenied);
        }
        if account == owner && (role != "owner" || disabled || password.is_none()) {
            return Err(CloudError::invalid_metadata(
                "Bootstrap owner cannot be demoted or disabled",
            ));
        }
        if account != owner {
            validate_role(role)?;
        }
        let hash = if let Some(password) = password {
            if !(8..=1024).contains(&password.encode_utf16().count())
                || password.contains(['\r', '\n'])
            {
                return Err(CloudError::invalid_metadata(
                    "password must be 8..1024 UTF-16 units without line breaks",
                ));
            }
            let salt = SaltString::encode_b64(&rand::random::<[u8; 16]>())
                .map_err(|_| CloudError::Internal(anyhow::anyhow!("salt failed")))?;
            Some(
                Argon2::default()
                    .hash_password(password.as_bytes(), &salt)
                    .map_err(|_| CloudError::Internal(anyhow::anyhow!("hash failed")))?
                    .to_string(),
            )
        } else {
            None
        };
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        if !tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1)",
            [account],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(CloudError::NotFound);
        }
        if account != owner {
            tx.execute("INSERT INTO admin_accounts(account_id,role,disabled) VALUES(?1,?2,?3) ON CONFLICT(account_id) DO UPDATE SET role=excluded.role,disabled=excluded.disabled",params![account,role,disabled])?;
        }
        if let Some(hash) = hash {
            tx.execute("INSERT INTO account_credentials(account_id,password_hash) VALUES(?1,?2) ON CONFLICT(account_id) DO UPDATE SET password_hash=excluded.password_hash",params![account,hash])?;
        }
        tx.execute(
            "UPDATE sessions SET revoked=1 WHERE account_id=?1",
            [account],
        )?;
        super::store::write_audit(
            &tx,
            actor,
            "ADMIN_ACCOUNT_UPDATED",
            None,
            None,
            Some(account),
            json!({"role":role,"disabled":disabled,"password_reset":password.is_some()}),
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn admin_asset(&self, asset: &str) -> Result<(String, String, String), CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        conn.query_row("SELECT a.owner_account_id,r.raw_sha256,r.name FROM assets a JOIN asset_revisions r ON r.asset_id=a.asset_id AND r.revision=a.current_revision WHERE a.asset_id=?1",[asset],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(CloudError::NotFound)
    }
    pub fn admin_audit(
        &self,
        actor: &str,
        action: &str,
        subject: Option<&str>,
        details: Value,
    ) -> Result<(), CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        super::store::write_audit(&conn, actor, action, None, None, subject, details)
    }
    pub fn admin_delete_asset(
        &self,
        actor: &str,
        asset: &str,
        restore: bool,
    ) -> Result<(), CloudError> {
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        if !tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM assets WHERE asset_id=?1)",
            [asset],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(CloudError::NotFound);
        }
        if restore {
            tx.execute("DELETE FROM deleted_assets WHERE asset_id=?1", [asset])?;
        } else {
            tx.execute(
                "INSERT OR IGNORE INTO deleted_assets VALUES(?1,?2,datetime('now'))",
                params![asset, actor],
            )?;
        }
        super::store::write_audit(
            &tx,
            actor,
            if restore {
                "MODEL_RESTORED"
            } else {
                "MODEL_DELETED"
            },
            None,
            None,
            Some(asset),
            json!({}),
        )?;
        tx.commit()?;
        Ok(())
    }
}
