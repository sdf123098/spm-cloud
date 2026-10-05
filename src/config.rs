use std::{env, net::SocketAddr, path::PathBuf};

use crate::error::CloudError;

#[derive(Clone, Debug)]
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
    pub trusted_proxy_ips: Vec<std::net::IpAddr>,
}

impl CloudConfig {
    pub fn from_env() -> Result<Self, CloudError> {
        let instance_id =
            env::var("SPM_CLOUD_INSTANCE_ID").unwrap_or_else(|_| "local-dev".to_owned());
        validate_slug(&instance_id, "instance_id")?;
        let origin = normalize_origin(
            &env::var("SPM_CLOUD_ORIGIN").unwrap_or_else(|_| "https://localhost".to_owned()),
        )?;
        let trusted_proxy_ips = env::var("SPM_CLOUD_TRUSTED_PROXY_IPS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse()
                    .map_err(|_| CloudError::configuration("invalid SPM_CLOUD_TRUSTED_PROXY_IPS"))
            })
            .collect::<Result<Vec<std::net::IpAddr>, CloudError>>()?;
        let bind_addr = env::var("SPM_CLOUD_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8787".to_owned())
            .parse()
            .map_err(|_| CloudError::configuration("invalid SPM_CLOUD_BIND"))?;
        let data_dir =
            PathBuf::from(env::var("SPM_CLOUD_DATA_DIR").unwrap_or_else(|_| "data".to_owned()));
        let database_path = env::var("SPM_CLOUD_DATABASE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("spm-cloud.db"));
        let object_dir = env::var("SPM_CLOUD_OBJECT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("objects"));
        let access_token = env::var("SPM_CLOUD_ACCESS_TOKEN")
            .ok()
            .filter(|v| !v.is_empty());
        let bootstrap_account_id =
            env::var("SPM_CLOUD_BOOTSTRAP_ACCOUNT").unwrap_or_else(|_| "account_local".to_owned());
        validate_slug(&bootstrap_account_id, "bootstrap_account_id")?;
        let bootstrap_password_hash = env::var("SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH")
            .ok()
            .filter(|v| !v.is_empty());
        let allow_self_registration = registration_policy(
            env::var("SPM_CLOUD_ALLOW_SELF_REGISTRATION")
                .ok()
                .as_deref(),
        )?;
        let max_asset_bytes = env::var("SPM_CLOUD_MAX_ASSET_BYTES")
            .ok()
            .map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|_| CloudError::configuration("invalid SPM_CLOUD_MAX_ASSET_BYTES"))
            })
            .transpose()?
            .unwrap_or(128 * 1024 * 1024);
        if !(1..=4 * 1024 * 1024 * 1024).contains(&max_asset_bytes) {
            return Err(CloudError::configuration(
                "SPM_CLOUD_MAX_ASSET_BYTES is outside 1..=4GiB",
            ));
        }
        let max_message_bytes = env::var("SPM_CLOUD_MAX_MESSAGE_BYTES")
            .ok()
            .map(|value| {
                value
                    .parse::<usize>()
                    .map_err(|_| CloudError::configuration("invalid SPM_CLOUD_MAX_MESSAGE_BYTES"))
            })
            .transpose()?
            .unwrap_or(64 * 1024);
        if !(1024..=1024 * 1024).contains(&max_message_bytes) {
            return Err(CloudError::configuration(
                "SPM_CLOUD_MAX_MESSAGE_BYTES is outside 1KiB..=1MiB",
            ));
        }
        Ok(Self {
            instance_id,
            origin,
            bind_addr,
            database_path,
            object_dir,
            access_token,
            bootstrap_account_id,
            bootstrap_password_hash,
            allow_self_registration,
            max_asset_bytes,
            max_message_bytes,
            trusted_proxy_ips,
        })
    }
}

fn registration_policy(value: Option<&str>) -> Result<bool, CloudError> {
    value.unwrap_or("true").parse::<bool>().map_err(|_| {
        CloudError::configuration("SPM_CLOUD_ALLOW_SELF_REGISTRATION must be true or false")
    })
}

fn normalize_origin(origin: &str) -> Result<String, CloudError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_defaults_to_official_open_policy_and_honors_operator_setting() {
        assert!(registration_policy(None).unwrap());
        assert!(registration_policy(Some("true")).unwrap());
        assert!(!registration_policy(Some("false")).unwrap());
        assert!(registration_policy(Some("yes")).is_err());
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
