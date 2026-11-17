pub const PROTOCOL_V1: &str = "spm.cloud.v1";
pub const CLOUD_OFFLINE_IDENTITY_CAPABILITY: &str = "cloud_offline_identity_v1";
pub const PLAYER_MOTION_CAPABILITY: &str = "player_motion_v1";
pub const GAME_IDENTITY_AUTH_CAPABILITY: &str = "game_identity_auth_v1";
pub const FIGURA_ASSETS_CAPABILITY: &str = "figura_assets_v1";
pub const CLIENT_VERSION_HEADER: &str = "x-spm-client-version";
pub const SUPPORTED_MODEL_FORMATS: [&str; 6] = ["ysm", "zip", "bbmodel", "gltf", "glb", "figura"];
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
pub const HEARTBEAT_INTERVAL_SECONDS: u64 = 15;
pub const HEARTBEAT_TTL_SECONDS: u64 = 45;

pub fn is_supported_model_format(format: &str) -> bool {
    let normalized = format.trim().to_ascii_lowercase();
    SUPPORTED_MODEL_FORMATS.contains(&normalized.as_str())
}

pub fn normalize_upload_format(raw: &str) -> String {
    let trimmed = raw.trim();
    if is_supported_model_format(trimmed) {
        trimmed.to_ascii_lowercase()
    } else {
        trimmed.to_owned()
    }
}

pub fn parse_client_version_tuple(raw: &str) -> Option<(u64, u64, u64)> {
    let cleaned = raw
        .trim()
        .strip_prefix(['v', 'V'])
        .unwrap_or(raw.trim())
        .split(['-', '+'])
        .next()?
        .trim();
    if cleaned.is_empty() {
        return None;
    }
    let mut parts = cleaned.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = match parts.next() {
        Some(value) => value.parse::<u64>().ok()?,
        None => 0,
    };
    let patch = match parts.next() {
        Some(value) => value.parse::<u64>().ok()?,
        None => 0,
    };
    Some((major, minor, patch))
}

pub fn client_supports_figura(client_version: Option<&str>) -> bool {
    match client_version.and_then(parse_client_version_tuple) {
        Some(version) => version >= (2, 4, 0),
        // Requests without an explicit legacy version (tests, WebUI, modern defaults) keep full format fidelity.
        None => true,
    }
}

pub fn request_supports_figura(headers: &axum::http::HeaderMap) -> bool {
    let header_value = headers
        .get(CLIENT_VERSION_HEADER)
        .and_then(|value| value.to_str().ok());
    client_supports_figura(header_value)
}

pub fn adapt_wire_format(format: &str, supports_figura: bool) -> &str {
    if !supports_figura && format.eq_ignore_ascii_case("figura") {
        // Pre-2.4.0 clients reject "figura" in CloudPlayerSelection/CloudAssetClient; ".figura" is a valid ZIP archive they can still parse via ZipModelSniffer.
        "zip"
    } else {
        format
    }
}

pub fn adapt_entries_resource_format(entries: &mut [serde_json::Value], supports_figura: bool) {
    if supports_figura {
        return;
    }
    for entry in entries {
        for container_key in ["selection", "resource"] {
            if let Some(format_value) = entry
                .get_mut(container_key)
                .and_then(|node| node.get_mut("format"))
                && let Some(raw) = format_value.as_str()
                && raw.eq_ignore_ascii_case("figura")
            {
                *format_value = serde_json::Value::String("zip".to_owned());
            }
        }
    }
}

pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/spm.cloud.v1.rs"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_model_formats_include_figura() {
        for format in ["ysm", "zip", "bbmodel", "gltf", "glb", "figura", "FIGURA"] {
            assert!(is_supported_model_format(format));
        }
        assert!(!is_supported_model_format("obj"));
        assert_eq!(normalize_upload_format(" FiGuRa "), "figura");
        assert_eq!(
            normalize_upload_format("application/octet-stream"),
            "application/octet-stream"
        );
    }

    #[test]
    fn client_version_compat_distinguishes_legacy_and_240() {
        assert_eq!(parse_client_version_tuple("2.0.0"), Some((2, 0, 0)));
        assert_eq!(parse_client_version_tuple("v2.3.1-beta.1"), Some((2, 3, 1)));
        assert_eq!(parse_client_version_tuple("2.4.0"), Some((2, 4, 0)));
        assert!(!client_supports_figura(Some("2.0.0")));
        assert!(!client_supports_figura(Some("2.3.9")));
        assert!(client_supports_figura(Some("2.4.0")));
        assert!(client_supports_figura(Some("2.4.1")));
        assert!(client_supports_figura(None));
        assert!(client_supports_figura(Some("unknown")));

        assert_eq!(adapt_wire_format("figura", false), "zip");
        assert_eq!(adapt_wire_format("figura", true), "figura");
        assert_eq!(adapt_wire_format("ysm", false), "ysm");
    }
}
