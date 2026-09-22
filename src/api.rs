use std::{sync::Arc, time::SystemTime};

use axum::{body::Body, extract::{Path, State}, http::{header, HeaderMap, HeaderValue, StatusCode}, response::Response, routing::{get, post}, Json, Router};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::{io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt}, fs};
use uuid::Uuid;

use crate::{config::CloudConfig, error::CloudError, models::{AccountSummary, AclUpdate, AppearanceUpdate, CreateIdentity, CreateScope, CreateTarget, InstanceResponse, Limits, OfflineBindingRequest}, protocol::{HEARTBEAT_INTERVAL_SECONDS, HEARTBEAT_TTL_SECONDS, PROTOCOL_V1}, store::CloudStore};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<CloudConfig>,
    pub store: CloudStore,
}

impl AppState {
    pub fn new(config: CloudConfig, store: CloudStore) -> Self { Self { config: Arc::new(config), store } }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/instance", get(instance))
        .route("/v1/accounts", post(create_account))
        .route("/v1/identity-providers", get(identity_providers))
        .route("/v1/identities", get(list_identities).post(create_identity))
        .route("/v1/identities/{identity_id}/offline-bindings", post(create_offline_binding))
        .route("/v1/scopes", get(list_scopes).post(create_scope))
        .route("/v1/assets", get(list_assets).post(upload_asset))
        .route("/v1/catalog/recovery", get(catalog_recovery))
        .route("/v1/assets/{asset_id}/revisions/{revision}/content", get(download_asset))
        .route("/v1/targets", post(create_target))
        .route("/v1/scopes/{scope_id}/targets", get(list_targets))
        .route("/v1/targets/{target_id}/appearance", get(get_appearance).put(update_appearance))
        .route("/v1/targets/{target_id}/acl", get(list_acl).put(set_acl))
        .with_state(state)
}

async fn health() -> &'static str { "ok" }

async fn instance(State(state): State<AppState>) -> Json<InstanceResponse> {
    Json(InstanceResponse {
        instance_id: state.config.instance_id.clone(),
        origin: state.config.origin.clone(),
        websocket_origin: websocket_origin(&state.config.origin),
        protocol: PROTOCOL_V1.to_owned(),
        limits: Limits { max_message_bytes: state.config.max_message_bytes, max_snapshot_bytes: 16 * 1024 * 1024, max_snapshot_chunk_bytes: 256 * 1024, max_asset_bytes: state.config.max_asset_bytes, max_subscriptions: 128, heartbeat_interval_seconds: HEARTBEAT_INTERVAL_SECONDS, heartbeat_ttl_seconds: HEARTBEAT_TTL_SECONDS },
    })
}

async fn identity_providers(State(state): State<AppState>) -> Result<Json<Vec<serde_json::Value>>, CloudError> { Ok(Json(state.store.list_providers()?)) }

async fn create_account(State(state): State<AppState>, headers: HeaderMap, Json(input): Json<AccountSummary>) -> Result<(StatusCode, Json<AccountSummary>), CloudError> {
    authenticate(&state, &headers)?;
    Ok((StatusCode::CREATED, Json(state.store.create_account(&input.account_id)?)))
}

async fn list_identities(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<crate::models::IdentitySummary>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_identities(&account)?))
}

async fn create_identity(State(state): State<AppState>, headers: HeaderMap, Json(input): Json<CreateIdentity>) -> Result<(StatusCode, Json<crate::models::IdentitySummary>), CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok((StatusCode::CREATED, Json(state.store.create_identity(&account, &input)?)))
}

async fn create_offline_binding(State(state): State<AppState>, headers: HeaderMap, Path(identity_id): Path<String>, Json(mut input): Json<OfflineBindingRequest>) -> Result<Json<serde_json::Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    input.identity_id = identity_id;
    Ok(Json(state.store.create_offline_binding(&account, &input)?))
}

async fn list_scopes(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<crate::models::ScopeSummary>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_scopes(&account)?))
}

async fn create_scope(State(state): State<AppState>, headers: HeaderMap, Json(input): Json<CreateScope>) -> Result<(StatusCode, Json<crate::models::ScopeSummary>), CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok((StatusCode::CREATED, Json(state.store.create_scope(&account, &input)?)))
}

async fn create_target(State(state): State<AppState>, headers: HeaderMap, Json(input): Json<CreateTarget>) -> Result<(StatusCode, Json<serde_json::Value>), CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok((StatusCode::CREATED, Json(state.store.create_target(&account, &input)?)))
}

async fn list_targets(State(state): State<AppState>, headers: HeaderMap, Path(scope_id): Path<String>) -> Result<Json<Vec<crate::models::TargetSummary>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_targets(&account, &scope_id)?))
}

async fn list_acl(State(state): State<AppState>, headers: HeaderMap, Path(target_id): Path<String>) -> Result<Json<Vec<crate::models::AclEntry>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_acl(&account, &target_id)?))
}

async fn set_acl(State(state): State<AppState>, headers: HeaderMap, Path(target_id): Path<String>, Json(input): Json<AclUpdate>) -> Result<Json<crate::models::AclEntry>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.set_acl(&account, &target_id, &input)?))
}

async fn get_appearance(State(state): State<AppState>, headers: HeaderMap, Path(target_id): Path<String>) -> Result<Json<crate::models::AppearanceState>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.get_appearance(&account, &target_id)?))
}

async fn update_appearance(State(state): State<AppState>, headers: HeaderMap, Path(target_id): Path<String>, Json(input): Json<AppearanceUpdate>) -> Result<Json<crate::models::AppearanceState>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.update_appearance(&account, &target_id, &input)?))
}

async fn list_assets(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<crate::models::AssetSummary>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_assets(&account)?))
}

async fn catalog_recovery(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<serde_json::Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    let entries = state.store.list_assets(&account)?;
    Ok(Json(serde_json::json!({ "view_epoch": 1, "cursor": {"tenant_id": account, "view_epoch": 1, "offset": entries.len()}, "entries": entries })))
}

async fn upload_asset(State(state): State<AppState>, headers: HeaderMap, body: Body) -> Result<(StatusCode, Json<crate::models::AssetSummary>), CloudError> {
    let account = authenticate(&state, &headers)?;
    let asset_id = header_string(&headers, "x-asset-id")?.unwrap_or_else(|| format!("asset_{}", Uuid::new_v4().simple()));
    let name = header_string(&headers, "x-asset-name")?.unwrap_or_else(|| asset_id.clone());
    let format = header_string(&headers, "x-asset-format")?.unwrap_or_else(|| "application/octet-stream".to_owned());
    let expected_sha = header_string(&headers, "x-asset-sha256")?;
    let content_length = headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    if content_length.is_some_and(|length| length > state.config.max_asset_bytes) { return Err(CloudError::AssetTooLarge); }
    let temp_path = state.config.object_dir.join(format!(".upload-{}", Uuid::new_v4().simple()));
    let mut file = fs::File::create(&temp_path).await?;
    let mut stream = body.into_data_stream();
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| CloudError::Internal(anyhow::anyhow!("request body stream failed")))?;
        total = total.saturating_add(chunk.len() as u64);
        if total > state.config.max_asset_bytes { let _ = fs::remove_file(&temp_path).await; return Err(CloudError::AssetTooLarge); }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    if total == 0 { let _ = fs::remove_file(&temp_path).await; return Err(CloudError::invalid_metadata("asset must not be empty")); }
    let sha256 = hex::encode(hasher.finalize());
    if expected_sha.as_deref().is_some_and(|expected| !expected.eq_ignore_ascii_case(&sha256)) { let _ = fs::remove_file(&temp_path).await; return Err(CloudError::AssetHashMismatch); }
    let object_path = state.store.object_path_for_sha(&sha256);
    if let Some(parent) = object_path.parent() { fs::create_dir_all(parent).await?; }
    if fs::try_exists(&object_path).await.unwrap_or(false) { let _ = fs::remove_file(&temp_path).await; } else { fs::rename(&temp_path, &object_path).await?; }
    let summary = state.store.register_asset(&account, &asset_id, &name, &format, &sha256, total, &object_path)?;
    Ok((StatusCode::CREATED, Json(summary)))
}

async fn download_asset(State(state): State<AppState>, headers: HeaderMap, Path((asset_id, revision)): Path<(String, u64)>) -> Result<Response, CloudError> {
    let _account = authenticate(&state, &headers)?;
    let content = state.store.asset_content(&asset_id, revision)?;
    let etag = format!("\"{}\"", content.raw_sha256);
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()).is_some_and(|value| value == etag) && headers.get(header::RANGE).is_none() {
        return Ok(Response::builder().status(StatusCode::NOT_MODIFIED).header(header::ETAG, etag).body(Body::empty()).unwrap());
    }
    let (start, end, partial) = parse_range(headers.get(header::RANGE), content.length)?;
    let length = end - start + 1;
    let mut file = fs::File::open(&content.path).await?;
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let stream = tokio_util::io::ReaderStream::new(file.take(length));
    let mut response = Response::builder().status(if partial { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK }).header(header::ETAG, etag).header(header::ACCEPT_RANGES, "bytes").header(header::CONTENT_LENGTH, length.to_string()).header(header::CONTENT_TYPE, "application/octet-stream");
    if partial { response = response.header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", start, end, content.length)); }
    response.body(Body::from_stream(stream)).map_err(|_| CloudError::Internal(anyhow::anyhow!("failed to build response")))
}

fn parse_range(value: Option<&HeaderValue>, total: u64) -> Result<(u64, u64, bool), CloudError> {
    let Some(value) = value else { return Ok((0, total.saturating_sub(1), false)); };
    let text = value.to_str().map_err(|_| CloudError::AssetRangeInvalid)?;
    let Some(range) = text.strip_prefix("bytes=") else { return Err(CloudError::AssetRangeInvalid); };
    if range.contains(',') { return Err(CloudError::AssetRangeInvalid); }
    let (start_text, end_text) = range.split_once('-').ok_or(CloudError::AssetRangeInvalid)?;
    let (start, end) = if start_text.is_empty() {
        let suffix = end_text.parse::<u64>().map_err(|_| CloudError::AssetRangeInvalid)?;
        if suffix == 0 { return Err(CloudError::AssetRangeInvalid); }
        (total.saturating_sub(suffix), total.saturating_sub(1))
    } else {
        let start = start_text.parse::<u64>().map_err(|_| CloudError::AssetRangeInvalid)?;
        let end = if end_text.is_empty() { total.saturating_sub(1) } else { end_text.parse::<u64>().map_err(|_| CloudError::AssetRangeInvalid)?.min(total.saturating_sub(1)) };
        (start, end)
    };
    if total == 0 || start >= total || start > end { return Err(CloudError::AssetRangeInvalid); }
    Ok((start, end, true))
}

pub fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<String, CloudError> {
    let Some(configured) = state.config.access_token.as_deref() else { return Err(CloudError::Unauthenticated); };
    let Some(value) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) else { return Err(CloudError::Unauthenticated); };
    let Some(token) = value.strip_prefix("Bearer ") else { return Err(CloudError::Unauthenticated); };
    if configured.as_bytes().ct_eq(token.as_bytes()).unwrap_u8() != 1 { return Err(CloudError::Unauthenticated); }
    Ok(state.config.bootstrap_account_id.clone())
}

fn header_string(headers: &HeaderMap, name: &str) -> Result<Option<String>, CloudError> {
    headers.get(name).map(|v| v.to_str().map(str::to_owned).map_err(|_| CloudError::InvalidMetadata(format!("invalid {name} header")))).transpose()
}

fn websocket_origin(origin: &str) -> String {
    if let Some(rest) = origin.strip_prefix("https://") { format!("wss://{rest}/v1/realtime") } else if let Some(rest) = origin.strip_prefix("http://") { format!("ws://{rest}/v1/realtime") } else { format!("{origin}/v1/realtime") }
}

pub fn now_unix_ms() -> i64 { SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis() as i64 }
