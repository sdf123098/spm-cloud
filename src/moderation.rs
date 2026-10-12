//! Content-addressed moderation: recognition and enforcement are independent.
use crate::database::Connection;
use crate::{CloudStore, error::CloudError};
use base64::{Engine, engine::general_purpose::STANDARD};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{io::Read, path::Path};

// Model decoding can temporarily retain decoded JSON, texture bytes, and (for
// encrypted YSM files) a complete parser output tree. Serialize that work so
// burst uploads remain viable on low-memory hosts.
static MODEL_SCAN_SLOTS: std::sync::OnceLock<std::sync::Arc<crate::performance::Limiter>> =
    std::sync::OnceLock::new();

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModerationSettings {
    pub detection_enabled: bool,
    pub block_r18: bool,
    pub texture_detection_enabled: bool,
    pub scanner_url: Option<String>,
    pub ysm_parser_path: Option<std::path::PathBuf>,
    pub score_threshold: f64,
    pub timeout_seconds: u64,
    pub max_textures: usize,
    pub max_texture_bytes: usize,
    pub block_unclassified: bool,
}
impl Default for ModerationSettings {
    fn default() -> Self {
        Self {
            detection_enabled: false,
            block_r18: false,
            texture_detection_enabled: false,
            scanner_url: None,
            ysm_parser_path: None,
            score_threshold: 0.7,
            timeout_seconds: 30,
            max_textures: 64,
            max_texture_bytes: 8 * 1024 * 1024,
            block_unclassified: false,
        }
    }
}
impl ModerationSettings {
    pub fn validate(&self) -> Result<(), CloudError> {
        if !self.score_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.score_threshold)
            || !(1..=300).contains(&self.timeout_seconds)
            || !(1..=256).contains(&self.max_textures)
            || !(1024..=32 * 1024 * 1024).contains(&self.max_texture_bytes)
        {
            return Err(CloudError::configuration(
                "moderation: invalid threshold, timeout or texture budget",
            ));
        }
        if let Some(url) = &self.scanner_url {
            let parsed = reqwest::Url::parse(url)
                .map_err(|_| CloudError::configuration("moderation.scanner_url: invalid URL"))?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.fragment().is_some()
            {
                return Err(CloudError::configuration(
                    "moderation.scanner_url: use HTTP(S) without credentials or fragment",
                ));
            }
        }
        if self.detection_enabled && self.texture_detection_enabled && self.scanner_url.is_none() {
            return Err(CloudError::configuration(
                "moderation.scanner_url is required for texture detection",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanResult {
    pub status: String,
    pub reason: String,
    pub textures_scanned: usize,
    pub score: Option<f64>,
}
impl ScanResult {
    fn unknown(reason: &str) -> Self {
        Self {
            status: "UNKNOWN".into(),
            reason: reason.into(),
            textures_scanned: 0,
            score: None,
        }
    }
}

pub(crate) fn migrate(conn: &Connection) -> Result<(), CloudError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS model_moderation (
        raw_sha256 TEXT PRIMARY KEY, status TEXT NOT NULL, reason TEXT NOT NULL,
        scan_json TEXT NOT NULL, manual_status TEXT, updated_at TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS moderation_policy(id INTEGER PRIMARY KEY CHECK(id=1), block_r18 INTEGER NOT NULL, block_unclassified INTEGER NOT NULL);
        INSERT OR IGNORE INTO moderation_policy VALUES(1,0,0);
        CREATE TABLE IF NOT EXISTS deleted_assets(asset_id TEXT PRIMARY KEY, deleted_by TEXT NOT NULL, deleted_at TEXT NOT NULL);
        CREATE VIEW IF NOT EXISTS available_asset_revisions AS SELECT r.* FROM asset_revisions r
        LEFT JOIN model_moderation m ON m.raw_sha256=r.raw_sha256
        JOIN moderation_policy p ON p.id=1
        WHERE NOT EXISTS(SELECT 1 FROM deleted_assets d WHERE d.asset_id=r.asset_id)
        AND NOT(p.block_r18=1 AND COALESCE(m.manual_status,m.status,'UNKNOWN')='R18')
        AND NOT(p.block_unclassified=1 AND COALESCE(m.manual_status,m.status,'UNKNOWN') NOT IN ('CLEAN','R18')); ")?;
    Ok(())
}
impl CloudStore {
    pub fn set_moderation_policy(&self, settings: &ModerationSettings) -> Result<(), CloudError> {
        self.connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?
            .execute(
                "UPDATE moderation_policy SET block_r18=?1,block_unclassified=?2 WHERE id=1",
                params![settings.block_r18, settings.block_unclassified],
            )?;
        Ok(())
    }
    pub fn save_scan(&self, sha: &str, result: &ScanResult) -> Result<(), CloudError> {
        self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?.execute(
            "INSERT INTO model_moderation(raw_sha256,status,reason,scan_json,updated_at) VALUES(?1,?2,?3,?4,datetime('now'))
            ON CONFLICT(raw_sha256) DO UPDATE SET status=CASE WHEN model_moderation.status='R18' THEN 'R18' WHEN excluded.status IN ('UNKNOWN','DISABLED') AND model_moderation.status='CLEAN' THEN 'CLEAN' ELSE excluded.status END,reason=CASE WHEN model_moderation.status='R18' THEN model_moderation.reason ELSE excluded.reason END,scan_json=excluded.scan_json,updated_at=excluded.updated_at",
            params![sha,result.status,result.reason,serde_json::to_string(result).unwrap()])?;
        Ok(())
    }
    pub fn enforce_content_policy(&self, sha: &str) -> Result<(), CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let status: Option<String> = conn
            .query_row(
                "SELECT COALESCE(manual_status,status) FROM model_moderation WHERE raw_sha256=?1",
                [sha],
                |r| r.get(0),
            )
            .optional()?;
        let (r18, unknown): (bool, bool) = conn.query_row(
            "SELECT block_r18,block_unclassified FROM moderation_policy WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if r18 && status.as_deref() == Some("R18") {
            return Err(CloudError::R18Blocked);
        }
        if unknown && !matches!(status.as_deref(), Some("CLEAN" | "R18")) {
            return Err(CloudError::ModerationUnclassified);
        }
        Ok(())
    }
}

fn r18_metadata(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, v)| {
            let key = key.to_ascii_lowercase();
            (matches!(key.as_str(), "r18" | "r_18" | "nsfw" | "adult" | "is_r18")
                && (v == &Value::Bool(true) || v.as_u64() == Some(1)))
                || (matches!(
                    key.as_str(),
                    "rating" | "content_rating" | "age_rating" | "tags" | "tag"
                ) && text_r18(&v.to_string()))
                || r18_metadata(v)
        }),
        Value::Array(values) => values.iter().any(r18_metadata),
        _ => false,
    }
}
fn text_r18(text: &str) -> bool {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '+' || c == '-'))
        .any(|word| {
            matches!(
                word,
                "r18" | "r-18" | "18+" | "nsfw" | "adult" | "成人" | "色情"
            )
        })
}

struct Inputs {
    images: Vec<Vec<u8>>,
    tagged: bool,
    complete: bool,
}
fn extract(path: &Path, settings: &ModerationSettings) -> anyhow::Result<Inputs> {
    // Never extract ZIP paths to disk or follow model-supplied external URLs.
    let mut prefix = [0; 4];
    if !path.is_dir() {
        std::fs::File::open(path)?.read_exact(&mut prefix)?;
    }
    let mut result = Inputs {
        images: vec![],
        tagged: false,
        complete: true,
    };
    let max_total = (settings.max_texture_bytes * settings.max_textures).min(64 * 1024 * 1024);
    let mut total = 0usize;
    fn add_json(
        bytes: &[u8],
        binary: Option<&[u8]>,
        result: &mut Inputs,
        settings: &ModerationSettings,
        total: &mut usize,
        max_total: usize,
    ) -> anyhow::Result<()> {
        let value: Value = serde_json::from_slice(bytes)?;
        result.tagged |= r18_metadata(&value);
        if result.tagged {
            return Ok(());
        }
        fn visit(
            value: &Value,
            result: &mut Inputs,
            settings: &ModerationSettings,
            total: &mut usize,
            max_total: usize,
        ) -> anyhow::Result<()> {
            match value {
                Value::String(s) if s.starts_with("data:image/") => {
                    let (kind, data) = s
                        .split_once(',')
                        .ok_or_else(|| anyhow::anyhow!("invalid data URI"))?;
                    anyhow::ensure!(kind.ends_with(";base64"), "unsupported image encoding");
                    anyhow::ensure!(
                        data.len() <= settings.max_texture_bytes.saturating_mul(4) / 3 + 4,
                        "texture budget exceeded"
                    );
                    let bytes = STANDARD.decode(data)?;
                    add_image(bytes, result, settings, total, max_total)?;
                }
                Value::Object(map) => {
                    for (k, v) in map {
                        if matches!(k.as_str(), "uri" | "source")
                            && v.as_str()
                                .is_some_and(|s| !s.is_empty() && !s.starts_with("data:"))
                        {
                            result.complete = false;
                        }
                        visit(v, result, settings, total, max_total)?;
                    }
                }
                Value::Array(values) => {
                    for v in values {
                        visit(v, result, settings, total, max_total)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        visit(&value, result, settings, total, max_total)?;
        if let Some(images) = value.get("images").and_then(Value::as_array) {
            for image in images {
                let Some(index) = image.get("bufferView").and_then(Value::as_u64) else {
                    continue;
                };
                let view = value
                    .get("bufferViews")
                    .and_then(Value::as_array)
                    .and_then(|views| views.get(index as usize))
                    .ok_or_else(|| anyhow::anyhow!("invalid image bufferView"))?;
                let buffer_index = view
                    .get("buffer")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| anyhow::anyhow!("missing image buffer"))?
                    as usize;
                let buffer = value
                    .get("buffers")
                    .and_then(Value::as_array)
                    .and_then(|buffers| buffers.get(buffer_index))
                    .ok_or_else(|| anyhow::anyhow!("missing buffer"))?;
                let owned;
                let bytes = if let Some(uri) = buffer.get("uri").and_then(Value::as_str) {
                    let Some((kind, data)) = uri.split_once(',') else {
                        result.complete = false;
                        continue;
                    };
                    if !kind.starts_with("data:") || !kind.ends_with(";base64") {
                        result.complete = false;
                        continue;
                    }
                    anyhow::ensure!(
                        data.len() <= max_total.saturating_mul(4) / 3 + 4,
                        "embedded buffer budget exceeded"
                    );
                    owned = STANDARD.decode(data)?;
                    owned.as_slice()
                } else if buffer_index == 0 {
                    binary.ok_or_else(|| anyhow::anyhow!("missing GLB binary chunk"))?
                } else {
                    result.complete = false;
                    continue;
                };
                let offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0);
                let length = view
                    .get("byteLength")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| anyhow::anyhow!("missing image length"))?;
                anyhow::ensure!(
                    length <= settings.max_texture_bytes as u64,
                    "texture budget exceeded"
                );
                let end = offset
                    .checked_add(length)
                    .filter(|end| *end <= bytes.len() as u64)
                    .ok_or_else(|| anyhow::anyhow!("invalid image bounds"))?;
                add_image(
                    bytes[offset as usize..end as usize].to_vec(),
                    result,
                    settings,
                    total,
                    max_total,
                )?;
            }
        }
        Ok(())
    }
    fn add_image(
        bytes: Vec<u8>,
        result: &mut Inputs,
        settings: &ModerationSettings,
        total: &mut usize,
        max_total: usize,
    ) -> anyhow::Result<()> {
        *total = total.saturating_add(bytes.len());
        anyhow::ensure!(
            bytes.len() <= settings.max_texture_bytes
                && *total <= max_total
                && result.images.len() < settings.max_textures,
            "texture budget exceeded"
        );
        result.images.push(bytes);
        Ok(())
    }
    if path.is_dir() {
        let mut pending = vec![path.to_owned()];
        let mut entries = 0usize;
        let mut metadata_bytes = 0usize;
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                entries += 1;
                anyhow::ensure!(entries <= 4096, "decoded entry budget exceeded");
                let kind = entry.file_type()?;
                anyhow::ensure!(!kind.is_symlink(), "decoded symlinks are unsupported");
                if kind.is_dir() {
                    pending.push(entry.path());
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_lowercase();
                result.tagged |= text_r18(&name);
                if result.tagged {
                    return Ok(result);
                }
                let image = [
                    ".png", ".jpg", ".jpeg", ".webp", ".avif", ".bmp", ".gif", ".tga",
                ]
                .iter()
                .any(|ext| name.ends_with(ext));
                let metadata = name.ends_with(".json") || name.ends_with(".bbmodel");
                if !image && !metadata {
                    continue;
                }
                let limit = if image {
                    settings.max_texture_bytes
                } else {
                    16 * 1024 * 1024
                };
                anyhow::ensure!(
                    entry.metadata()?.len() <= limit as u64,
                    "decoded file budget exceeded"
                );
                let bytes = std::fs::read(entry.path())?;
                if image {
                    add_image(bytes, &mut result, settings, &mut total, max_total)?;
                } else {
                    metadata_bytes += bytes.len();
                    anyhow::ensure!(
                        metadata_bytes <= 32 * 1024 * 1024,
                        "metadata budget exceeded"
                    );
                    if add_json(&bytes, None, &mut result, settings, &mut total, max_total).is_err()
                    {
                        result.complete = false;
                    }
                    if result.tagged {
                        return Ok(result);
                    }
                }
            }
        }
    } else if prefix == *b"PK\x03\x04" {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
        anyhow::ensure!(archive.len() <= 4096, "archive entry budget exceeded");
        let mut metadata_bytes = 0u64;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_lowercase();
            result.tagged |= text_r18(&name);
            if result.tagged {
                return Ok(result);
            }
            let image = [
                ".png", ".jpg", ".jpeg", ".webp", ".avif", ".bmp", ".gif", ".tga",
            ]
            .iter()
            .any(|ext| name.ends_with(ext));
            let metadata =
                name.ends_with(".json") || name.ends_with(".bbmodel") || name.ends_with(".gltf");
            if !image && !metadata {
                continue;
            }
            let limit = if image {
                settings.max_texture_bytes
            } else {
                16 * 1024 * 1024
            };
            if metadata {
                metadata_bytes = metadata_bytes.saturating_add(entry.size());
                anyhow::ensure!(
                    metadata_bytes <= 32 * 1024 * 1024,
                    "metadata budget exceeded"
                );
            }
            anyhow::ensure!(
                entry.size() <= limit as u64,
                "archive entry budget exceeded"
            );
            let mut bytes = Vec::new();
            (&mut entry)
                .take(limit as u64 + 1)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(bytes.len() <= limit, "archive entry budget exceeded");
            if image {
                add_image(bytes, &mut result, settings, &mut total, max_total)?;
            } else if add_json(&bytes, None, &mut result, settings, &mut total, max_total).is_err()
            {
                result.complete = false;
            }
            if result.tagged {
                return Ok(result);
            }
        }
    } else if prefix == *b"glTF" {
        anyhow::ensure!(
            std::fs::metadata(path)?.len() <= 96 * 1024 * 1024,
            "GLB budget exceeded"
        );
        let bytes = std::fs::read(path)?;
        fn number(bytes: &[u8], at: usize) -> anyhow::Result<u32> {
            Ok(u32::from_le_bytes(
                bytes
                    .get(at..at + 4)
                    .ok_or_else(|| anyhow::anyhow!("truncated GLB"))?
                    .try_into()?,
            ))
        }
        anyhow::ensure!(
            number(&bytes, 4)? == 2 && number(&bytes, 8)? as usize == bytes.len(),
            "invalid GLB header"
        );
        let mut at = 12;
        let mut metadata = None;
        let mut binary = None;
        while at < bytes.len() {
            let length = number(&bytes, at)? as usize;
            let kind = number(&bytes, at + 4)?;
            at += 8;
            let chunk = bytes
                .get(
                    at..at
                        .checked_add(length)
                        .ok_or_else(|| anyhow::anyhow!("GLB overflow"))?,
                )
                .ok_or_else(|| anyhow::anyhow!("truncated GLB chunk"))?;
            match kind {
                0x4e4f534a => {
                    anyhow::ensure!(
                        metadata.is_none() && length <= 16 * 1024 * 1024,
                        "invalid GLB JSON"
                    );
                    metadata = Some(chunk);
                }
                0x004e4942 => {
                    anyhow::ensure!(binary.is_none(), "duplicate GLB binary");
                    binary = Some(chunk);
                }
                _ => {}
            }
            at += length;
        }
        add_json(
            metadata.ok_or_else(|| anyhow::anyhow!("GLB JSON missing"))?,
            binary,
            &mut result,
            settings,
            &mut total,
            max_total,
        )?;
    } else {
        let length = std::fs::metadata(path)?.len();
        anyhow::ensure!(
            length <= 32 * 1024 * 1024,
            "JSON model budget exceeded or binary model unsupported"
        );
        let bytes = std::fs::read(path)?;
        add_json(&bytes, None, &mut result, settings, &mut total, max_total)?;
    }
    Ok(result)
}

const EXPOSED: &[&str] = &[
    "FEMALE_BREAST_EXPOSED",
    "FEMALE_GENITALIA_EXPOSED",
    "MALE_GENITALIA_EXPOSED",
    "ANUS_EXPOSED",
    "BUTTOCKS_EXPOSED",
];
async fn model_inputs(path: &Path, settings: &ModerationSettings) -> anyhow::Result<Inputs> {
    let owned = path.to_owned();
    let options = settings.clone();
    let first = tokio::task::spawn_blocking(move || extract(&owned, &options)).await?;
    if let Ok(inputs) = first {
        return Ok(inputs);
    }
    let parser = settings
        .ysm_parser_path
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No YSM decoder configured"))?;
    let scratch = tempfile::tempdir()?;
    let input = scratch.path().join("input");
    let output = scratch.path().join("output");
    tokio::fs::create_dir(&input).await?;
    tokio::fs::create_dir(&output).await?;
    tokio::fs::copy(path, input.join("model.ysm")).await?;
    let mut command = tokio::process::Command::new(parser);
    command
        .args(["-i"])
        .arg(&input)
        .args(["-o"])
        .arg(&output)
        .args(["-j", "1"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn()?;
    match tokio::time::timeout(
        std::time::Duration::from_secs(settings.timeout_seconds),
        child.wait(),
    )
    .await
    {
        Ok(Ok(status)) if status.success() => {}
        _ => {
            let _ = child.kill().await;
            anyhow::bail!("YSM decoding failed or timed out");
        }
    }
    let options = settings.clone();
    // Keep the temporary directory alive inside the blocking task even if the caller is cancelled.
    tokio::task::spawn_blocking(move || {
        let _scratch = scratch;
        extract(&output, &options)
    })
    .await?
}
pub async fn scan(path: &Path, name: &str, settings: &ModerationSettings) -> ScanResult {
    scan_limited(
        path,
        name,
        settings,
        MODEL_SCAN_SLOTS
            .get_or_init(|| crate::performance::Limiter::new(2))
            .clone(),
    )
    .await
}
pub async fn scan_limited(
    path: &Path,
    _name: &str,
    settings: &ModerationSettings,
    limiter: std::sync::Arc<crate::performance::Limiter>,
) -> ScanResult {
    let path = path.to_owned();
    let settings = settings.clone();
    // Detached worker retains the admission permit until decoding really ends, even on HTTP cancellation.
    tokio::spawn(async move {
        let _permit = limiter.acquire().await;
        scan_inner(&path, &settings).await
    })
    .await
    .unwrap_or_else(|_| ScanResult::unknown("Model scanner worker failed"))
}
async fn scan_inner(path: &Path, settings: &ModerationSettings) -> ScanResult {
    if !settings.detection_enabled {
        let mut r = ScanResult::unknown("Recognition disabled");
        r.status = "DISABLED".into();
        return r;
    }
    let inputs = match model_inputs(path, settings).await {
        Ok(inputs) => inputs,
        _ => {
            return ScanResult::unknown(
                "Unreadable/encrypted model, unsupported format or extraction budget exceeded",
            );
        }
    };
    if inputs.tagged {
        return ScanResult {
            status: "R18".into(),
            reason: "R18 model metadata/tag".into(),
            textures_scanned: 0,
            score: None,
        };
    }
    if !settings.texture_detection_enabled {
        return ScanResult::unknown("No R18 tags; texture recognition disabled");
    }
    if inputs.images.is_empty() {
        return ScanResult::unknown("No readable embedded textures");
    }
    let Some(url) = settings.scanner_url.as_deref() else {
        return ScanResult::unknown("Scanner URL is not configured");
    };
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(settings.timeout_seconds))
        .build()
    {
        Ok(client) => client,
        Err(_) => return ScanResult::unknown("Cannot initialize texture scanner"),
    };
    let work = async {
        let mut scanned = 0;
        let mut maximum = 0.0f64;
        let mut exposed_found = false;
        for image in inputs.images {
            let form = reqwest::multipart::Form::new().part(
                "f1",
                reqwest::multipart::Part::bytes(image).file_name("texture.png"),
            );
            let mut response = client
                .post(url)
                .multipart(form)
                .send()
                .await?
                .error_for_status()?;
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                anyhow::ensure!(
                    bytes.len() + chunk.len() <= 1024 * 1024,
                    "scanner response budget exceeded"
                );
                bytes.extend_from_slice(&chunk);
            }
            let body: Value = serde_json::from_slice(&bytes)?;
            anyhow::ensure!(body["success"] == true, "scanner failed");
            let predictions = body["prediction"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("invalid scanner response"))?;
            anyhow::ensure!(predictions.len() == 1, "invalid scanner response");
            for detection in predictions[0]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("invalid prediction"))?
            {
                let label = detection["class"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing class"))?;
                let score = detection["score"]
                    .as_f64()
                    .filter(|s| s.is_finite() && (0.0..=1.0).contains(s))
                    .ok_or_else(|| anyhow::anyhow!("invalid score"))?;
                if EXPOSED.contains(&label) {
                    exposed_found = true;
                    maximum = maximum.max(score);
                }
            }
            scanned += 1;
            if exposed_found && maximum >= settings.score_threshold {
                return Ok::<_, anyhow::Error>(ScanResult {
                    status: "R18".into(),
                    reason: "Exposed content detected in texture".into(),
                    textures_scanned: scanned,
                    score: Some(maximum),
                });
            }
        }
        Ok(ScanResult {
            status: if inputs.complete { "CLEAN" } else { "UNKNOWN" }.into(),
            reason: if inputs.complete {
                "No exposed content detected in embedded textures"
            } else {
                "Some textures could not be inspected"
            }
            .into(),
            textures_scanned: scanned,
            score: Some(maximum),
        })
    };
    match tokio::time::timeout(
        std::time::Duration::from_secs(settings.timeout_seconds),
        work,
    )
    .await
    {
        Ok(Ok(result)) => result,
        _ => ScanResult::unknown("Texture scanner unavailable, timed out or returned invalid data"),
    }
}
