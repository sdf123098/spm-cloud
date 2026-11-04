//! Bounded, authenticated model preview. Model-controlled paths never leave archives.
use crate::{admin::authorize, api::AppState, error::CloudError};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::get,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{io::Read, path::Path as FilePath};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/api/assets/{asset}/preview", get(preview))
        .route("/admin/preview.js", get(script))
        .route("/admin/preview.bundle.js.LEGAL.txt", get(license))
}
async fn license() -> impl axum::response::IntoResponse {
    (
        [("content-type", "text/plain; charset=utf-8")],
        concat!(
            include_str!("../web/THREE-LICENSE.txt"),
            "\n\n",
            include_str!("../web/preview.bundle.js.LEGAL.txt")
        ),
    )
}
async fn script() -> impl axum::response::IntoResponse {
    (
        [("content-type", "text/javascript; charset=utf-8")],
        include_str!("../web/preview.bundle.js"),
    )
}
async fn preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
) -> Result<Json<Value>, CloudError> {
    authorize(&state, &headers, false)?;
    let (_, sha, _) = state.store.admin_asset(&asset)?;
    let path = state.store.object_path_for_sha(&sha);
    let settings = state.runtime_config().moderation;
    let limiter = state.processing_slots.clone();
    // Keep the permit inside the task so a browser disconnecting mid-decode
    // releases the slot with the worker instead of leaking it.
    let result = tokio::spawn(async move {
        let _permit = limiter.acquire().await;
        decode_model(&path, &settings).await
    })
    .await
    .map_err(|e| CloudError::Internal(e.into()))??;
    Ok(Json(result))
}

/// Decode a model path into the preview payload shared with the browser.
///
/// `collect` handles archives and plain JSON/GLB; anything it cannot read goes to
/// the configured YSMParser. Returning the scratch directory alongside the
/// payload keeps parser output alive for the caller's lifetime.
pub(crate) async fn decode_model(
    path: &FilePath,
    settings: &crate::moderation::ModerationSettings,
) -> Result<Value, CloudError> {
    let owned = path.to_owned();
    let first = tokio::task::spawn_blocking(move || collect(&owned))
        .await
        .map_err(|e| CloudError::Internal(e.into()))?;
    match first {
        Ok(result) => Ok(result),
        Err(_) => decode_with_parser(path, settings)
            .await
            .map(|(result, _scratch)| result),
    }
}

async fn decode_with_parser(
    path: &FilePath,
    settings: &crate::moderation::ModerationSettings,
) -> Result<(Value, tempfile::TempDir), CloudError> {
    let parser = settings.ysm_parser_path.clone().ok_or_else(|| {
        CloudError::invalid_metadata("模型无法直接预览；请在服务器配置中设置 YSMParser 路径")
    })?;
    let scratch = tempfile::tempdir()?;
    let input = scratch.path().join("input");
    let output = scratch.path().join("output");
    tokio::fs::create_dir(&input).await?;
    tokio::fs::create_dir(&output).await?;
    tokio::fs::copy(path, input.join("model.ysm")).await?;
    let mut command = tokio::process::Command::new(parser);
    command
        .arg("-i")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(["-j", "1"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn()?;
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(settings.timeout_seconds),
        child.wait(),
    )
    .await
    .map_err(|_| CloudError::invalid_metadata("模型解码超时"))??;
    if !status.success() {
        return Err(CloudError::invalid_metadata("YSMParser 无法解码此模型"));
    }
    let owned = output.clone();
    let payload = tokio::task::spawn_blocking(move || collect(&owned))
        .await
        .map_err(|e| CloudError::Internal(e.into()))??;
    Ok((payload, scratch))
}
/// RFC 6901 escaping for a JSON Pointer segment. Animation names can contain
/// `/` or `~` (e.g. `tac:aim:fire:pistol` is fine, but custom names may not be).
fn json_pointer_escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn collect(path: &FilePath) -> Result<Value, CloudError> {
    let mut documents = Vec::new();
    let mut images = Vec::new();
    // ysm.json / animation files are not geometry, so they never enter
    // `documents`. They carry what the client needs to reproduce the card the
    // author designed: the texture the pack declares, whether the preview
    // rotates, and the idle pose. Passing them separately keeps `documents`
    // meaning "renderable geometry" for the existing consumers.
    let mut pack = serde_json::Map::new();
    let mut animation_files: Vec<Value> = Vec::new();
    let mut total = 0u64;
    fn add(
        name: &str,
        bytes: Vec<u8>,
        documents: &mut Vec<Value>,
        images: &mut Vec<Value>,
        pack: &mut serde_json::Map<String, Value>,
        animation_files: &mut Vec<Value>,
        total: &mut u64,
    ) -> Result<(), CloudError> {
        // The budget bounds what is *forwarded*, not what is merely parsed.
        // Charging every JSON byte up front made a pack with an 18-file animation
        // set (one 19 MB file) fail with AssetTooLarge before the preview-animation
        // filter ever ran, costing those models both their preview and thumbnail.
        // Only payload-carrying bytes count.
        let lower = name.to_lowercase();
        let charges_budget = !lower.ends_with(".json");
        if charges_budget {
            *total += bytes.len() as u64;
            if *total > 48 * 1024 * 1024 {
                return Err(CloudError::AssetTooLarge);
            }
        }
        if lower.ends_with(".png")
            || lower.ends_with(".jpg")
            || lower.ends_with(".jpeg")
            || lower.ends_with(".webp")
        {
            let mime = if lower.ends_with(".png") {
                "image/png"
            } else if lower.ends_with(".webp") {
                "image/webp"
            } else {
                "image/jpeg"
            };
            if images.len() >= 64 {
                return Err(CloudError::AssetTooLarge);
            }
            images.push(
                json!({"path":name,"data":format!("data:{mime};base64,{}",STANDARD.encode(bytes))}),
            );
        } else if lower.ends_with(".json")
            || lower.ends_with(".bbmodel")
            || lower.ends_with(".gltf")
        {
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes)
                && (value.get("elements").is_some()
                    || value.get("minecraft:geometry").is_some()
                    || value.get("bones").is_some()
                    || value.get("asset").is_some())
            {
                documents.push(json!({"path":name,"value":value}));
            } else if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                // Non-geometry JSON: ysm.json (pack metadata) or *.animation.json.
                // Kept verbatim for now; which animation matters is only known
                // once ysm.json has been read, and archive order is not fixed.
                if value.get("animations").is_some() {
                    animation_files.push(json!({"path":name,"value":value}));
                } else if value.get("files").is_some() || value.get("properties").is_some() {
                    pack.insert(name.to_owned(), value);
                }
            }
        } else if lower.ends_with(".glb") {
            documents.push(json!({"path":name,"glb":STANDARD.encode(bytes)}));
        }
        Ok(())
    }
    fn accepted(name: &str) -> bool {
        let name = name.to_lowercase();
        [
            ".json", ".bbmodel", ".gltf", ".glb", ".png", ".jpg", ".jpeg", ".webp",
        ]
        .iter()
        .any(|s| name.ends_with(s))
    }
    if path.is_dir() {
        let mut pending = vec![path.to_owned()];
        let mut count = 0;
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                count += 1;
                if count > 4096 {
                    return Err(CloudError::AssetTooLarge);
                }
                let kind = entry.file_type()?;
                if kind.is_symlink() {
                    return Err(CloudError::invalid_metadata("模型包含不支持的符号链接"));
                }
                if kind.is_dir() {
                    pending.push(entry.path());
                } else {
                    let name = entry
                        .path()
                        .strip_prefix(path)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    if accepted(&name) {
                        // Same rule as the archive branch: one animation JSON may
                        // legitimately exceed 16 MB; geometry and textures may not.
                        let entry_limit = if name.to_lowercase().ends_with(".json") {
                            48 * 1024 * 1024
                        } else {
                            16 * 1024 * 1024
                        };
                        if entry.metadata()?.len() > entry_limit {
                            return Err(CloudError::AssetTooLarge);
                        }
                        add(
                            &name,
                            std::fs::read(entry.path())?,
                            &mut documents,
                            &mut images,
                            &mut pack,
                            &mut animation_files,
                            &mut total,
                        )?;
                    }
                }
            }
        }
    } else {
        let mut file = std::fs::File::open(path)?;
        let mut magic = [0; 4];
        file.read_exact(&mut magic)?;
        if magic == *b"PK\x03\x04" {
            let mut archive =
                zip::ZipArchive::new(file).map_err(|e| CloudError::Internal(e.into()))?;
            if archive.len() > 4096 {
                return Err(CloudError::AssetTooLarge);
            }
            for index in 0..archive.len() {
                let entry = archive
                    .by_index(index)
                    .map_err(|e| CloudError::Internal(e.into()))?;
                let name = entry.name().to_owned();
                if !accepted(&name) {
                    continue;
                }
                // A per-entry cap protects against a zip bomb in geometry or
                // textures, but animation JSON is legitimately large: one pack
                // ships a 19 MB `extra.animation.json`. Allow JSON to reach the
                // total budget; the payload filter keeps only the clip in use.
                let entry_limit = if name.to_lowercase().ends_with(".json") {
                    48 * 1024 * 1024
                } else {
                    16 * 1024 * 1024
                };
                if entry.size() > entry_limit {
                    return Err(CloudError::AssetTooLarge);
                }
                let mut bytes = Vec::new();
                entry.take(entry_limit + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > entry_limit {
                    return Err(CloudError::AssetTooLarge);
                }
                add(
                    &name,
                    bytes,
                    &mut documents,
                    &mut images,
                    &mut pack,
                    &mut animation_files,
                    &mut total,
                )?;
            }
        } else {
            if std::fs::metadata(path)?.len() > 32 * 1024 * 1024 {
                return Err(CloudError::AssetTooLarge);
            }
            let bytes = std::fs::read(path)?;
            if magic == *b"glTF" {
                documents.push(json!({"path":"model.glb","glb":STANDARD.encode(bytes)}));
            } else {
                add(
                    "model.json",
                    bytes,
                    &mut documents,
                    &mut images,
                    &mut pack,
                    &mut animation_files,
                    &mut total,
                )?;
            }
        }
    }
    if documents.is_empty() {
        return Err(CloudError::invalid_metadata("此模型没有可读取的几何数据"));
    }
    // SPM runs the main model's parallel initialization around the GUI clip.
    // Keep those channels (they hide unused effects/variants), without forwarding
    // entire multi-megabyte movement, arm, projectile and compatibility files.
    let wanted = pack.values().find_map(|doc| {
        doc.pointer("/properties/preview_animation")
            .and_then(Value::as_str)
    });
    let main_path = pack.iter().find_map(|(path, doc)| {
        let main = doc.pointer("/files/player/animation/main")?.as_str()?;
        let prefix = path.rsplit_once('/').map_or("", |(prefix, _)| prefix);
        Some(if prefix.is_empty() {
            main.to_owned()
        } else {
            format!("{prefix}/{main}")
        })
    });
    let main = main_path.as_ref().and_then(|path| {
        animation_files
            .iter()
            .find(|file| file["path"].as_str() == Some(path.as_str()))
    });
    let mut selected = serde_json::Map::new();
    if let Some(main) = main
        && let Some(clips) = main.pointer("/value/animations").and_then(Value::as_object)
    {
        for (name, clip) in clips {
            let parallel = name
                .strip_prefix("pre_parallel")
                .or_else(|| name.strip_prefix("parallel"))
                .is_some_and(|suffix| {
                    suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'0'..=b'7')
                });
            if parallel || Some(name.as_str()) == wanted {
                selected.insert(name.clone(), clip.clone());
            }
        }
    }
    if let Some(name) = wanted
        && !selected.contains_key(name)
        && let Some(clip) = animation_files.iter().find_map(|file| {
            file.pointer(&format!("/value/animations/{}", json_pointer_escape(name)))
        })
    {
        selected.insert(name.to_owned(), clip.clone());
    }
    let animations: Vec<Value> = if selected.is_empty() {
        Vec::new()
    } else {
        vec![json!({"path":main_path.unwrap_or_default(),"value":{"animations":selected}})]
    };
    Ok(json!({"documents":documents,"images":images,"pack":pack,"animations":animations}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn preview_retains_main_parallel_initialization_without_other_animation_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.json"),
            r#"{"bones":[{"name":"Root"}]}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("ysm.json"), r#"{"properties":{"preview_animation":"gui"},"files":{"player":{"animation":{"main":"main.animation.json","arm":"arm.animation.json"}}}}"#).unwrap();
        std::fs::write(
            dir.path().join("arm.animation.json"),
            r#"{"animations":{"parallel1":{"bones":{"armOnly":{"scale":0}}}}}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("main.animation.json"), r#"{"animations":{"gui":{"bones":{"Root":{"scale":2}}},"pre_parallel2":{"bones":{"effect":{"scale":0}}},"parallel1":{"bones":{"Root":{"scale":1}}},"walk":{"bones":{"Root":{"scale":99}}}}}"#).unwrap();
        let payload = collect(dir.path()).unwrap();
        let clips = &payload["animations"][0]["value"]["animations"];
        assert!(clips.get("pre_parallel2").is_some());
        assert_eq!(clips["parallel1"]["bones"]["Root"]["scale"], 1);
        assert!(clips.get("gui").is_some());
        assert!(clips.get("walk").is_none());
    }
    #[test]
    fn zip_paths_are_data_and_missing_geometry_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file(
            "../../outside.json",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(br#"{"elements":[{"from":[0,0,0],"to":[1,1,1]}]}"#)
            .unwrap();
        zip.finish().unwrap();
        assert_eq!(
            collect(&path).unwrap()["documents"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(!dir.path().parent().unwrap().join("outside.json").exists());
    }
}
