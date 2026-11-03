//! Thumbnail ingestion for the model directory.
//!
//! Rendering lives outside this service (a headless browser runs `web/preview.js`
//! through `tools/render-thumbnails.mjs`), because a 300 MB chromium shared with
//! an API process is an OOM waiting to happen on a small host. What the service
//! owns is the contract around that work: which revisions still need an image,
//! where the bytes are stored, and how a reader learns the state.
//!
//! Storage reuses the existing content-addressed object directory, so the same
//! model uploaded as two assets stores one thumbnail, and a re-render that
//! produces identical bytes writes nothing new.

use crate::{admin::authorize, api::AppState, error::CloudError};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// 8 MiB. A 512×512 PNG of a blocky model is 20–40 KB; anything near this bound
/// is a mistake (a screenshot of the whole page, an uncompressed frame) and
/// accepting it would let one caller fill the object store.
const MAX_THUMBNAIL_BYTES: usize = 8 * 1024 * 1024;

/// PNG magic. Checked before hashing so a mislabelled upload fails at the edge
/// instead of becoming an image the browser refuses to decode.
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/api/thumbnails/pending", get(pending))
        .route(
            "/admin/api/assets/{asset}/revisions/{revision}/thumbnail",
            get(inspect).put(store),
        )
        .route(
            "/admin/api/assets/{asset}/revisions/{revision}/thumbnail/failed",
            post(failed),
        )
}

#[derive(Deserialize)]
struct PendingQuery {
    limit: Option<usize>,
}

/// Work queue for the renderer. Ordered by asset id so a run resumes where the
/// last one stopped and two concurrent runners do not collide on the same rows.
async fn pending(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PendingQuery>,
) -> Result<Json<Value>, CloudError> {
    authorize(&state, &headers, false)?;
    let limit = query.limit.unwrap_or(32).clamp(1, 200);
    let conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let mut statement = conn.prepare(
        "SELECT a.asset_id, a.current_revision, r.name, r.format, r.raw_sha256, r.byte_length, a.visibility
         FROM assets a JOIN asset_revisions r ON r.asset_id = a.asset_id AND r.revision = a.current_revision
         WHERE r.thumbnail_status IS NULL
           AND NOT EXISTS(SELECT 1 FROM deleted_assets d WHERE d.asset_id = a.asset_id)
         ORDER BY a.asset_id LIMIT ?1",
    )?;
    let entries = statement
        .query_map([limit as i64], |row| {
            Ok(json!({
                "asset_id": row.get::<_, String>(0)?,
                "revision": row.get::<_, i64>(1)?,
                "name": row.get::<_, String>(2)?,
                "format": row.get::<_, String>(3)?,
                "raw_sha256": row.get::<_, String>(4)?,
                "byte_length": row.get::<_, i64>(5)?,
                "visibility": row.get::<_, String>(6)?,
                "preview_url": format!(
                    "/admin/api/assets/{}/preview",
                    crate::api::path_segment(&row.get::<_, String>(0)?)
                )
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let remaining: i64 = conn.query_row(
        "SELECT COUNT(*) FROM assets a JOIN asset_revisions r ON r.asset_id = a.asset_id AND r.revision = a.current_revision WHERE r.thumbnail_status IS NULL AND NOT EXISTS(SELECT 1 FROM deleted_assets d WHERE d.asset_id = a.asset_id)",
        [],
        |row| row.get(0),
    )?;
    Ok(Json(json!({ "entries": entries, "remaining": remaining })))
}

/// Upload rendered PNG bytes for one revision.
///
/// Idempotent by content: re-uploading the same image rewrites the same CAS
/// object and leaves the row in the same state, so a renderer that crashes after
/// the PUT but before its bookkeeping can safely retry.
async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((asset, revision)): Path<(String, u64)>,
    body: Body,
) -> Result<Json<Value>, CloudError> {
    let actor = authorize(&state, &headers, false)?.0;
    let bytes = to_bytes(body, MAX_THUMBNAIL_BYTES)
        .await
        .map_err(|_| CloudError::AssetTooLarge)?;
    if bytes.len() < PNG_MAGIC.len() || bytes[..PNG_MAGIC.len()] != PNG_MAGIC {
        return Err(CloudError::invalid_metadata("缩略图必须是 PNG"));
    }
    let sha256 = hex::encode(Sha256::digest(&bytes));
    let path = state.store.object_path_for_sha(&sha256);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    // Write-then-rename so a reader never observes a half-written thumbnail and a
    // concurrent upload of the same bytes cannot interleave.
    let temp = path.with_extension("tmp");
    tokio::fs::write(&temp, &bytes).await?;
    tokio::fs::rename(&temp, &path).await?;
    state.store.record_thumbnail(
        &asset,
        revision,
        &sha256,
        bytes.len() as u64,
        &path.to_string_lossy(),
    )?;
    state.store.admin_audit(
        &actor,
        "MODEL_THUMBNAIL_RENDERED",
        Some(&asset),
        json!({"revision":revision,"sha256":sha256,"byte_length":bytes.len()}),
    )?;
    Ok(Json(
        json!({"asset_id":asset,"revision":revision,"thumbnail_status":"ready","thumbnail_sha256":sha256,"byte_length":bytes.len()}),
    ))
}

/// Record a permanent failure. `failed` is sticky: the batch renderer only picks
/// up `thumbnail_status IS NULL`, so an unsupported format is not retried every
/// night forever, and the client stops asking.
async fn failed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((asset, revision)): Path<(String, u64)>,
) -> Result<StatusCode, CloudError> {
    let actor = authorize(&state, &headers, false)?.0;
    state.store.fail_thumbnail(&asset, revision)?;
    state.store.admin_audit(
        &actor,
        "MODEL_THUMBNAIL_FAILED",
        Some(&asset),
        json!({"revision":revision}),
    )?;
    Ok(StatusCode::NO_CONTENT)
}

/// Serve the stored bytes to a logged-in administrator. The public route is
/// anonymous and CDN-cacheable; this one is for inspection (and for the
/// verification script, which should not need a second credential).
async fn inspect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((asset, revision)): Path<(String, u64)>,
) -> Result<Response, CloudError> {
    authorize(&state, &headers, false)?;
    let (sha256, path) = state.store.thumbnail_record(&asset, revision)?;
    let bytes = tokio::fs::read(&path).await?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE.as_str(), "image/png"),
            ("etag", &format!("\"{sha256}\"")),
        ],
        bytes,
    )
        .into_response())
}
