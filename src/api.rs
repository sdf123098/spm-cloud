use std::{sync::Arc, time::SystemTime};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::broadcast,
};
use uuid::Uuid;

use crate::{
    config::CloudConfig,
    error::CloudError,
    models::{
        AccountSummary, AclUpdate, AnimationUpdate, AppearanceUpdate, AssetAclUpdate,
        AssetVisibilityUpdate, ClaimCodeRequest, CreateAccount, CreateIdentity, CreateScope,
        CreateTarget, IdentityProviderUpdate, InstanceResponse, Limits, LoginRequest,
        ObserveEntityBinding, OfflineBindingApproval, OfflineBindingRequest, RedeemClaimCode,
        RegisterEntityBinding, RevokeClaimCode, ScopeAclUpdate,
    },
    protocol::{
        HEARTBEAT_INTERVAL_SECONDS, HEARTBEAT_TTL_SECONDS, PLAYER_MOTION_CAPABILITY, PROTOCOL_V1,
    },
    store::CloudStore,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<CloudConfig>,
    pub store: CloudStore,
    pub events: broadcast::Sender<CloudEvent>,
}

#[derive(Clone)]
pub enum CloudEvent {
    Appearance {
        event_id: String,
        scope_id: String,
        appearance: crate::models::AppearanceState,
    },
    Animation {
        event_id: String,
        scope_id: String,
        animation: crate::models::AnimationState,
    },
}

impl CloudEvent {
    pub fn scope_id(&self) -> &str {
        match self {
            Self::Appearance { scope_id, .. } | Self::Animation { scope_id, .. } => scope_id,
        }
    }
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
        .route(
            "/v1/players/me/appearance",
            get(crate::player::get).put(crate::player::put),
        )
        .route("/v1/players/appearances/query", post(crate::player::query))
        .route(
            "/v1/entity-worlds/{world_key}",
            get(crate::entity_world::get_world).post(crate::entity_world::create_world),
        )
        .route(
            "/v1/entity-worlds/{world_key}/appearances/query",
            post(crate::entity_world::query),
        )
        .route(
            "/v1/entity-worlds/{world_key}/entities/{entity_uuid}/appearance",
            get(crate::entity_world::get).put(crate::entity_world::put),
        )
        .route(
            "/v1/entity-worlds/{world_key}/entities/{entity_uuid}/asset",
            get(crate::entity_world::asset),
        )
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
        .route(
            "/v1/auth/login-challenges",
            post(crate::game_auth::login_challenge),
        )
        .route(
            "/v1/auth/login-challenges/{challenge_id}/complete",
            post(crate::game_auth::login_complete),
        )
        .route(
            "/v1/auth/challenges",
            post(crate::game_auth::link_challenge),
        )
        .route(
            "/v1/auth/challenges/{challenge_id}/verify",
            post(crate::game_auth::link_complete),
        )
        .route(
            "/v1/auth/challenges/{challenge_id}/complete",
            post(crate::game_auth::link_complete),
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
        .route("/v1/assets", get(crate::catalog::list).post(upload_asset))
        .route(
            "/v1/assets/{asset_id}/visibility",
            put(set_asset_visibility),
        )
        .route("/v1/operations/{operation_id}", get(get_operation))
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
        .route(
            "/v1/targets/{target_id}/animation",
            get(get_animation).put(update_animation),
        )
        .route("/v1/targets/{target_id}/acl", get(list_acl).put(set_acl))
        .with_state(state)
        .layer(axum::middleware::from_fn(api_headers))
}

async fn api_headers(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let mut response = if request.method() == axum::http::Method::OPTIONS {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
    );
    headers.insert("access-control-allow-headers", HeaderValue::from_static("Authorization, Content-Type, Idempotency-Key, Range, If-None-Match, X-Asset-Id, X-Asset-Name, X-Asset-Format, X-Asset-Sha256, X-Asset-Visibility, X-Asset-Metadata-Encoding"));
    response
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "ok": true,
        "instance_id": state.config.instance_id,
        "protocol": PROTOCOL_V1,
        "storage": "sqlite-wal-cas",
        "limits": {
            "max_asset_bytes": state.config.max_asset_bytes,
            "max_message_bytes": state.config.max_message_bytes
        }
    }))
}

async fn instance(State(state): State<AppState>) -> Json<InstanceResponse> {
    Json(InstanceResponse {
        instance_id: state.config.instance_id.clone(),
        origin: state.config.origin.clone(),
        websocket_origin: websocket_origin(&state.config.origin),
        protocol: PROTOCOL_V1.to_owned(),
        capabilities: vec![PLAYER_MOTION_CAPABILITY.to_owned()],
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
    let token = bearer_token(&headers)?;
    if state
        .config
        .access_token
        .as_deref()
        .is_none_or(|configured| configured.as_bytes().ct_eq(token.as_bytes()).unwrap_u8() != 1)
    {
        return Err(CloudError::AccessDenied);
    }
    Ok(Json(state.store.configure_provider(&input)?))
}

async fn create_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateAccount>,
) -> Result<(StatusCode, Json<AccountSummary>), CloudError> {
    authorize_account_registration(&state, &headers)?;
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .store
                .create_account(&input.account_id, &input.password)?,
        ),
    ))
}

fn authorize_account_registration(state: &AppState, headers: &HeaderMap) -> Result<(), CloudError> {
    if state.config.allow_self_registration {
        return Ok(());
    }
    let account = authenticate(state, headers)?;
    if account == state.config.bootstrap_account_id {
        Ok(())
    } else {
        Err(CloudError::AccessDenied)
    }
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
    let _ = state.events.send(CloudEvent::Appearance {
        event_id: mutation.event_id,
        scope_id: mutation.scope_id,
        appearance: mutation.appearance.clone(),
    });
    Ok(Json(mutation.appearance))
}

async fn get_animation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target_id): Path<String>,
) -> Result<Json<crate::models::AnimationState>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.get_animation(&account, &target_id)?))
}

async fn update_animation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target_id): Path<String>,
    Json(input): Json<AnimationUpdate>,
) -> Result<Json<crate::models::AnimationState>, CloudError> {
    let account = authenticate(&state, &headers)?;
    let mutation = state.store.update_animation(&account, &target_id, &input)?;
    let _ = state.events.send(CloudEvent::Animation {
        event_id: mutation.event_id,
        scope_id: mutation.scope_id,
        animation: mutation.animation.clone(),
    });
    Ok(Json(mutation.animation))
}

pub(crate) fn is_allowed_provider_address(address: std::net::IpAddr) -> bool {
    match address {
        std::net::IpAddr::V4(ip) => {
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_documentation()
                || ip.octets()[0] == 0
                || ip.octets()[0] >= 240
                || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
                || (ip.octets()[0] == 198 && (18..=19).contains(&ip.octets()[1])))
        }
        std::net::IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_allowed_provider_address(std::net::IpAddr::V4(mapped));
            }
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_multicast())
        }
    }
}

async fn get_operation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(operation_id): Path<String>,
) -> Result<Json<crate::models::UploadOperation>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.upload_operation(&account, &operation_id)?))
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

async fn set_asset_visibility(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset_id): Path<String>,
    Json(input): Json<AssetVisibilityUpdate>,
) -> Result<Json<crate::models::AssetSummary>, CloudError> {
    let account = authenticate(&state, &headers)?;
    Ok(Json(state.store.set_asset_visibility(
        &account,
        &asset_id,
        &input.visibility,
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
) -> Result<Response, CloudError> {
    let account = authenticate(&state, &headers)?;
    let request_id = header_string(&headers, "idempotency-key")?
        .ok_or_else(|| CloudError::invalid_metadata("Idempotency-Key is required"))?;
    let asset_id = asset_metadata_header(&headers, "x-asset-id")?
        .unwrap_or_else(|| format!("asset_{}", Uuid::new_v4().simple()));
    let name = asset_metadata_header(&headers, "x-asset-name")?.unwrap_or_else(|| asset_id.clone());
    let format = header_string(&headers, "x-asset-format")?
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    let expected_sha = header_string(&headers, "x-asset-sha256")?;
    let requested_visibility = header_string(&headers, "x-asset-visibility")?
        .map(|value| value.trim().to_uppercase())
        .filter(|value| !value.is_empty());
    if requested_visibility
        .as_deref()
        .is_some_and(|value| !matches!(value, "PRIVATE" | "PUBLIC"))
    {
        return Err(CloudError::invalid_metadata(
            "visibility must be PRIVATE or PUBLIC",
        ));
    }
    let content_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let operation_id = state
        .store
        .begin_upload_operation(&account, &request_id, &asset_id)?;
    let operation_header = HeaderValue::from_str(&operation_id)
        .map_err(|_| CloudError::invalid_metadata("invalid upload operation id"))?;
    if content_length.is_some_and(|length| length > state.config.max_asset_bytes) {
        state
            .store
            .fail_upload_operation(&operation_id, "ASSET_TOO_LARGE")?;
        return Err(CloudError::AssetTooLarge);
    }
    let temp_path = state
        .config
        .object_dir
        .join(format!(".upload-{}", Uuid::new_v4().simple()));
    state.store.create_upload_lease(
        &operation_id,
        &account,
        &temp_path,
        crate::api::now_unix_ms() / 1000 + 15 * 60,
    )?;
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
            let _ = state
                .store
                .fail_upload_operation(&operation_id, "ASSET_TOO_LARGE");
            return Err(CloudError::AssetTooLarge);
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    if total == 0 {
        let _ = fs::remove_file(&temp_path).await;
        let _ = state
            .store
            .fail_upload_operation(&operation_id, "INVALID_METADATA");
        return Err(CloudError::invalid_metadata("asset must not be empty"));
    }
    let sha256 = hex::encode(hasher.finalize());
    if expected_sha
        .as_deref()
        .is_some_and(|expected| !expected.eq_ignore_ascii_case(&sha256))
    {
        let _ = fs::remove_file(&temp_path).await;
        let _ = state
            .store
            .fail_upload_operation(&operation_id, "ASSET_HASH_MISMATCH");
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
            requested_visibility.as_deref(),
        ))
        .map_err(|_| CloudError::invalid_metadata("invalid upload metadata"))?,
    ));
    let summary = match state.store.register_asset_with_visibility(
        &account,
        &asset_id,
        &name,
        &format,
        &sha256,
        total,
        &object_path,
        Some((&request_id, &request_hash)),
        requested_visibility.as_deref(),
    ) {
        Ok(summary) => summary,
        Err(error) => {
            if !object_already_exists {
                let _ = fs::remove_file(&object_path).await;
            }
            let _ = state
                .store
                .fail_upload_operation(&operation_id, error.code());
            return Err(error);
        }
    };
    state
        .store
        .complete_upload_operation(&operation_id, &summary)?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::HeaderName::from_static("x-operation-id"),
        operation_header,
    );
    Ok((StatusCode::CREATED, response_headers, Json(summary)).into_response())
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

/// Metadata is encoded only when explicitly marked, preserving literal percent/plus in legacy requests.
fn asset_metadata_header(headers: &HeaderMap, name: &str) -> Result<Option<String>, CloudError> {
    let encoding = header_string(headers, "x-asset-metadata-encoding")?;
    match encoding.as_deref() {
        None => return header_string(headers, name),
        Some("utf-8-percent") => {}
        _ => {
            return Err(CloudError::invalid_metadata(
                "asset metadata encoding is unsupported",
            ));
        }
    }
    header_string(headers, name)?
        .map(|value| {
            let bytes = value.as_bytes();
            let mut decoded = Vec::with_capacity(bytes.len());
            let mut index = 0;
            while index < bytes.len() {
                if bytes[index] == b'%' {
                    let digits = bytes.get(index + 1..index + 3).ok_or_else(|| {
                        CloudError::invalid_metadata(format!("invalid percent encoding in {name}"))
                    })?;
                    let hex_digit = |byte: u8| (byte as char).to_digit(16);
                    let high = hex_digit(digits[0]);
                    let low = hex_digit(digits[1]);
                    let (Some(high), Some(low)) = (high, low) else {
                        return Err(CloudError::invalid_metadata(format!(
                            "invalid percent encoding in {name}"
                        )));
                    };
                    decoded.push(((high << 4) | low) as u8);
                    index += 3;
                } else {
                    decoded.push(bytes[index]);
                    index += 1;
                }
            }
            let value = String::from_utf8(decoded)
                .map_err(|_| CloudError::invalid_metadata(format!("invalid UTF-8 in {name}")))?;
            if value.trim().is_empty() || value.chars().any(char::is_control) {
                return Err(CloudError::invalid_metadata(format!(
                    "{name} must be a non-empty single-line value"
                )));
            }
            Ok(value)
        })
        .transpose()
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::store::CloudStore;
    use tempfile::TempDir;

    #[tokio::test]
    async fn instance_advertises_player_motion_support() {
        let (state, _dir) = test_state(false);
        let response = serde_json::to_value(instance(State(state)).await.0).unwrap();
        assert_eq!(
            response["capabilities"],
            serde_json::json!(["player_motion_v1"])
        );
    }

    #[test]
    fn official_editor_role_can_create_targets_and_edit_granted_appearances() {
        let (state, _dir) = test_state(false);
        let store = &state.store;
        store
            .create_account("editor_account", "editor-test-password")
            .unwrap();
        store
            .create_scope(
                "account_local",
                &CreateScope {
                    scope_id: "editor_scope".into(),
                    name: "Editors".into(),
                    world_epoch: "epoch".into(),
                    offline_policy: None,
                },
            )
            .unwrap();
        let target = CreateTarget {
            scope_id: "editor_scope".into(),
            target_id: Some("owner_target".into()),
            kind: crate::models::TargetKind::Dummy,
            display_name: "Dummy".into(),
        };
        store.create_target("account_local", &target).unwrap();
        store
            .set_scope_acl(
                "account_local",
                "editor_scope",
                &ScopeAclUpdate {
                    account_id: "editor_account".into(),
                    role: "editor".into(),
                },
            )
            .unwrap();
        store
            .create_target(
                "editor_account",
                &CreateTarget {
                    target_id: Some("editor_target".into()),
                    ..target
                },
            )
            .unwrap();
        store
            .set_acl(
                "account_local",
                "owner_target",
                &AclUpdate {
                    account_id: "editor_account".into(),
                    role: "editor".into(),
                },
            )
            .unwrap();
        let update:AppearanceUpdate=serde_json::from_value(serde_json::json!({"request_id":"editor_update","expected_revision":0,"disabled":false})).unwrap();
        assert_eq!(
            store
                .update_appearance("editor_account", "owner_target", &update)
                .unwrap()
                .appearance
                .revision,
            1
        );
    }

    pub(crate) fn test_state(allow_self_registration: bool) -> (AppState, TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let config = Arc::new(CloudConfig {
            instance_id: "registration-test".into(),
            origin: "https://localhost".into(),
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            database_path: directory.path().join("cloud.db"),
            object_dir: directory.path().join("objects"),
            access_token: Some("bootstrap-secret".into()),
            bootstrap_account_id: "account_local".into(),
            bootstrap_password_hash: None,
            allow_self_registration,
            max_asset_bytes: 128 * 1024 * 1024,
            max_message_bytes: 64 * 1024,
            trusted_proxy_ips: Vec::new(),
        });
        let store = CloudStore::open(&config).unwrap();
        let (events, _) = broadcast::channel(8);
        (
            AppState {
                config,
                store,
                events,
            },
            directory,
        )
    }

    #[tokio::test]
    async fn websocket_hello_heartbeat_and_revoked_session_close_without_panicking() {
        use crate::protocol::generated::{Envelope, Heartbeat, Hello};
        use futures_util::SinkExt;
        use prost::Message as _;
        use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
        let (state, _dir) = test_state(false);
        state
            .store
            .create_account("ws_player", "ws-test-password")
            .unwrap();
        let session = state
            .store
            .issue_session(&LoginRequest {
                account_id: "ws_player".into(),
                password: "ws-test-password".into(),
            })
            .unwrap();
        let store = state.store.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = router(state.clone()).merge(crate::realtime::router(state));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut request = format!("ws://{address}/v1/realtime")
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {}", session.access_token)).unwrap(),
        );
        let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let hello = Hello {
            instance_id: "registration-test".into(),
            protocol_versions: vec![PROTOCOL_V1.into()],
            ..Default::default()
        };
        let envelope = Envelope {
            protocol_version: PROTOCOL_V1.into(),
            kind: "Hello".into(),
            request_id: "hello".into(),
            payload: hello.encode_to_vec().into(),
            ..Default::default()
        };
        ws.send(Message::Binary(envelope.encode_to_vec().into()))
            .await
            .unwrap();
        let received = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Binary(bytes) = received else {
            panic!("expected binary hello ack")
        };
        assert_eq!(Envelope::decode(bytes).unwrap().kind, "HelloAck");
        let heartbeat = Envelope {
            kind: "Heartbeat".into(),
            payload: Heartbeat::default().encode_to_vec().into(),
            ..envelope
        };
        ws.send(Message::Binary(heartbeat.encode_to_vec().into()))
            .await
            .unwrap();
        let received = ws.next().await.unwrap().unwrap();
        let Message::Binary(bytes) = received else {
            panic!("expected heartbeat ack")
        };
        assert_eq!(Envelope::decode(bytes).unwrap().kind, "HeartbeatAck");
        store.revoke_access_token(&session.access_token).unwrap();
        ws.send(Message::Binary(heartbeat.encode_to_vec().into()))
            .await
            .unwrap();
        let received = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next()).await;
        assert!(matches!(
            received,
            Ok(None) | Ok(Some(Err(_))) | Ok(Some(Ok(Message::Close(_))))
        ));
        server.abort();
    }

    #[tokio::test]
    async fn public_catalog_has_cursor_pages_latest_revisions_and_search_requirement() {
        let (state, directory) = test_state(false);
        let path = directory.path().join("asset");
        std::fs::write(&path, b"model").unwrap();
        for id in ["a", "b"] {
            state
                .store
                .register_asset("account_local", id, "芙宁娜", "ysm", "sha", 5, &path)
                .unwrap();
            state
                .store
                .set_asset_visibility("account_local", id, "PUBLIC")
                .unwrap();
        }
        state
            .store
            .register_asset("account_local", "a", "芙宁娜更新", "ysm", "sha2", 5, &path)
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server =
            tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
        let client = reqwest::Client::new();
        let base = format!("http://{address}/v1/assets");
        assert_eq!(
            client
                .get(format!("{base}?scope=public"))
                .bearer_auth("bootstrap-secret")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        let first: serde_json::Value = client
            .get(&base)
            .query(&[("scope", "public"), ("q", "芙宁娜"), ("limit", "1")])
            .bearer_auth("bootstrap-secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(first["entries"][0]["revision"], 2);
        assert_eq!(first["next_cursor"], "a");
        assert_eq!(first["has_more"], true);
        let second: serde_json::Value = client
            .get(&base)
            .query(&[
                ("scope", "public"),
                ("q", "芙宁娜"),
                ("limit", "1"),
                ("after", "a"),
            ])
            .bearer_auth("bootstrap-secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(second["entries"][0]["asset_id"], "b");
        assert_eq!(second["has_more"], false);
        server.abort();
    }

    #[tokio::test]
    async fn self_host_exposes_anonymous_game_login_and_domain_bound_proof() {
        let (state, _directory) = test_state(false);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server =
            tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
        let client = reqwest::Client::new();
        let response = client.post(format!("http://{address}/v1/auth/login-challenges"))
            .json(&serde_json::json!({"provider_id":"official","username":"Player","profile_uuid":"00000000-0000-0000-0000-000000000001"})).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let challenge: serde_json::Value = response.json().await.unwrap();
        assert!(
            challenge["profile_key_payload"]
                .as_str()
                .unwrap()
                .starts_with("SPM-CLOUD-GAME-IDENTITY-V1\nhttps://localhost\nlogin\n\nofficial\n")
        );
        let id = challenge["challenge_id"].as_str().unwrap();
        let url = format!("http://{address}/v1/auth/login-challenges/{id}/complete");
        let body = serde_json::json!({"challenge_id":id,"profile_key":{}});
        let first = client.post(&url).json(&body).send().await.unwrap();
        assert_eq!(first.status(), StatusCode::FORBIDDEN);
        let second: serde_json::Value = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(second["code"], "IDENTITY_CHALLENGE_REPLAYED");
        server.abort();
    }

    fn registration_input() -> CreateAccount {
        CreateAccount {
            account_id: "player_one".into(),
            password: "test-password-123".into(),
        }
    }

    #[tokio::test]
    async fn upload_preserves_encoded_unicode_metadata_and_original_bytes() {
        let (state, _directory) = test_state(false);
        let content = b"\x00\xffmodel\r\nbytes";
        let sha = hex::encode(Sha256::digest(content));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer bootstrap-secret"),
        );
        headers.insert(
            "idempotency-key",
            HeaderValue::from_static("unicode-upload"),
        );
        headers.insert(
            "x-asset-metadata-encoding",
            HeaderValue::from_static("utf-8-percent"),
        );
        headers.insert(
            "x-asset-id",
            HeaderValue::from_static("%E6%A8%A1%E5%9E%8B%20100%25%2Btest"),
        );
        headers.insert(
            "x-asset-name",
            HeaderValue::from_static(
                "%E8%8A%99%E5%AE%81%E5%A8%9Cv3.14%E6%97%A5%E8%AF%AD%E9%85%8D%E9%9F%B3.ysm",
            ),
        );
        headers.insert("x-asset-sha256", HeaderValue::from_str(&sha).unwrap());
        let response = upload_asset(
            State(state.clone()),
            headers.clone(),
            Body::from(content.to_vec()),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024)
            .await
            .unwrap();
        let summary: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(summary["asset_id"], "模型 100%+test");
        assert_eq!(summary["name"], "芙宁娜v3.14日语配音.ysm");
        assert_eq!(summary["raw_sha256"], sha);
        assert_eq!(summary["byte_length"], content.len());
        assert_eq!(
            fs::read(state.store.object_path_for_sha(&sha))
                .await
                .unwrap(),
            content
        );
        let download = download_asset(
            State(state),
            headers,
            Path(("模型 100%+test".to_owned(), 1)),
        )
        .await
        .unwrap();
        assert_eq!(download.status(), StatusCode::OK);
        assert_eq!(
            axum::body::to_bytes(download.into_body(), 16 * 1024)
                .await
                .unwrap()
                .as_ref(),
            content
        );
    }

    #[test]
    fn upload_metadata_decoding_is_opt_in_and_rejects_malformed_values() {
        let mut headers = HeaderMap::new();
        headers.insert("x-asset-id", HeaderValue::from_static("literal%20+id"));
        assert_eq!(
            asset_metadata_header(&headers, "x-asset-id")
                .unwrap()
                .as_deref(),
            Some("literal%20+id")
        );
        headers.insert(
            "x-asset-metadata-encoding",
            HeaderValue::from_static("utf-8-percent"),
        );
        headers.insert("x-asset-id", HeaderValue::from_static("literal%2520%2Bid"));
        assert_eq!(
            asset_metadata_header(&headers, "x-asset-id")
                .unwrap()
                .as_deref(),
            Some("literal%20+id")
        );
        for value in [
            "bad%",
            "bad%GG",
            "bad%FF",
            "bad%0Aname",
            "bad%00name",
            "%20",
        ] {
            headers.insert("x-asset-id", HeaderValue::from_str(value).unwrap());
            assert!(matches!(
                asset_metadata_header(&headers, "x-asset-id"),
                Err(CloudError::InvalidMetadata(_))
            ));
        }
        headers.insert(
            "x-asset-metadata-encoding",
            HeaderValue::from_static("unsupported"),
        );
        assert!(matches!(
            asset_metadata_header(&headers, "x-asset-id"),
            Err(CloudError::InvalidMetadata(_))
        ));
    }

    #[tokio::test]
    async fn identical_bytes_can_be_reuploaded_and_reused_by_another_asset() {
        let (state, _directory) = test_state(false);
        let bytes = b"identical-model-original";
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer bootstrap-secret"),
        );
        headers.insert("x-asset-id", HeaderValue::from_static("repeat-model"));
        headers.insert("x-asset-name", HeaderValue::from_static("repeat-model.ysm"));
        for (index, visibility) in ["PRIVATE", "PUBLIC", "PRIVATE"].into_iter().enumerate() {
            headers.insert(
                "idempotency-key",
                HeaderValue::from_str(&format!("repeat-{index}")).unwrap(),
            );
            headers.insert(
                "x-asset-visibility",
                HeaderValue::from_str(visibility).unwrap(),
            );
            let result = upload_asset(
                State(state.clone()),
                headers.clone(),
                Body::from(bytes.to_vec()),
            )
            .await;
            assert!(
                result.is_ok(),
                "reusing original bytes must not violate SHA uniqueness: {result:?}"
            );
            let json: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(result.unwrap().into_body(), 16 * 1024)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(json["revision"], index + 1);
            if index > 0 {
                assert_eq!(json["visibility"], visibility);
            }
        }
        headers.insert(
            "idempotency-key",
            HeaderValue::from_static("same-bytes-different-asset"),
        );
        headers.insert("x-asset-id", HeaderValue::from_static("another-asset"));
        let copied = upload_asset(
            State(state.clone()),
            headers.clone(),
            Body::from(bytes.to_vec()),
        )
        .await;
        assert!(copied.is_ok());
        assert_eq!(
            fs::read(
                state
                    .store
                    .object_path_for_sha(&hex::encode(Sha256::digest(bytes)))
            )
            .await
            .unwrap(),
            bytes
        );
        headers.insert("idempotency-key", HeaderValue::from_static("repeat-0"));
        headers.insert("x-asset-id", HeaderValue::from_static("repeat-model"));
        headers.insert("x-asset-visibility", HeaderValue::from_static("PUBLIC"));
        assert!(matches!(
            upload_asset(State(state), headers, Body::from(bytes.to_vec())).await,
            Err(CloudError::IdempotencyConflict)
        ));
    }

    #[tokio::test]
    async fn owner_can_change_visibility_without_revisions_or_blob_writes() {
        let (state, _directory) = test_state(false);
        let bytes = b"visibility-original";
        let sha = hex::encode(Sha256::digest(bytes));
        let object = state.store.object_path_for_sha(&sha);
        fs::create_dir_all(object.parent().unwrap()).await.unwrap();
        fs::write(&object, bytes).await.unwrap();
        let id = "模型%+case";
        let original = state
            .store
            .register_asset(
                "account_local",
                id,
                "name.ysm",
                "ysm",
                &sha,
                bytes.len() as u64,
                &object,
            )
            .unwrap();
        let modified = fs::metadata(&object).await.unwrap().modified().unwrap();
        state
            .store
            .create_account("observer", "test-password-123")
            .unwrap();
        state
            .store
            .set_asset_acl(
                "account_local",
                id,
                &AssetAclUpdate {
                    account_id: "observer".into(),
                    permission: "manage".into(),
                },
            )
            .unwrap();
        let observer = state
            .store
            .issue_session(&LoginRequest {
                account_id: "observer".into(),
                password: "test-password-123".into(),
            })
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = router(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let endpoint =
            format!("http://{address}/v1/assets/%E6%A8%A1%E5%9E%8B%25%2Bcase/visibility");
        let client = reqwest::Client::new();
        for visibility in ["PUBLIC", "PUBLIC", "PRIVATE", "PRIVATE"] {
            let response = client
                .put(&endpoint)
                .bearer_auth("bootstrap-secret")
                .json(&serde_json::json!({"visibility":visibility}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            let summary: serde_json::Value = response.json().await.unwrap();
            assert_eq!(summary["visibility"], visibility);
            assert_eq!(summary["asset_id"], id);
            assert_eq!(summary["revision"], original.revision);
            assert_eq!(summary["raw_sha256"], sha);
            assert_eq!(
                fs::metadata(&object).await.unwrap().modified().unwrap(),
                modified
            );
            assert_eq!(state.store.list_assets("account_local").unwrap().len(), 1);
            assert_eq!(
                state.store.asset_content("no_acl_account", id, 1).is_ok(),
                visibility == "PUBLIC"
            );
        }
        let denied = client
            .put(&endpoint)
            .bearer_auth(&observer.access_token)
            .json(&serde_json::json!({"visibility":"PUBLIC"}))
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);
        let invalid = client
            .put(&endpoint)
            .bearer_auth("bootstrap-secret")
            .json(&serde_json::json!({"visibility":"UNLISTED"}))
            .send()
            .await
            .unwrap();
        assert_eq!(invalid.status(), reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(fs::read(&object).await.unwrap(), bytes);
        server.abort();
    }

    #[tokio::test]
    async fn self_registration_requires_opt_in_but_bootstrap_creation_still_works() {
        let (disabled, _disabled_directory) = test_state(false);
        let denied = create_account(
            State(disabled.clone()),
            HeaderMap::new(),
            Json(registration_input()),
        )
        .await;
        assert!(matches!(denied, Err(CloudError::Unauthenticated)));

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer bootstrap-secret"),
        );
        let admin_created = create_account(State(disabled), headers, Json(registration_input()))
            .await
            .unwrap();
        assert_eq!(admin_created.0, StatusCode::CREATED);

        let (enabled, _enabled_directory) = test_state(true);
        let public_created =
            create_account(State(enabled), HeaderMap::new(), Json(registration_input()))
                .await
                .unwrap();
        assert_eq!(public_created.0, StatusCode::CREATED);
        assert_eq!(public_created.1.account_id, "player_one");
    }
}
