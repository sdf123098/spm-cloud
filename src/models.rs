use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct InstanceResponse {
    pub instance_id: String,
    pub origin: String,
    pub websocket_origin: String,
    pub protocol: String,
    pub limits: Limits,
}

#[derive(Clone, Debug, Serialize)]
pub struct Limits {
    pub max_message_bytes: usize,
    pub max_snapshot_bytes: usize,
    pub max_snapshot_chunk_bytes: usize,
    pub max_asset_bytes: u64,
    pub max_subscriptions: usize,
    pub heartbeat_interval_seconds: u64,
    pub heartbeat_ttl_seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AssetSummary {
    pub asset_id: String,
    pub revision: u64,
    pub name: String,
    pub format: String,
    pub raw_sha256: String,
    pub byte_length: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AssetAclEntry {
    pub account_id: String,
    pub permission: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AssetAclUpdate {
    pub account_id: String,
    pub permission: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateScope {
    pub scope_id: String,
    pub name: String,
    pub world_epoch: String,
    #[serde(default)]
    pub offline_policy: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScopeSummary {
    pub scope_id: String,
    pub tenant_id: String,
    pub name: String,
    pub world_epoch: String,
    pub offline_policy: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScopeAclEntry {
    pub account_id: String,
    pub role: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ScopeAclUpdate {
    pub account_id: String,
    pub role: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountSummary {
    pub account_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateAccount {
    pub account_id: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LoginRequest {
    pub account_id: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_in_seconds: u64,
    pub refresh_expires_in_seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct IdentitySummary {
    pub identity_id: String,
    pub account_id: String,
    pub identity: String,
    pub display_name: String,
    pub verification_status: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateIdentity {
    pub identity: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateIdentityChallenge {
    pub provider_id: String,
    pub username: String,
    pub profile_uuid: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IdentityChallengeResponse {
    pub challenge_id: String,
    pub provider_id: String,
    pub server_id: String,
    pub expires_in_seconds: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VerifyIdentityChallenge {
    pub challenge_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct IdentityProviderUpdate {
    pub provider_id: String,
    pub display_name: String,
    pub base_url: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OfflineBindingRequest {
    pub scope_id: String,
    pub world_epoch: String,
    pub identity_id: String,
    pub target_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScopedIdentityBindingSummary {
    pub binding_id: String,
    pub account_id: String,
    pub identity_id: String,
    pub target_id: String,
    pub scope_id: String,
    pub world_epoch: String,
    pub entity_uuid: String,
    pub verification_method: String,
    pub status: String,
    pub approved_by: Option<String>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ClaimCodeRequest {
    pub world_epoch: String,
    pub entity_uuid: String,
    pub expires_in_seconds: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ClaimCodeResponse {
    pub code: String,
    pub scope_id: String,
    pub world_epoch: String,
    pub target_id: String,
    pub entity_uuid: String,
    pub expires_in_seconds: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RedeemClaimCode {
    pub code: String,
    pub identity_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RevokeClaimCode {
    pub code: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuditEntry {
    pub event_id: String,
    pub actor_account_id: String,
    pub action: String,
    pub scope_id: Option<String>,
    pub target_id: Option<String>,
    pub subject_id: Option<String>,
    pub details: serde_json::Value,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OfflineBindingApproval {
    pub status: String,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppearanceUpdate {
    pub request_id: String,
    pub expected_revision: u64,
    pub asset_id: Option<String>,
    pub asset_revision: Option<u64>,
    pub raw_sha256: Option<String>,
    pub texture_id: Option<String>,
    pub scale: Option<f32>,
    pub disabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppearanceState {
    pub target_id: String,
    pub revision: u64,
    pub asset_id: Option<String>,
    pub asset_revision: Option<u64>,
    pub raw_sha256: Option<String>,
    pub texture_id: Option<String>,
    pub scale: Option<f32>,
    pub disabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateTarget {
    pub scope_id: String,
    pub target_id: Option<String>,
    pub kind: TargetKind,
    pub display_name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TargetSummary {
    pub target_id: String,
    pub scope_id: String,
    pub kind: TargetKind,
    pub display_name: String,
    pub revision: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct EntityBindingSummary {
    pub binding_id: String,
    pub scope_id: String,
    pub world_epoch: String,
    pub entity_uuid: String,
    pub entity_kind: String,
    pub target_id: String,
    pub observation_state: String,
    pub last_seen_at: Option<String>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RegisterEntityBinding {
    pub world_epoch: String,
    pub entity_uuid: String,
    pub entity_kind: String,
    pub target_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ObserveEntityBinding {
    pub world_epoch: String,
    pub observation_state: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AclEntry {
    pub account_id: String,
    pub role: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AclUpdate {
    pub account_id: String,
    pub role: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TargetKind { Player, Dummy, Maid }

impl CreateTarget {
    pub fn id(&self) -> String { self.target_id.clone().unwrap_or_else(|| format!("target_{}", Uuid::new_v4().simple())) }
}
