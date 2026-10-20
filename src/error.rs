use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CloudError {
    #[error("offline synchronization authorization is awaiting administrator approval")]
    OfflineAuthorizationPending,
    #[error("offline synchronization authorization was revoked")]
    OfflineAuthorizationDenied,
    #[error("experimental offline identities are disabled")]
    OfflineIdentityDisabled,
    #[error("offline key is invalid for this account or instance")]
    OfflineKeyInvalid,
    #[error("offline key has been revoked")]
    OfflineKeyRevoked,
    #[error("offline publication session expired")]
    OfflineSessionExpired,
    #[error("offline binding was replaced; activate again explicitly")]
    OfflineBindingStale,
    #[error("offline identity quota exceeded")]
    OfflineIdentityQuota,
    #[error("configuration validation failed")]
    SettingsValidation(Vec<serde_json::Value>),
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("session expired")]
    SessionExpired,
    #[error("refresh token reused")]
    RefreshReused,
    #[error("not found")]
    NotFound,
    #[error("access denied")]
    AccessDenied,
    #[error("R18 model blocked by Cloud policy")]
    R18Blocked,
    #[error("Model could not be classified; blocked by Cloud policy")]
    ModerationUnclassified,
    #[error("scope access denied")]
    ScopeAccessDenied,
    #[error("provider configuration access denied")]
    ProviderAccessDenied,
    #[error("account already exists")]
    AccountExists,
    #[error("invalid metadata: {0}")]
    InvalidMetadata(String),
    #[error("asset too large")]
    AssetTooLarge,
    #[error("asset hash mismatch")]
    AssetHashMismatch,
    #[error("invalid range")]
    AssetRangeInvalid,
    #[error("revision conflict")]
    RevisionConflict,
    #[error("idempotency conflict")]
    IdempotencyConflict,
    #[error("message too large")]
    MessageTooLarge,
    #[error("protocol unsupported")]
    ProtocolUnsupported,
    #[error("identity is not verified")]
    IdentityNotVerified,
    #[error("identity challenge expired")]
    IdentityChallengeExpired,
    #[error("identity challenge was already consumed")]
    IdentityChallengeReplayed,
    #[error("identity provider is not trusted")]
    IdentityProviderUntrusted,
    #[error("identity profile mismatch")]
    IdentityProfileMismatch,
    #[error("identity provider is unavailable")]
    IdentityProviderUnavailable,
    #[error("game identity is not linked to one Cloud account")]
    IdentityNotLinked,
    #[error("game identity is already linked to another Cloud account")]
    IdentityAlreadyLinked,
    #[error("too many game identity challenges")]
    RateLimited,
    #[error("public asset discovery requires a search query")]
    SearchRequired,
    #[error("internal error")]
    Internal(#[source] anyhow::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

impl CloudError {
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration(message.into())
    }
    pub fn invalid_metadata(message: impl Into<String>) -> Self {
        Self::InvalidMetadata(message.into())
    }
    pub fn code(&self) -> &'static str {
        match self {
            Self::OfflineAuthorizationPending => "OFFLINE_AUTHORIZATION_PENDING",
            Self::OfflineAuthorizationDenied => "OFFLINE_AUTHORIZATION_DENIED",
            Self::OfflineIdentityDisabled => "OFFLINE_IDENTITY_DISABLED",
            Self::OfflineKeyInvalid => "OFFLINE_KEY_INVALID",
            Self::OfflineKeyRevoked => "OFFLINE_KEY_REVOKED",
            Self::OfflineSessionExpired => "OFFLINE_SESSION_EXPIRED",
            Self::OfflineBindingStale => "OFFLINE_BINDING_STALE",
            Self::OfflineIdentityQuota => "OFFLINE_IDENTITY_QUOTA",
            Self::SettingsValidation(_) => "INVALID_METADATA",
            Self::Configuration(_) => "INTERNAL",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::SessionExpired => "SESSION_EXPIRED",
            Self::RefreshReused => "REFRESH_REUSED",
            Self::NotFound => "ASSET_NOT_FOUND",
            Self::AccessDenied => "ASSET_ACCESS_DENIED",
            Self::R18Blocked => "R18_MODEL_BLOCKED",
            Self::ModerationUnclassified => "MODEL_UNCLASSIFIED",
            Self::ScopeAccessDenied => "SCOPE_ACCESS_DENIED",
            Self::ProviderAccessDenied => "ACCESS_DENIED",
            Self::AccountExists => "ACCOUNT_EXISTS",
            Self::InvalidMetadata(_) => "INVALID_METADATA",
            Self::AssetTooLarge => "ASSET_TOO_LARGE",
            Self::AssetHashMismatch => "ASSET_HASH_MISMATCH",
            Self::AssetRangeInvalid => "ASSET_RANGE_INVALID",
            Self::RevisionConflict => "REVISION_CONFLICT",
            Self::IdempotencyConflict => "IDEMPOTENCY_CONFLICT",
            Self::MessageTooLarge => "MESSAGE_TOO_LARGE",
            Self::ProtocolUnsupported => "PROTOCOL_UNSUPPORTED",
            Self::IdentityNotVerified => "IDENTITY_PROFILE_MISMATCH",
            Self::IdentityChallengeExpired => "IDENTITY_CHALLENGE_EXPIRED",
            Self::IdentityChallengeReplayed => "IDENTITY_CHALLENGE_REPLAYED",
            Self::IdentityProviderUntrusted => "IDENTITY_PROVIDER_UNTRUSTED",
            Self::IdentityProfileMismatch => "IDENTITY_PROFILE_MISMATCH",
            Self::IdentityProviderUnavailable => "IDENTITY_PROVIDER_UNAVAILABLE",
            Self::IdentityNotLinked => "IDENTITY_NOT_LINKED",
            Self::IdentityAlreadyLinked => "IDENTITY_ALREADY_LINKED",
            Self::RateLimited => "RATE_LIMITED",
            Self::SearchRequired => "SEARCH_REQUIRED",
            Self::Internal(_) | Self::Io(_) | Self::Sqlite(_) => "INTERNAL",
        }
    }
    pub fn status(&self) -> StatusCode {
        match self {
            Self::OfflineAuthorizationPending
            | Self::OfflineAuthorizationDenied
            | Self::OfflineIdentityDisabled
            | Self::OfflineKeyInvalid
            | Self::OfflineKeyRevoked
            | Self::OfflineSessionExpired => StatusCode::FORBIDDEN,
            Self::OfflineBindingStale => StatusCode::CONFLICT,
            Self::OfflineIdentityQuota => StatusCode::TOO_MANY_REQUESTS,
            Self::SettingsValidation(_) => StatusCode::BAD_REQUEST,
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::IdentityProviderUnavailable => StatusCode::BAD_GATEWAY,
            Self::IdentityNotLinked => StatusCode::NOT_FOUND,
            Self::IdentityAlreadyLinked => StatusCode::CONFLICT,
            Self::AccountExists => StatusCode::CONFLICT,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::SearchRequired => StatusCode::BAD_REQUEST,
            Self::SessionExpired | Self::RefreshReused => StatusCode::UNAUTHORIZED,
            Self::AccessDenied
            | Self::ScopeAccessDenied
            | Self::ProviderAccessDenied
            | Self::R18Blocked
            | Self::ModerationUnclassified => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::AssetRangeInvalid => StatusCode::RANGE_NOT_SATISFIABLE,
            Self::AssetHashMismatch => StatusCode::UNPROCESSABLE_ENTITY,
            Self::RevisionConflict | Self::IdempotencyConflict => StatusCode::CONFLICT,
            Self::AssetTooLarge | Self::MessageTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::InvalidMetadata(_) | Self::ProtocolUnsupported => StatusCode::BAD_REQUEST,
            Self::IdentityNotVerified => StatusCode::FORBIDDEN,
            Self::IdentityChallengeExpired | Self::IdentityChallengeReplayed => {
                StatusCode::UNAUTHORIZED
            }
            Self::IdentityProviderUntrusted | Self::IdentityProfileMismatch => {
                StatusCode::FORBIDDEN
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    ok: bool,
    code: &'a str,
    retryable: bool,
    message_key: String,
    message: String,
}

impl IntoResponse for CloudError {
    fn into_response(self) -> Response {
        if let Self::SettingsValidation(errors) = &self {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"ok":false,"saved":false,"code":"INVALID_METADATA","retryable":false,"message_key":"cloud.error.invalid_metadata","errors":errors,"message":"Configuration validation failed"}))).into_response();
        }
        let status = self.status();
        let code = self.code();
        let body = Json(ErrorBody {
            ok: false,
            code,
            retryable: matches!(
                self,
                Self::Internal(_)
                    | Self::Io(_)
                    | Self::IdentityProviderUnavailable
                    | Self::RateLimited
            ),
            message_key: format!("cloud.error.{}", code.to_lowercase()),
            message: match &self {
                // io 层错误保留可操作分类: "文件系统只读/权限不足" 之类的信息
                // 决定了运维能否自助解决, 比统一的 "Cloud request failed" 有用得多。
                Self::Internal(err) => match io_error_kind(err) {
                    Some(std::io::ErrorKind::ReadOnlyFilesystem) => {
                        "文件系统只读，当前部署不支持写回程序文件".to_owned()
                    }
                    Some(std::io::ErrorKind::PermissionDenied) => {
                        "权限不足，无法修改程序文件".to_owned()
                    }
                    Some(std::io::ErrorKind::NotFound) => "所需的文件或目录不存在".to_owned(),
                    _ => "Cloud request failed".to_owned(),
                },
                Self::Io(err) => match err.kind() {
                    std::io::ErrorKind::ReadOnlyFilesystem => {
                        "文件系统只读，当前部署不支持写回程序文件".to_owned()
                    }
                    std::io::ErrorKind::PermissionDenied => "权限不足，无法修改程序文件".to_owned(),
                    _ => "Cloud request failed".to_owned(),
                },
                Self::Sqlite(_) | Self::Configuration(_) => "Cloud request failed".to_owned(),
                _ => self.to_string(),
            },
        });
        (status, body).into_response()
    }
}

/// 穿透 anyhow 的错误链找出底层 io::Error 的 kind。
fn io_error_kind(err: &anyhow::Error) -> Option<std::io::ErrorKind> {
    for cause in err.chain() {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            return Some(io.kind());
        }
    }
    None
}
