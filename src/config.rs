use std::{env, net::SocketAddr, path::PathBuf};

use crate::error::CloudError;

#[derive(Clone)]
pub struct CloudConfig {
    pub instance_id: String,
    pub origin: String,
    pub bind_addr: SocketAddr,
    pub database_path: PathBuf,
    pub object_dir: PathBuf,
    pub access_token: Option<String>,
    pub bootstrap_account_id: String,
    pub bootstrap_password_hash: Option<String>,
    pub allow_self_registration: bool,
    pub max_asset_bytes: u64,
    pub max_message_bytes: usize,
    pub max_entity_query_count: usize,
    pub visual: crate::visual::VisualSettings,
    pub trusted_proxy_ips: Vec<std::net::IpAddr>,
}

impl CloudConfig {
    pub fn from_env() -> Result<Self, CloudError> {
        let cwd = env::current_dir()?;
        let environment = crate::config_file::current_environment()?;
        Ok(crate::config_file::LoadedConfig::load_with(None, &cwd, &environment)?.cloud)
    }
}

pub(crate) fn normalize_origin(origin: &str) -> Result<String, CloudError> {
    let url = reqwest::Url::parse(origin.trim())
        .map_err(|_| CloudError::configuration("SPM_CLOUD_ORIGIN must be a bare HTTPS origin"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(CloudError::configuration(
            "SPM_CLOUD_ORIGIN must be a bare HTTPS origin",
        ));
    }
    Ok(url.origin().ascii_serialization())
}

pub fn validate_slug(value: &str, field: &str) -> Result<(), CloudError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
        && value.as_bytes()[0].is_ascii_alphanumeric();
    if valid {
        Ok(())
    } else {
        Err(CloudError::invalid_metadata(format!("invalid {field}")))
    }
}

impl std::fmt::Debug for CloudConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudConfig")
            .field("instance_id", &self.instance_id)
            .field("origin", &self.origin)
            .field("bind_addr", &self.bind_addr)
            .field("database_path", &self.database_path)
            .field("object_dir", &self.object_dir)
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "bootstrap_password_hash",
                &self.bootstrap_password_hash.as_ref().map(|_| "[REDACTED]"),
            )
            .field("bootstrap_account_id", &self.bootstrap_account_id)
            .field("allow_self_registration", &self.allow_self_registration)
            .field("max_asset_bytes", &self.max_asset_bytes)
            .field("max_message_bytes", &self.max_message_bytes)
            .field("max_entity_query_count", &self.max_entity_query_count)
            .field("trusted_proxy_ips", &self.trusted_proxy_ips)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_defaults_to_official_open_policy_and_honors_operator_setting() {
        use crate::config_file::{Environment, LoadedConfig};
        let dir = tempfile::tempdir().unwrap();
        let load = |value: Option<&str>| {
            let env = value
                .map(|v| {
                    Environment::from([("SPM_CLOUD_ALLOW_SELF_REGISTRATION".into(), v.into())])
                })
                .unwrap_or_default();
            LoadedConfig::load_with(None, dir.path(), &env)
        };
        assert!(load(None).unwrap().cloud.allow_self_registration);
        assert!(load(Some("true")).unwrap().cloud.allow_self_registration);
        assert!(!load(Some("false")).unwrap().cloud.allow_self_registration);
        assert!(load(Some("yes")).is_err());
    }
    #[test]
    fn signing_origin_is_normalized_and_cannot_include_credentials_or_paths() {
        assert_eq!(
            normalize_origin("https://cloud.example.com/").unwrap(),
            "https://cloud.example.com"
        );
        assert_eq!(
            normalize_origin("https://cloud.example.com:8443").unwrap(),
            "https://cloud.example.com:8443"
        );
        for origin in [
            "http://example.com",
            "https://user:password@example.com",
            "https://example.com/v1",
            "https://example.com?q=x",
            "https://example.com/#x",
        ] {
            assert!(normalize_origin(origin).is_err());
        }
    }
}
