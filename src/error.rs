use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CloudError {
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("not found")]
    NotFound,
    #[error("access denied")]
    AccessDenied,
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
    #[error("internal error")]
    Internal(#[source] anyhow::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

impl CloudError {
    pub fn configuration(message: impl Into<String>) -> Self { Self::Configuration(message.into()) }
    pub fn invalid_metadata(message: impl Into<String>) -> Self { Self::InvalidMetadata(message.into()) }
    pub fn code(&self) -> &'static str {
        match self {
            Self::Configuration(_) => "INTERNAL",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::NotFound => "ASSET_NOT_FOUND",
            Self::AccessDenied => "ASSET_ACCESS_DENIED",
            Self::InvalidMetadata(_) => "INVALID_METADATA",
            Self::AssetTooLarge => "ASSET_TOO_LARGE",
            Self::AssetHashMismatch => "ASSET_HASH_MISMATCH",
            Self::AssetRangeInvalid => "ASSET_RANGE_INVALID",
            Self::RevisionConflict => "REVISION_CONFLICT",
            Self::IdempotencyConflict => "IDEMPOTENCY_CONFLICT",
            Self::MessageTooLarge => "MESSAGE_TOO_LARGE",
            Self::ProtocolUnsupported => "PROTOCOL_UNSUPPORTED",
            Self::IdentityNotVerified => "IDENTITY_PROFILE_MISMATCH",
            Self::Internal(_) | Self::Io(_) | Self::Sqlite(_) => "INTERNAL",
        }
    }
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::AccessDenied => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::AssetRangeInvalid => StatusCode::RANGE_NOT_SATISFIABLE,
            Self::RevisionConflict | Self::IdempotencyConflict => StatusCode::CONFLICT,
            Self::AssetTooLarge | Self::MessageTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::InvalidMetadata(_) | Self::ProtocolUnsupported => StatusCode::BAD_REQUEST,
            Self::IdentityNotVerified => StatusCode::FORBIDDEN,
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
}

impl IntoResponse for CloudError {
    fn into_response(self) -> Response {
        let status = self.status();
        let code = self.code();
        let body = Json(ErrorBody { ok: false, code, retryable: matches!(self, Self::Internal(_) | Self::Io(_)), message_key: format!("cloud.error.{}", code.to_lowercase()) });
        (status, body).into_response()
    }
}
