use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CloudError {
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
            Self::Configuration(_) => "INTERNAL",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::SessionExpired => "SESSION_EXPIRED",
            Self::RefreshReused => "REFRESH_REUSED",
            Self::NotFound => "ASSET_NOT_FOUND",
            Self::AccessDenied => "ASSET_ACCESS_DENIED",
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
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::IdentityProviderUnavailable => StatusCode::BAD_GATEWAY,
            Self::IdentityNotLinked => StatusCode::NOT_FOUND,
            Self::IdentityAlreadyLinked => StatusCode::CONFLICT,
            Self::AccountExists => StatusCode::CONFLICT,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::SearchRequired => StatusCode::BAD_REQUEST,
            Self::SessionExpired | Self::RefreshReused => StatusCode::UNAUTHORIZED,
            Self::AccessDenied | Self::ScopeAccessDenied | Self::ProviderAccessDenied => {
                StatusCode::FORBIDDEN
            }
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
                Self::Internal(_) | Self::Io(_) | Self::Sqlite(_) | Self::Configuration(_) => {
                    "Cloud request failed".to_owned()
                }
                _ => self.to_string(),
            },
        });
        (status, body).into_response()
    }
}
