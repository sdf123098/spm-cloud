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
        include_str!("../web/THREE-LICENSE.txt"),
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
    // Keep worker and permit together when a browser disconnects mid-decode.
    let result = tokio::spawn(async move {
        let permit = limiter.acquire().await;
        let owned = path.clone();
        let first = tokio::task::spawn_blocking(move || collect(&owned))
            .await
            .map_err(|e| CloudError::Internal(e.into()))?;
        if let Ok(result) = first {
            return Ok(result);
        }
        let parser = settings.ysm_parser_path.ok_or_else(|| {
            CloudError::invalid_metadata("模型无法直接预览；请在服务器配置中设置 YSMParser 路径")
        })?;
        let scratch = tempfile::tempdir()?;
        let input = scratch.path().join("input");
        let output = scratch.path().join("output");
        tokio::fs::create_dir(&input).await?;
        tokio::fs::create_dir(&output).await?;
        tokio::fs::copy(&path, input.join("model.ysm")).await?;
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
        tokio::task::spawn_blocking(move || {
            let _scratch = scratch;
            let _permit = permit;
            collect(&output)
        })
        .await
        .map_err(|e| CloudError::Internal(e.into()))?
    })
    .await
    .map_err(|e| CloudError::Internal(e.into()))??;
    Ok(Json(result))
}
fn collect(path: &FilePath) -> Result<Value, CloudError> {
    let mut documents = Vec::new();
    let mut images = Vec::new();
    let mut total = 0u64;
    fn add(
        name: &str,
        bytes: Vec<u8>,
        documents: &mut Vec<Value>,
        images: &mut Vec<Value>,
        total: &mut u64,
    ) -> Result<(), CloudError> {
        *total += bytes.len() as u64;
        if *total > 48 * 1024 * 1024 {
            return Err(CloudError::AssetTooLarge);
        }
        let lower = name.to_lowercase();
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
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                if value.get("elements").is_some()
                    || value.get("minecraft:geometry").is_some()
                    || value.get("bones").is_some()
                    || value.get("asset").is_some()
                {
                    documents.push(json!({"path":name,"value":value}));
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
                        if entry.metadata()?.len() > 16 * 1024 * 1024 {
                            return Err(CloudError::AssetTooLarge);
                        }
                        add(
                            &name,
                            std::fs::read(entry.path())?,
                            &mut documents,
                            &mut images,
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
                if entry.size() > 16 * 1024 * 1024 {
                    return Err(CloudError::AssetTooLarge);
                }
                let mut bytes = Vec::new();
                entry.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
                if bytes.len() > 16 * 1024 * 1024 {
                    return Err(CloudError::AssetTooLarge);
                }
                add(&name, bytes, &mut documents, &mut images, &mut total)?;
            }
        } else {
            if std::fs::metadata(path)?.len() > 32 * 1024 * 1024 {
                return Err(CloudError::AssetTooLarge);
            }
            let bytes = std::fs::read(path)?;
            if magic == *b"glTF" {
                documents.push(json!({"path":"model.glb","glb":STANDARD.encode(bytes)}));
            } else {
                add("model.json", bytes, &mut documents, &mut images, &mut total)?;
            }
        }
    }
    if documents.is_empty() {
        return Err(CloudError::invalid_metadata("此模型没有可读取的几何数据"));
    }
    Ok(json!({"documents":documents,"images":images}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
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
