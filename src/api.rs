use std::{sync::Arc, time::SystemTime};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::{delete, get, post, put},
};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    net::lookup_host,
    sync::broadcast,
};
use uuid::Uuid;

use crate::{
    config::CloudConfig,
    error::CloudError,
    models::{
        AccountSummary, AclUpdate, AppearanceUpdate, AssetAclUpdate, ClaimCodeRequest,
        CreateAccount, CreateIdentity, CreateIdentityChallenge, CreateScope, CreateTarget,
        IdentityChallengeResponse, IdentityProviderUpdate, IdentitySummary, InstanceResponse,
        Limits, LoginRequest, ObserveEntityBinding, OfflineBindingApproval, OfflineBindingRequest,
        RedeemClaimCode, RegisterEntityBinding, RevokeClaimCode, ScopeAclUpdate,
        VerifyIdentityChallenge,
    },
    protocol::{HEARTBEAT_INTERVAL_SECONDS, HEARTBEAT_TTL_SECONDS, PROTOCOL_V1},
    store::CloudStore,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<CloudConfig>,
    pub store: CloudStore,
    pub events: broadcast::Sender<CloudEvent>,
}

#[derive(Clone)]
pub struct CloudEvent {
    pub event_id: String,
    pub scope_id: String,
    pub appearance: crate::models::AppearanceState,
}

impl AppState {
    pub fn new(config: CloudConfig, store: CloudStore) -> Self {
        let (events, _) = broadcast::channel(512);
        Self {
            config: Arc::new(config),
            store,
            events,
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/instance", get(instance))
        .route("/v1/accounts", post(create_account))
        .route("/v1/sessions", post(login))
        .route("/v1/sessions/refresh", post(refresh_session))
        .route("/v1/sessions/current", delete(logout))
        .route(
            "/v1/identity-providers",
            get(identity_providers).post(configure_identity_provider),
        )
        .route("/v1/auth/challenges", post(create_identity_challenge))
        .route(
            "/v1/auth/challenges/{challenge_id}/verify",
            post(verify_identity_challenge),
        )
        .route(
            "/v1/auth/challenges/{challenge_id}/complete",
            post(verify_identity_challenge),
        )
        .route("/v1/identities", get(list_identities).post(create_identity))
        .route(
            "/v1/identities/{identity_id}/offline-bindings",
            post(create_offline_binding),
        )
        .route("/v1/claim-codes/redeem", post(redeem_claim_code))
        .route("/v1/claim-codes/revoke", post(revoke_claim_code))
        .route(
            "/v1/targets/{target_id}/claim-codes",
            post(create_claim_code),
        )
        .route(
            "/v1/scoped-identity-bindings/{binding_id}",
            put(approve_offline_binding),
        )
        .route("/v1/scopes", get(list_scopes).post(create_scope))
        .route(
            "/v1/scopes/{scope_id}/acl",
            get(list_scope_acl).put(set_scope_acl),
        )
        .route("/v1/assets", get(list_assets).post(upload_asset))
        .route(
            "/v1/assets/{asset_id}/acl",
            get(list_asset_acl).put(set_asset_acl),
        )
        .route("/v1/catalog/recovery", get(catalog_recovery))
        .route(
            "/v1/assets/{asset_id}/revisions/{revision}/content",
            get(download_asset),
        )
        .route("/v1/targets", post(create_target))
        .route("/v1/scopes/{scope_id}/targets", get(list_targets))
        .route(
            "/v1/scopes/{scope_id}/bindings",
            get(list_bindings).post(register_binding),
        )
        .route(
            "/v1/scopes/{scope_id}/offline-bindings",
            get(list_offline_bindings),
        )
        .route("/v1/scopes/{scope_id}/audit", get(list_audit))
        .route(
            "/v1/bindings/{binding_id}/observation",
            put(observe_binding),
        )
        .route(
            "/v1/scopes/{scope_id}/events/recovery",
            get(outbox_recovery),
        )
        .route(
            "/v1/targets/{target_id}/appearance",
            get(get_appearance).put(update_appearance),
        )
        .route("/v1/targets/{target_id}/acl", get(list_acl).put(set_acl))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn instance(State(state): State<AppState>) -> Json<InstanceResponse> {
    Json(InstanceResponse {
        instance_id: state.config.instance_id.clone(),
        origin: state.config.origin.clone(),
        websocket_origin: websocket_origin(&state.config.origin),
        protocol: PROTOCOL_V1.to_owned(),
        limits: Limits {
            max_message_bytes: state.config.max_message_bytes,
            max_snapshot_bytes: 16 * 1024 * 1024,
            max_snapshot_chunk_bytes: 256 * 1024,
            max_asset_bytes: state.config.max_asset_bytes,
            max_subscriptions: 128,
            heartbeat_interval_seconds: HEARTBEAT_INTERVAL_SECONDS,
            heartbeat_ttl_seconds: HEARTBEAT_TTL_SECONDS,
        },
    })
}

async fn identity_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, CloudError> {
    Ok(Json(state.store.list_providers()?))
}

async fn configure_identity_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<IdentityProviderUpdate>,
) -> Result<Json<serde_json::Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    if account != state.config.bootstrap_account_id {
        return Err(CloudError::AccessDenied);
    }
    Ok(Json(state.store.configure_provider(&input)?))
}

async fn create_identity_challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateIdentityChallenge>,
) -> Result<Json<IdentityChallengeResponse>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(
        state.store.create_identity_challenge(&account, &input)?,
    ))
}

async fn verify_identity_challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(challenge_id): Path<String>,
    Json(input): Json<VerifyIdentityChallenge>,
) -> Result<Json<IdentitySummary>, CloudError> {
    let account = authenticate(&state, &headers)?;
    if input.challenge_id != challenge_id {
        return Err(CloudError::invalid_metadata(
            "challenge_id path/body mismatch",
        ));
    }
    let challenge = state.store.provider_challenge(&account, &challenge_id)?;
    let base_url = state.store.provider_base_url(&challenge.provider_id)?;
    let mut provider_url =
        reqwest::Url::parse(&base_url).map_err(|_| CloudError::IdentityProviderUntrusted)?;
    let host = provider_url
        .host_str()
        .ok_or(CloudError::IdentityProviderUntrusted)?
        .to_owned();
    let port = provider_url
        .port_or_known_default()
        .ok_or(CloudError::IdentityProviderUntrusted)?;
    let addresses: Vec<_> = lookup_host((host.as_str(), port))
        .await
        .map_err(|error| {
            CloudError::Internal(anyhow::anyhow!(
                "identity provider DNS lookup failed: {error}"
            ))
        })?
        .filter(|address| is_allowed_provider_address(address.ip()))
        .collect();
    let address = addresses
        .first()
        .copied()
        .ok_or(CloudError::IdentityProviderUntrusted)?;
    provider_url.set_path("/session/minecraft/hasJoined");
    provider_url
        .query_pairs_mut()
        .append_pair("username", &challenge.username)
        .append_pair("serverId", &challenge.server_id);
    let client = reqwest::Client::builder()
        .resolve(&host, address)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|error| {
            CloudError::Internal(anyhow::anyhow!(
                "provider client configuration failed: {error}"
            ))
        })?;
    let response = client.get(provider_url).send().await.map_err(|error| {
        CloudError::Internal(anyhow::anyhow!("identity provider request failed: {error}"))
    })?;
    if !response.status().is_success() {
        return Err(CloudError::IdentityProfileMismatch);
    }
    let profile: serde_json::Value = response.json().await.map_err(|error| {
        CloudError::Internal(anyhow::anyhow!(
            "identity provider response was invalid: {error}"
        ))
    })?;
    let profile_uuid = profile
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or(CloudError::IdentityProfileMismatch)?;
    let display_name = profile
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&challenge.username);
    Ok(Json(state.store.complete_identity_challenge(
        &account,
        &challenge_id,
        profile_uuid,
        display_name,
    )?))
}

async fn create_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateAccount>,
) -> Result<(StatusCode, Json<AccountSummary>), CloudError> {
    authenticate(&state, &headers)?;
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .store
                .create_account(&input.account_id, &input.password)?,
        ),
    ))
}

async fn login(
    State(state): State<AppState>,
    Json(input): Json<LoginRequest>,
) -> Result<Json<crate::models::SessionResponse>, CloudError> {
    Ok(Json(state.store.issue_session(&input)?))
}

#[derive(serde::Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

async fn refresh_session(
    State(state): State<AppState>,
    Json(input): Json<RefreshRequest>,
) -> Result<Json<crate::models::SessionResponse>, CloudError> {
    Ok(Json(state.store.refresh_session(&input.refresh_token)?))
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, CloudError> {
    let token = bearer_token(&headers)?;
    state.store.revoke_access_token(token)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_identities(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::models::IdentitySummary>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_identities(&account)?))
}

async fn create_identity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateIdentity>,
) -> Result<(StatusCode, Json<crate::models::IdentitySummary>), CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok((
        StatusCode::CREATED,
        Json(state.store.create_identity(&account, &input)?),
    ))
}

async fn create_offline_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(identity_id): Path<String>,
    Json(mut input): Json<OfflineBindingRequest>,
) -> Result<Json<serde_json::Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    input.identity_id = identity_id;
    Ok(Json(state.store.create_offline_binding(&account, &input)?))
}

async fn create_claim_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target_id): Path<String>,
    Json(input): Json<ClaimCodeRequest>,
) -> Result<(StatusCode, Json<crate::models::ClaimCodeResponse>), CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .store
                .create_claim_code(&account, &target_id, &input)?,
        ),
    ))
}

async fn redeem_claim_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RedeemClaimCode>,
) -> Result<Json<crate::models::ScopedIdentityBindingSummary>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.redeem_claim_code(&account, &input)?))
}

async fn revoke_claim_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RevokeClaimCode>,
) -> Result<StatusCode, CloudError> {
    let account = authenticate(&state, &headers)?;
    state.store.revoke_claim_code(&account, &input)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn approve_offline_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(binding_id): Path<String>,
    Json(input): Json<OfflineBindingApproval>,
) -> Result<Json<crate::models::ScopedIdentityBindingSummary>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.approve_offline_binding(
        &account,
        &binding_id,
        &input,
    )?))
}

async fn list_scopes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::models::ScopeSummary>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_scopes(&account)?))
}

async fn create_scope(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateScope>,
) -> Result<(StatusCode, Json<crate::models::ScopeSummary>), CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok((
        StatusCode::CREATED,
        Json(state.store.create_scope(&account, &input)?),
    ))
}

async fn list_scope_acl(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope_id): Path<String>,
) -> Result<Json<Vec<crate::models::ScopeAclEntry>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_scope_acl(&account, &scope_id)?))
}

async fn set_scope_acl(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope_id): Path<String>,
    Json(input): Json<ScopeAclUpdate>,
) -> Result<Json<crate::models::ScopeAclEntry>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(
        state.store.set_scope_acl(&account, &scope_id, &input)?,
    ))
}

async fn create_target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateTarget>,
) -> Result<(StatusCode, Json<serde_json::Value>), CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok((
        StatusCode::CREATED,
        Json(state.store.create_target(&account, &input)?),
    ))
}

async fn list_targets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope_id): Path<String>,
) -> Result<Json<Vec<crate::models::TargetSummary>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_targets(&account, &scope_id)?))
}

async fn list_bindings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope_id): Path<String>,
) -> Result<Json<Vec<crate::models::EntityBindingSummary>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_bindings(&account, &scope_id)?))
}

async fn list_offline_bindings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope_id): Path<String>,
) -> Result<Json<Vec<crate::models::ScopedIdentityBindingSummary>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(
        state.store.list_offline_bindings(&account, &scope_id)?,
    ))
}

async fn list_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope_id): Path<String>,
    Query(query): Query<CatalogQuery>,
) -> Result<Json<Vec<crate::models::AuditEntry>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_audit(
        &account,
        &scope_id,
        query.limit.unwrap_or(100) as u64,
    )?))
}

async fn register_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope_id): Path<String>,
    Json(input): Json<RegisterEntityBinding>,
) -> Result<(StatusCode, Json<crate::models::EntityBindingSummary>), CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok((
        StatusCode::CREATED,
        Json(state.store.register_binding(&account, &scope_id, &input)?),
    ))
}

async fn observe_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(binding_id): Path<String>,
    Json(input): Json<ObserveEntityBinding>,
) -> Result<Json<crate::models::EntityBindingSummary>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.observe_binding(
        &account,
        &binding_id,
        &input,
    )?))
}

async fn outbox_recovery(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope_id): Path<String>,
    Query(query): Query<CatalogQuery>,
) -> Result<Json<serde_json::Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.outbox_recovery(
        &account,
        &scope_id,
        query.after.unwrap_or(0),
        query.limit.unwrap_or(256),
    )?))
}

async fn list_acl(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target_id): Path<String>,
) -> Result<Json<Vec<crate::models::AclEntry>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_acl(&account, &target_id)?))
}

async fn set_acl(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target_id): Path<String>,
    Json(input): Json<AclUpdate>,
) -> Result<Json<crate::models::AclEntry>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.set_acl(&account, &target_id, &input)?))
}

async fn get_appearance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target_id): Path<String>,
) -> Result<Json<crate::models::AppearanceState>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.get_appearance(&account, &target_id)?))
}

async fn update_appearance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target_id): Path<String>,
    Json(input): Json<AppearanceUpdate>,
) -> Result<Json<crate::models::AppearanceState>, CloudError> {
    let account = authenticate(&state, &headers)?;
    let mutation = state
        .store
        .update_appearance(&account, &target_id, &input)?;
    let _ = state.events.send(CloudEvent {
        event_id: mutation.event_id,
        scope_id: mutation.scope_id,
        appearance: mutation.appearance.clone(),
    });
    Ok(Json(mutation.appearance))
}

fn is_allowed_provider_address(address: std::net::IpAddr) -> bool {
    match address {
        std::net::IpAddr::V4(ip) => {
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast())
        }
        std::net::IpAddr::V6(ip) => {
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local())
        }
    }
}

async fn list_assets(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::models::AssetSummary>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_assets(&account)?))
}

async fn list_asset_acl(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset_id): Path<String>,
) -> Result<Json<Vec<crate::models::AssetAclEntry>>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.list_asset_acl(&account, &asset_id)?))
}

async fn set_asset_acl(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset_id): Path<String>,
    Json(input): Json<AssetAclUpdate>,
) -> Result<Json<crate::models::AssetAclEntry>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(
        state.store.set_asset_acl(&account, &asset_id, &input)?,
    ))
}

async fn catalog_recovery(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CatalogQuery>,
) -> Result<Json<serde_json::Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.catalog_recovery(
        &account,
        query.after.unwrap_or(0),
        query.limit.unwrap_or(256),
    )?))
}

#[derive(Debug, serde::Deserialize)]
struct CatalogQuery {
    after: Option<u64>,
    limit: Option<usize>,
}

async fn upload_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<crate::models::AssetSummary>), CloudError> {
    let account = authenticate(&state, &headers)?;
    let request_id = header_string(&headers, "idempotency-key")?
        .ok_or_else(|| CloudError::invalid_metadata("Idempotency-Key is required"))?;
    let asset_id = header_string(&headers, "x-asset-id")?
        .unwrap_or_else(|| format!("asset_{}", Uuid::new_v4().simple()));
    let name = header_string(&headers, "x-asset-name")?.unwrap_or_else(|| asset_id.clone());
    let format = header_string(&headers, "x-asset-format")?
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    let expected_sha = header_string(&headers, "x-asset-sha256")?;
    let content_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if content_length.is_some_and(|length| length > state.config.max_asset_bytes) {
        return Err(CloudError::AssetTooLarge);
    }
    let temp_path = state
        .config
        .object_dir
        .join(format!(".upload-{}", Uuid::new_v4().simple()));
    let mut file = fs::File::create(&temp_path).await?;
    let mut stream = body.into_data_stream();
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|_| CloudError::Internal(anyhow::anyhow!("request body stream failed")))?;
        total = total.saturating_add(chunk.len() as u64);
        if total > state.config.max_asset_bytes {
            let _ = fs::remove_file(&temp_path).await;
            return Err(CloudError::AssetTooLarge);
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    if total == 0 {
        let _ = fs::remove_file(&temp_path).await;
        return Err(CloudError::invalid_metadata("asset must not be empty"));
    }
    let sha256 = hex::encode(hasher.finalize());
    if expected_sha
        .as_deref()
        .is_some_and(|expected| !expected.eq_ignore_ascii_case(&sha256))
    {
        let _ = fs::remove_file(&temp_path).await;
        return Err(CloudError::AssetHashMismatch);
    }
    let object_path = state.store.object_path_for_sha(&sha256);
    if let Some(parent) = object_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let object_already_exists = fs::try_exists(&object_path).await.unwrap_or(false);
    if object_already_exists {
        let _ = fs::remove_file(&temp_path).await;
    } else {
        fs::rename(&temp_path, &object_path).await?;
    }
    let request_hash = hex::encode(Sha256::digest(
        serde_json::to_vec(&(
            asset_id.as_str(),
            name.as_str(),
            format.as_str(),
            expected_sha.as_deref(),
            total,
            sha256.as_str(),
        ))
        .map_err(|_| CloudError::invalid_metadata("invalid upload metadata"))?,
    ));
    let summary = match state.store.register_asset_with_idempotency(
        &account,
        &asset_id,
        &name,
        &format,
        &sha256,
        total,
        &object_path,
        Some((&request_id, &request_hash)),
    ) {
        Ok(summary) => summary,
        Err(error) => {
            if !object_already_exists {
                let _ = fs::remove_file(&object_path).await;
            }
            return Err(error);
        }
    };
    Ok((StatusCode::CREATED, Json(summary)))
}

async fn download_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((asset_id, revision)): Path<(String, u64)>,
) -> Result<Response, CloudError> {
    let account = authenticate(&state, &headers)?;
    let content = state.store.asset_content(&account, &asset_id, revision)?;
    let etag = format!("\"{}\"", content.raw_sha256);
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|value| value == etag)
        && headers.get(header::RANGE).is_none()
    {
        return Ok(Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, etag)
            .body(Body::empty())
            .unwrap());
    }
    let (start, end, partial) = parse_range(headers.get(header::RANGE), content.length)?;
    let length = end - start + 1;
    let mut file = fs::File::open(&content.path).await?;
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let stream = tokio_util::io::ReaderStream::new(file.take(length));
    let mut response = Response::builder()
        .status(if partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(header::ETAG, etag)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, length.to_string())
        .header(header::CONTENT_TYPE, "application/octet-stream");
    if partial {
        response = response.header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", start, end, content.length),
        );
    }
    response
        .body(Body::from_stream(stream))
        .map_err(|_| CloudError::Internal(anyhow::anyhow!("failed to build response")))
}

fn parse_range(value: Option<&HeaderValue>, total: u64) -> Result<(u64, u64, bool), CloudError> {
    let Some(value) = value else {
        return Ok((0, total.saturating_sub(1), false));
    };
    let text = value.to_str().map_err(|_| CloudError::AssetRangeInvalid)?;
    let Some(range) = text.strip_prefix("bytes=") else {
        return Err(CloudError::AssetRangeInvalid);
    };
    if range.contains(',') {
        return Err(CloudError::AssetRangeInvalid);
    }
    let (start_text, end_text) = range.split_once('-').ok_or(CloudError::AssetRangeInvalid)?;
    let (start, end) = if start_text.is_empty() {
        let suffix = end_text
            .parse::<u64>()
            .map_err(|_| CloudError::AssetRangeInvalid)?;
        if suffix == 0 {
            return Err(CloudError::AssetRangeInvalid);
        }
        (total.saturating_sub(suffix), total.saturating_sub(1))
    } else {
        let start = start_text
            .parse::<u64>()
            .map_err(|_| CloudError::AssetRangeInvalid)?;
        let end = if end_text.is_empty() {
            total.saturating_sub(1)
        } else {
            end_text
                .parse::<u64>()
                .map_err(|_| CloudError::AssetRangeInvalid)?
                .min(total.saturating_sub(1))
        };
        (start, end)
    };
    if total == 0 || start >= total || start > end {
        return Err(CloudError::AssetRangeInvalid);
    }
    Ok((start, end, true))
}

pub fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<String, CloudError> {
    let token = bearer_token(headers)?;
    if let Some(configured) = state.config.access_token.as_deref() {
        if configured.as_bytes().ct_eq(token.as_bytes()).unwrap_u8() == 1 {
            return Ok(state.config.bootstrap_account_id.clone());
        }
    }
    state.store.authenticate_access_token(token)
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, CloudError> {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return Err(CloudError::Unauthenticated);
    };
    value
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
        .ok_or(CloudError::Unauthenticated)
}

fn header_string(headers: &HeaderMap, name: &str) -> Result<Option<String>, CloudError> {
    headers
        .get(name)
        .map(|v| {
            v.to_str()
                .map(str::to_owned)
                .map_err(|_| CloudError::InvalidMetadata(format!("invalid {name} header")))
        })
        .transpose()
}

fn websocket_origin(origin: &str) -> String {
    if let Some(rest) = origin.strip_prefix("https://") {
        format!("wss://{rest}/v1/realtime")
    } else if let Some(rest) = origin.strip_prefix("http://") {
        format!("ws://{rest}/v1/realtime")
    } else {
        format!("{origin}/v1/realtime")
    }
}

pub fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
