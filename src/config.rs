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
}

impl CloudConfig {
    pub fn from_env() -> Result<Self, CloudError> {
        let instance_id =
            env::var("SPM_CLOUD_INSTANCE_ID").unwrap_or_else(|_| "local-dev".to_owned());
        validate_slug(&instance_id, "instance_id")?;
        let origin =
            env::var("SPM_CLOUD_ORIGIN").unwrap_or_else(|_| "https://localhost".to_owned());
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
        let allow_self_registration = env::var("SPM_CLOUD_ALLOW_SELF_REGISTRATION")
            .unwrap_or_else(|_| "false".to_owned())
            .parse::<bool>()
            .map_err(|_| {
                CloudError::configuration("SPM_CLOUD_ALLOW_SELF_REGISTRATION must be true or false")
            })?;
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
        })
    }
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
