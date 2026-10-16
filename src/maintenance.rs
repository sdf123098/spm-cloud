//! Owner-only maintenance, consistent SQLite snapshots and verified native updates.
use crate::{
    admin::{AuditQuery, authorize},
    api::AppState,
    error::CloudError,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UpdateSettings {
    pub auto_update: bool,
    pub check_interval_hours: u64,
}
impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            auto_update: false,
            check_interval_hours: 24,
        }
    }
}
impl UpdateSettings {
    pub fn validate(&self) -> Result<(), CloudError> {
        if !(1..=168).contains(&self.check_interval_hours) {
            return Err(CloudError::invalid_metadata(
                "updates.check_interval_hours: 更新检查间隔应为 1–168 小时",
            ));
        }
        Ok(())
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/api/maintenance", get(status))
        .route("/admin/api/updates/check", post(check))
        .route("/admin/api/updates/install", post(install))
        .route("/admin/api/exports/backup", get(backup))
        .route("/admin/api/exports/models", get(models))
        .route("/admin/api/assets/{asset}/export", get(model))
        .route("/admin/api/exports/audit", get(audit))
        .route("/admin/api/exports/logs", get(logs))
}
pub(crate) fn native_supported() -> bool {
    if std::env::var_os("SPM_CLOUD_CONTAINER").is_some()
        || std::path::Path::new("/.dockerenv").exists()
    {
        return false;
    }
    // 真正决定"能否自更新"的是二进制所在目录可写，而不是"不在容器里"。
    // systemd 的 ProtectSystem=strict / 只读挂载下目录不可写，UI 必须如实显示不支持，
    // 否则用户点了安装才失败 (Read-only file system)。
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .map(|dir| {
            tempfile::Builder::new()
                .prefix(".spm-write-probe")
                .tempfile_in(&dir)
                .is_ok()
        })
        .unwrap_or(false)
}
async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, CloudError> {
    authorize(&state, &headers, true)?;
    let config = state.runtime_config();
    let (uploads, processing) = config.performance.resolved()?;
    Ok(Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"updates":config.updates,"native_update_supported":native_supported(),"upload_concurrency":uploads,"model_processing_concurrency":processing,"log_note":"当前进程最近 2 MiB 运行日志；重启前日志请从部署平台导出"}),
    ))
}
fn attachment(body: Body, mime: &str, filename: &str) -> Response {
    (
        [
            ("content-type", mime.to_owned()),
            (
                "content-disposition",
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response()
}
async fn temp_response(file: tempfile::NamedTempFile, name: &str) -> Result<Response, CloudError> {
    let file = Arc::new(file);
    let reader = tokio::fs::File::open(file.path()).await?;
    let stream = tokio_util::io::ReaderStream::new(reader).map(move |chunk| {
        let _keep = &file;
        chunk
    });
    Ok(attachment(
        Body::from_stream(stream),
        "application/zip",
        name,
    ))
}
pub(crate) fn archive(
    state: &AppState,
    include_database: bool,
) -> Result<tempfile::NamedTempFile, CloudError> {
    let scratch = tempfile::tempdir()?;
    let database = scratch.path().join("spm-cloud.db");
    let (hashes, entries) = {
        let conn = state
            .store
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        // VACUUM INTO is a SQLite transactionally consistent snapshot, including WAL changes.
        conn.snapshot_sqlite(&database)?;
        let snapshot = rusqlite::Connection::open(&database)?;
        let hashes = snapshot
            .prepare("SELECT DISTINCT raw_sha256 FROM asset_revisions ORDER BY raw_sha256")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let entries = snapshot.prepare("SELECT asset_id,revision,name,format,raw_sha256,byte_length FROM asset_revisions ORDER BY asset_id,revision")?.query_map([],|r|Ok(json!({"asset_id":r.get::<_,String>(0)?,"revision":r.get::<_,i64>(1)?,"name":r.get::<_,String>(2)?,"format":r.get::<_,String>(3)?,"raw_sha256":r.get::<_,String>(4)?,"byte_length":r.get::<_,i64>(5)?})))?.collect::<Result<Vec<_>,_>>()?;
        (hashes, entries)
    };
    let file = tempfile::NamedTempFile::new()?;
    let mut zip = zip::ZipWriter::new(file.reopen()?);
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut inventory = Vec::new();
    for sha in hashes {
        if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CloudError::invalid_metadata("invalid stored object hash"));
        }
        let path = format!("{}/{}", &sha[..2], sha);
        let mut input = std::fs::File::open(state.store.object_path_for_sha(&sha))?;
        zip.start_file(format!("objects/{path}"), opts)
            .map_err(|e| CloudError::Internal(e.into()))?;
        let mut hasher = Sha256::new();
        let mut size = 0;
        let mut buffer = [0u8; 65536];
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
            zip.write_all(&buffer[..count])?;
            size += count;
        }
        if hex::encode(hasher.finalize()) != sha {
            return Err(CloudError::AssetHashMismatch);
        }
        inventory.push(json!({"path":path,"bytes":size,"sha256":sha}));
    }
    if !include_database {
        for (index, entry) in entries.iter().enumerate() {
            let sha = entry["raw_sha256"].as_str().ok_or(CloudError::NotFound)?;
            let format = entry["format"].as_str().unwrap_or("bin");
            let ext = if ["ysm", "zip", "bbmodel", "gltf", "glb"].contains(&format) {
                format
            } else {
                "bin"
            };
            let name: String = entry["name"]
                .as_str()
                .unwrap_or("model")
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .take(80)
                .collect();
            let path = format!(
                "models/{:04}-{}-r{}.{}",
                index + 1,
                name,
                entry["revision"],
                ext
            );
            zip.start_file(path, opts)
                .map_err(|e| CloudError::Internal(e.into()))?;
            std::io::copy(
                &mut std::fs::File::open(state.store.object_path_for_sha(sha))?,
                &mut zip,
            )?;
        }
    }
    if include_database {
        zip.start_file("spm-cloud.db", opts)
            .map_err(|e| CloudError::Internal(e.into()))?;
        std::io::copy(&mut std::fs::File::open(&database)?, &mut zip)?;
    }
    let mut db_hash = Sha256::new();
    std::io::copy(
        &mut std::fs::File::open(&database)?,
        &mut HashWriter(&mut db_hash),
    )?;
    zip.start_file("manifest.json", opts)
        .map_err(|e| CloudError::Internal(e.into()))?;
    zip.write_all(serde_json::to_string_pretty(&json!({"schema":if include_database {"spm.cloud.backup.v1"} else {"spm.cloud.models.v1"},"database":{"path":"spm-cloud.db","sha256":hex::encode(db_hash.finalize())},"objects":{"path":"objects","file_count":inventory.len(),"entries":inventory},"models":entries,"operator_note":"Consistent online SQLite snapshot; immutable content objects, including retained revisions. Configuration and credential files are not included."})).map_err(|e| CloudError::Internal(e.into()))?.as_bytes())?;
    zip.finish().map_err(|e| CloudError::Internal(e.into()))?;
    Ok(file)
}
struct HashWriter<'a>(&'a mut Sha256);
impl Write for HashWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
async fn export_archive(
    state: AppState,
    headers: HeaderMap,
    database: bool,
) -> Result<Response, CloudError> {
    let (actor, _) = authorize(&state, &headers, true)?;
    let guard = state.maintenance_lock.clone().lock_owned().await;
    let worker = state.clone();
    let file = tokio::task::spawn_blocking(move || {
        let _guard = guard;
        archive(&worker, database)
    })
    .await
    .map_err(|e| CloudError::Internal(e.into()))??;
    state.store.admin_audit(
        &actor,
        if database {
            "BACKUP_EXPORTED"
        } else {
            "MODELS_EXPORTED"
        },
        None,
        json!({}),
    )?;
    temp_response(
        file,
        if database {
            "spm-cloud-backup.zip"
        } else {
            "spm-cloud-models.zip"
        },
    )
    .await
}
async fn backup(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, CloudError> {
    export_archive(state, headers, true).await
}
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, CloudError> {
    export_archive(state, headers, false).await
}
async fn model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
) -> Result<Response, CloudError> {
    let (actor, _) = authorize(&state, &headers, false)?;
    let (_, sha, _) = state.store.admin_asset(&asset)?;
    let reader = tokio::fs::File::open(state.store.object_path_for_sha(&sha)).await?;
    let format:String=state.store.connection.lock().unwrap().query_row("SELECT format FROM asset_revisions r JOIN assets a ON a.asset_id=r.asset_id AND a.current_revision=r.revision WHERE a.asset_id=?1",[&asset],|r|r.get(0))?;
    let ext = if ["ysm", "zip", "bbmodel", "gltf", "glb"].contains(&format.as_str()) {
        format.as_str()
    } else {
        "bin"
    };
    state
        .store
        .admin_audit(&actor, "MODEL_EXPORTED", Some(&asset), json!({}))?;
    Ok(attachment(
        Body::from_stream(tokio_util::io::ReaderStream::new(reader)),
        "application/octet-stream",
        &format!("{sha}.{ext}"),
    ))
}
async fn audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditQuery>,
) -> Result<Response, CloudError> {
    let (actor, _) = authorize(&state, &headers, false)?;
    let value = crate::admin::audit_entries(&state, &query, true)?;
    state
        .store
        .admin_audit(&actor, "AUDIT_EXPORTED", None, json!({}))?;
    Ok(attachment(
        Body::from(serde_json::to_vec_pretty(&value).map_err(|e| CloudError::Internal(e.into()))?),
        "application/json",
        "spm-cloud-audit.json",
    ))
}
static LOGS: OnceLock<Mutex<VecDeque<u8>>> = OnceLock::new();
pub struct LogWriter;
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogWriter {
    type Writer = Self;
    fn make_writer(&'a self) -> Self {
        Self
    }
}
impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut logs = LOGS
            .get_or_init(|| Mutex::new(VecDeque::new()))
            .lock()
            .unwrap();
        logs.extend(buf);
        let excess = logs.len().saturating_sub(2 * 1024 * 1024);
        logs.drain(..excess);
        std::io::stdout().write_all(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stdout().flush()
    }
}
async fn logs(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, CloudError> {
    let (actor, _) = authorize(&state, &headers, true)?;
    let logs: Vec<u8> = LOGS
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .unwrap()
        .iter()
        .copied()
        .collect();
    state
        .store
        .admin_audit(&actor, "LOGS_EXPORTED", None, json!({}))?;
    Ok(attachment(
        Body::from(logs),
        "text/plain; charset=utf-8",
        "spm-cloud-runtime.log",
    ))
}

fn version(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.trim_start_matches('v').split('.');
    let result = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    if parts.next().is_some() {
        None
    } else {
        Some(result)
    }
}
fn release_target() -> String {
    let arch = std::env::consts::ARCH;
    let suffix = match std::env::consts::OS {
        "windows" => "pc-windows-msvc",
        "macos" => "apple-darwin",
        _ => "unknown-linux-gnu",
    };
    format!(
        "spm-cloud-{arch}-{suffix}.{}",
        if cfg!(windows) { "zip" } else { "tar.xz" }
    )
}
async fn latest() -> Result<Value, CloudError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("spm-cloud-updater")
        .build()
        .map_err(|e| CloudError::Internal(e.into()))?;
    let response = client
        .get("https://api.github.com/repos/sdf123098/spm-cloud/releases/latest")
        .send()
        .await
        .map_err(|e| CloudError::Internal(e.into()))?
        .error_for_status()
        .map_err(|e| CloudError::Internal(e.into()))?;
    let release: Value = response
        .json()
        .await
        .map_err(|e| CloudError::Internal(e.into()))?;
    let tag = release["tag_name"]
        .as_str()
        .ok_or_else(|| CloudError::invalid_metadata("invalid release tag"))?;
    let newer = version(tag)
        .zip(version(env!("CARGO_PKG_VERSION")))
        .is_some_and(|(a, b)| a > b);
    let target = release_target();
    let assets = release["assets"]
        .as_array()
        .ok_or_else(|| CloudError::invalid_metadata("invalid release assets"))?;
    let asset = assets.iter().find(|a| a["name"] == target);
    let checksum = assets
        .iter()
        .find(|a| a["name"] == format!("{target}.sha256"));
    Ok(
        json!({"current_version":env!("CARGO_PKG_VERSION"),"latest_version":tag,"update_available":newer,"release_url":release["html_url"],"asset_url":asset.map(|a|&a["browser_download_url"]),"checksum_url":checksum.map(|a|&a["browser_download_url"]),"asset_name":target,"native_update_supported":native_supported()&&asset.is_some()&&checksum.is_some()}),
    )
}
async fn check(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, CloudError> {
    authorize(&state, &headers, true)?;
    Ok(Json(latest().await?))
}
async fn install(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, CloudError> {
    let (actor, _) = authorize(&state, &headers, true)?;
    let guard = state
        .maintenance_lock
        .clone()
        .try_lock_owned()
        .map_err(|_| CloudError::RateLimited)?;
    let release = latest().await?;
    prepare_update(state, actor, release, guard).await?;
    Ok(Json(
        json!({"scheduled":true,"message":"更新包已校验并备份，服务将在数秒后重启"}),
    ))
}
async fn download(url: &str, dest: &std::path::Path, limit: u64) -> Result<(), CloudError> {
    let parsed = reqwest::Url::parse(url).map_err(|e| CloudError::Internal(e.into()))?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("github.com")
        || !parsed
            .path()
            .starts_with("/sdf123098/spm-cloud/releases/download/")
    {
        return Err(CloudError::invalid_metadata("untrusted release URL"));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .user_agent("spm-cloud-updater")
        .build()
        .map_err(|e| CloudError::Internal(e.into()))?;
    let mut response = client
        .get(parsed)
        .send()
        .await
        .map_err(|e| CloudError::Internal(e.into()))?
        .error_for_status()
        .map_err(|e| CloudError::Internal(e.into()))?;
    let mut file = tokio::fs::File::create(dest).await?;
    let mut total = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| CloudError::Internal(e.into()))?
    {
        total += chunk.len() as u64;
        if total > limit {
            return Err(CloudError::AssetTooLarge);
        }
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
    }
    Ok(())
}
fn quote(value: &str) -> String {
    format!(
        "'{}'",
        value.replace('\'', if cfg!(windows) { "''" } else { "'\"'\"'" })
    )
}
async fn prepare_update(
    state: AppState,
    actor: String,
    release: Value,
    guard: tokio::sync::OwnedMutexGuard<()>,
) -> Result<(), CloudError> {
    if release["update_available"] != true || release["native_update_supported"] != true {
        return Err(CloudError::invalid_metadata(
            "此部署没有可自动安装的更新包；容器请通过部署平台更新",
        ));
    }
    let exe = std::env::current_exe()?;
    let parent = exe.parent().ok_or(CloudError::NotFound)?;
    let scratch = tempfile::Builder::new()
        .prefix("spm-update-")
        .tempdir_in(parent)?;
    let package = scratch.path().join("package");
    let checksum = scratch.path().join("checksum");
    download(
        release["asset_url"].as_str().ok_or(CloudError::NotFound)?,
        &package,
        256 * 1024 * 1024,
    )
    .await?;
    download(
        release["checksum_url"]
            .as_str()
            .ok_or(CloudError::NotFound)?,
        &checksum,
        4096,
    )
    .await?;
    let expected = tokio::fs::read_to_string(&checksum)
        .await?
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let package_copy = package.clone();
    let scratch_path = scratch.path().to_owned();
    let expected_version = release["latest_version"]
        .as_str()
        .ok_or(CloudError::NotFound)?
        .trim_start_matches('v')
        .to_owned();
    let staged = tokio::task::spawn_blocking(move || -> Result<std::path::PathBuf, CloudError> {
        let mut hasher = Sha256::new();
        std::io::copy(
            &mut std::fs::File::open(&package_copy)?,
            &mut HashWriter(&mut hasher),
        )?;
        if expected != hex::encode(hasher.finalize()) {
            return Err(CloudError::AssetHashMismatch);
        }
        let binary = if cfg!(windows) {
            "spm-cloud.exe"
        } else {
            "spm-cloud"
        };
        let staged = scratch_path.join(binary);
        if cfg!(windows) {
            let mut zip = zip::ZipArchive::new(std::fs::File::open(&package_copy)?)
                .map_err(|e| CloudError::Internal(e.into()))?;
            let index = (0..zip.len())
                .find(|&i| {
                    zip.by_index(i).ok().is_some_and(|f| {
                        std::path::Path::new(f.name())
                            .file_name()
                            .is_some_and(|n| n == binary)
                    })
                })
                .ok_or(CloudError::NotFound)?;
            let mut entry = zip
                .by_index(index)
                .map_err(|e| CloudError::Internal(e.into()))?;
            if entry.size() > 128 * 1024 * 1024 {
                return Err(CloudError::AssetTooLarge);
            }
            std::io::copy(&mut entry, &mut std::fs::File::create(&staged)?)?;
        } else {
            let list = std::process::Command::new("tar")
                .args(["-tf"])
                .arg(&package_copy)
                .output()?;
            let member = String::from_utf8_lossy(&list.stdout)
                .lines()
                .find(|s| {
                    std::path::Path::new(s)
                        .file_name()
                        .is_some_and(|n| n == binary)
                })
                .map(str::to_owned)
                .ok_or(CloudError::NotFound)?;
            let file = std::fs::File::create(&staged)?;
            let status = std::process::Command::new("tar")
                .args(["-xOf"])
                .arg(&package_copy)
                .arg(&member)
                .stdout(file)
                .status()?;
            if !status.success() {
                return Err(CloudError::invalid_metadata("cannot unpack release"));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
            }
        }
        let mut verify = std::process::Command::new(&staged);
        verify.arg("--version");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            verify.creation_flags(0x08000000);
        }
        let output = verify.output()?;
        if !output.status.success()
            || String::from_utf8_lossy(&output.stdout).trim()
                != format!("spm-cloud {expected_version}")
        {
            return Err(CloudError::invalid_metadata(
                "更新程序版本或运行环境不匹配，保留当前版本",
            ));
        }
        Ok(staged)
    })
    .await
    .map_err(|e| CloudError::Internal(e.into()))??;
    // Finish in-flight uploads before snapshot/restart; new admissions wait.
    let upload_pause = state.upload_slots.pause();
    tokio::time::timeout(
        std::time::Duration::from_secs(600),
        state.upload_slots.idle(),
    )
    .await
    .map_err(|_| CloudError::RateLimited)?;
    let processing_pause = state.processing_slots.pause();
    tokio::time::timeout(
        std::time::Duration::from_secs(600),
        state.processing_slots.idle(),
    )
    .await
    .map_err(|_| CloudError::RateLimited)?;
    let worker = state.clone();
    let backup_path = scratch.path().join("backup.zip");
    tokio::task::spawn_blocking(move || -> Result<(), CloudError> {
        archive(&worker, true)?
            .persist(backup_path)
            .map_err(|e| e.error)?;
        Ok(())
    })
    .await
    .map_err(|e| CloudError::Internal(e.into()))??;
    state.store.admin_audit(
        &actor,
        "UPDATE_SCHEDULED",
        None,
        json!({"version":release["latest_version"]}),
    )?;
    let cwd = std::env::current_dir()?;
    let old = exe.with_extension("previous");
    let pid = std::process::id();
    let args = std::env::args()
        .skip(1)
        .map(|a| quote(&a))
        .collect::<Vec<_>>()
        .join(if cfg!(windows) { "," } else { " " });
    let script = if cfg!(windows) {
        format!(
            "$ErrorActionPreference='Stop'\nWait-Process -Id {pid} -ErrorAction SilentlyContinue\nMove-Item -LiteralPath {} -Destination {} -Force\ntry {{ Move-Item -LiteralPath {} -Destination {} -Force; Set-Location -LiteralPath {}; & {} {}; if ($LASTEXITCODE -ne 0) {{ throw 'updated service failed' }} }} catch {{ Move-Item -LiteralPath {} -Destination {} -Force; Set-Location -LiteralPath {}; & {} {} }}",
            quote(&exe.to_string_lossy()),
            quote(&old.to_string_lossy()),
            quote(&staged.to_string_lossy()),
            quote(&exe.to_string_lossy()),
            quote(&cwd.to_string_lossy()),
            quote(&exe.to_string_lossy()),
            if args.is_empty() {
                String::new()
            } else {
                format!("@({args})")
            },
            quote(&old.to_string_lossy()),
            quote(&exe.to_string_lossy()),
            quote(&cwd.to_string_lossy()),
            quote(&exe.to_string_lossy()),
            if args.is_empty() {
                String::new()
            } else {
                format!("@({args})")
            }
        )
    } else {
        format!(
            "while kill -0 {pid} 2>/dev/null; do sleep 1; done\nmv -f {} {} || exit 1\nmv -f {} {} || {{ mv -f {} {}; exit 1; }}\ncd {} || exit 1\nexec {} {args}\n",
            quote(&exe.to_string_lossy()),
            quote(&old.to_string_lossy()),
            quote(&staged.to_string_lossy()),
            quote(&exe.to_string_lossy()),
            quote(&old.to_string_lossy()),
            quote(&exe.to_string_lossy()),
            quote(&cwd.to_string_lossy()),
            quote(&exe.to_string_lossy())
        )
    };
    let helper = scratch.path().join(if cfg!(windows) {
        "restart.ps1"
    } else {
        "restart.sh"
    });
    tokio::fs::write(&helper, script).await?;
    let mut command = std::process::Command::new(if cfg!(windows) {
        "powershell.exe"
    } else {
        "sh"
    });
    if cfg!(windows) {
        command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
    }
    command.arg(&helper).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.spawn()?;
    let _saved = scratch.keep();
    tokio::spawn(async move {
        let _guard = guard;
        let _admission = (upload_pause, processing_pause);
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        std::process::exit(0);
    });
    Ok(())
}
pub fn start_update_loop(state: AppState) {
    tokio::spawn(async move {
        let mut next = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let settings = state.runtime_config().updates;
            if !settings.auto_update || tokio::time::Instant::now() < next {
                continue;
            }
            next = tokio::time::Instant::now()
                + std::time::Duration::from_secs(settings.check_interval_hours * 3600);
            if let Ok(guard) = state.maintenance_lock.clone().try_lock_owned() {
                match latest().await {
                    Ok(release)
                        if release["update_available"] == true
                            && release["native_update_supported"] == true =>
                    {
                        if let Err(error) = prepare_update(
                            state.clone(),
                            state.config.bootstrap_account_id.clone(),
                            release,
                            guard,
                        )
                        .await
                        {
                            tracing::warn!(%error,"automatic update failed; current binary retained");
                        }
                    }
                    Err(error) => tracing::warn!(%error,"update check failed"),
                    _ => {}
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_release_versions_compare_numerically() {
        assert!(version("v2.10.0") > version("2.9.9"));
        assert!(version("2.3.0-beta").is_none());
        assert!(version("2.3").is_none());
    }

    /// 只读路径的探测必须失败 —— 这正是 systemd ProtectSystem=strict 下
    /// UI 曾显示虚假"可更新"的根因。
    #[test]
    fn write_probe_rejects_readonly_dir() {
        assert!(
            tempfile::Builder::new()
                .prefix(".spm-write-probe-test")
                .tempfile_in(std::path::Path::new("/proc/1"))
                .is_err(),
            "只读路径 /proc/1 不应可写"
        );
    }

    /// 二进制所在目录可写时, native_supported() 应为 true (非容器环境)。
    #[test]
    fn write_probe_accepts_writable_exe_dir() {
        let dir = std::env::current_exe()
            .expect("current_exe")
            .parent()
            .expect("exe parent")
            .to_path_buf();
        let writable = tempfile::Builder::new()
            .prefix(".spm-write-probe-test")
            .tempfile_in(&dir)
            .is_ok();
        if !writable {
            eprintln!("跳过: 二进制目录 {dir:?} 不可写");
            return;
        }
        assert!(native_supported(), "可写目录应报告支持自更新");
    }
}
