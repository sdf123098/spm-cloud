//! Startup-only administrator configuration. Loading never opens the database.
use crate::{
    config::{CloudConfig, normalize_origin, validate_slug},
    error::CloudError,
    models::IdentityProviderUpdate,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub type Environment = BTreeMap<String, String>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub schema_version: u32,
    pub instance: Instance,
    pub storage: Storage,
    pub auth: Auth,
    pub limits: Limits,
    pub features: Features,
    pub retention: Retention,
    pub logging: Logging,
}
impl Default for FileConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            instance: Instance::default(),
            storage: Storage::default(),
            auth: Auth::default(),
            limits: Limits::default(),
            features: Features::default(),
            retention: Retention::default(),
            logging: Logging::default(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Instance {
    pub instance_id: String,
    pub origin: String,
    pub bind: String,
    pub trusted_proxy_ips: Vec<String>,
}
impl Default for Instance {
    fn default() -> Self {
        Self {
            instance_id: "self-hosted".into(),
            origin: "https://localhost".into(),
            bind: "127.0.0.1:8787".into(),
            trusted_proxy_ips: vec![],
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Storage {
    pub data_dir: PathBuf,
    pub database_path: Option<PathBuf>,
    pub object_dir: Option<PathBuf>,
}
impl Default for Storage {
    fn default() -> Self {
        Self {
            data_dir: "data".into(),
            database_path: None,
            object_dir: None,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub provider_id: String,
    pub display_name: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub session_path: Option<String>,
    #[serde(default)]
    pub has_joined_url: Option<String>,
}
fn enabled() -> bool {
    true
}
impl From<Provider> for IdentityProviderUpdate {
    fn from(p: Provider) -> Self {
        Self {
            provider_id: p.provider_id,
            display_name: p.display_name,
            base_url: p.base_url,
            enabled: p.enabled,
            session_path: p.session_path,
            has_joined_url: p.has_joined_url,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Auth {
    pub allow_self_registration: bool,
    pub bootstrap_account_id: String,
    pub bootstrap_access_token_file: Option<PathBuf>,
    pub bootstrap_password_hash_file: Option<PathBuf>,
    pub identity_providers: Vec<Provider>,
}
impl Default for Auth {
    fn default() -> Self {
        Self {
            allow_self_registration: true,
            bootstrap_account_id: "account_local".into(),
            bootstrap_access_token_file: None,
            bootstrap_password_hash_file: None,
            identity_providers: vec![],
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_asset_bytes: u64,
    pub max_message_bytes: usize,
    pub max_entity_query_count: usize,
    pub max_visual_state_bytes: usize,
    pub max_visual_variables: usize,
    pub max_projectile_snapshots_per_publisher: usize,
    pub max_projectile_snapshots_per_world: usize,
    pub visual_publish_requests_per_second: usize,
    pub visual_publish_burst: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_asset_bytes: 134217728,
            max_message_bytes: 65536,
            max_entity_query_count: 64,
            max_visual_state_bytes: 8192,
            max_visual_variables: 32,
            max_projectile_snapshots_per_publisher: 256,
            max_projectile_snapshots_per_world: 4096,
            visual_publish_requests_per_second: 20,
            visual_publish_burst: 40,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Features {
    pub entity_motion: bool,
    pub player_display_state: bool,
    pub vehicle_bindings: bool,
    pub projectile_snapshots: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Retention {
    pub projectile_idle_ttl_seconds: u64,
    pub projectile_max_lifetime_seconds: u64,
    pub cleanup_interval_seconds: u64,
}
impl Default for Retention {
    fn default() -> Self {
        Self {
            projectile_idle_ttl_seconds: 600,
            projectile_max_lifetime_seconds: 86400,
            cleanup_interval_seconds: 60,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Logging {
    pub level: String,
    pub format: String,
}
impl Default for Logging {
    fn default() -> Self {
        Self {
            level: "info".into(),
            format: "text".into(),
        }
    }
}

pub struct LoadedConfig {
    pub cloud: CloudConfig,
    pub providers: Vec<IdentityProviderUpdate>,
    pub logging: Logging,
    pub log_filter: String,
    report: Value,
}
impl LoadedConfig {
    pub fn effective_report(&self) -> &Value {
        &self.report
    }
    pub fn load(explicit: Option<&Path>) -> Result<Self, CloudError> {
        let cwd = std::env::current_dir()?;
        let environment = current_environment()?;
        let selected = select_file(explicit, &cwd, &environment)?;
        Self::load_with(selected.as_deref(), &cwd, &environment)
    }
    pub fn load_with(
        file: Option<&Path>,
        cwd: &Path,
        environment: &Environment,
    ) -> Result<Self, CloudError> {
        let file = file.map(|p| absolute(cwd, p));
        let (mut input, raw) = if let Some(path) = &file {
            let text = fs::read_to_string(path)
                .map_err(|_| invalid("config", "cannot read selected file"))?;
            let mut decoder = serde_json::Deserializer::from_str(&text);
            let input: FileConfig = serde_path_to_error::deserialize(&mut decoder)
                .map_err(|e| invalid(&format!("config.{}", e.path()), e.inner().to_string()))?;
            decoder
                .end()
                .map_err(|_| invalid("config", "unexpected trailing data"))?;
            let raw: Value =
                serde_json::from_str(&text).map_err(|_| invalid("config", "invalid JSON"))?;
            (input, raw)
        } else {
            let mut input = FileConfig::default();
            input.instance.instance_id = "local-dev".into();
            (input, json!({}))
        };
        if input.schema_version != 1 {
            return Err(invalid("schema_version", "only version 1 is supported"));
        }
        let mut sources = BTreeMap::new();
        collect_sources(
            "",
            &serde_json::to_value(&input).unwrap(),
            Some(&raw),
            &mut sources,
        );
        macro_rules! override_text {
            ($field:expr, $key:literal, $path:literal) => {
                if let Some(value) = environment.get($key) {
                    $field = value.clone();
                    sources.insert($path.into(), $key.into());
                }
            };
        }
        override_text!(
            input.instance.instance_id,
            "SPM_CLOUD_INSTANCE_ID",
            "instance.instance_id"
        );
        override_text!(input.instance.origin, "SPM_CLOUD_ORIGIN", "instance.origin");
        override_text!(input.instance.bind, "SPM_CLOUD_BIND", "instance.bind");
        override_text!(
            input.auth.bootstrap_account_id,
            "SPM_CLOUD_BOOTSTRAP_ACCOUNT",
            "auth.bootstrap_account_id"
        );
        if let Some(value) = environment.get("SPM_CLOUD_ALLOW_SELF_REGISTRATION") {
            input.auth.allow_self_registration = value.parse().map_err(|_| {
                invalid(
                    "auth.allow_self_registration",
                    "SPM_CLOUD_ALLOW_SELF_REGISTRATION must be true or false",
                )
            })?;
            sources.insert(
                "auth.allow_self_registration".into(),
                "SPM_CLOUD_ALLOW_SELF_REGISTRATION".into(),
            );
        }
        if let Some(value) = environment.get("SPM_CLOUD_TRUSTED_PROXY_IPS") {
            input.instance.trusted_proxy_ips = value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            sources.insert(
                "instance.trusted_proxy_ips".into(),
                "SPM_CLOUD_TRUSTED_PROXY_IPS".into(),
            );
        }
        for (key, path, destination) in [(
            "SPM_CLOUD_MAX_ASSET_BYTES",
            "limits.max_asset_bytes",
            &mut input.limits.max_asset_bytes,
        )] {
            if let Some(value) = environment.get(key) {
                *destination = value
                    .parse()
                    .map_err(|_| invalid(path, format!("invalid {key}")))?;
                sources.insert(path.into(), key.into());
            }
        }
        if let Some(value) = environment.get("SPM_CLOUD_MAX_MESSAGE_BYTES") {
            input.limits.max_message_bytes = value.parse().map_err(|_| {
                invalid(
                    "limits.max_message_bytes",
                    "invalid SPM_CLOUD_MAX_MESSAGE_BYTES",
                )
            })?;
            sources.insert(
                "limits.max_message_bytes".into(),
                "SPM_CLOUD_MAX_MESSAGE_BYTES".into(),
            );
        }
        validate(&input, file.is_some())?;
        validate_slug(&input.instance.instance_id, "instance.instance_id")?;
        validate_slug(
            &input.auth.bootstrap_account_id,
            "auth.bootstrap_account_id",
        )?;
        let origin = normalize_origin(&input.instance.origin)
            .map_err(|_| invalid("instance.origin", "must be a bare HTTPS origin"))?;
        let bind_addr = input
            .instance
            .bind
            .parse()
            .map_err(|_| invalid("instance.bind", "invalid SocketAddr"))?;
        let trusted_proxy_ips = input
            .instance
            .trusted_proxy_ips
            .iter()
            .map(|s| {
                s.parse()
                    .map_err(|_| invalid("instance.trusted_proxy_ips", "invalid IP address"))
            })
            .collect::<Result<_, _>>()?;
        let base = file.as_deref().and_then(Path::parent).unwrap_or(cwd);
        let data_dir = resolve_path(
            "storage.data_dir",
            "SPM_CLOUD_DATA_DIR",
            Some(&input.storage.data_dir),
            base,
            cwd,
            environment,
            &mut sources,
        )?;
        let database_path = match input
            .storage
            .database_path
            .as_deref()
            .or_else(|| environment.get("SPM_CLOUD_DATABASE").map(Path::new))
        {
            Some(_) => resolve_path(
                "storage.database_path",
                "SPM_CLOUD_DATABASE",
                input.storage.database_path.as_deref(),
                base,
                cwd,
                environment,
                &mut sources,
            )?,
            None => {
                sources.insert(
                    "storage.database_path".into(),
                    "derived from storage.data_dir".into(),
                );
                data_dir.join("spm-cloud.db")
            }
        };
        let object_dir = match input
            .storage
            .object_dir
            .as_deref()
            .or_else(|| environment.get("SPM_CLOUD_OBJECT_DIR").map(Path::new))
        {
            Some(_) => resolve_path(
                "storage.object_dir",
                "SPM_CLOUD_OBJECT_DIR",
                input.storage.object_dir.as_deref(),
                base,
                cwd,
                environment,
                &mut sources,
            )?,
            None => {
                sources.insert(
                    "storage.object_dir".into(),
                    "derived from storage.data_dir".into(),
                );
                data_dir.join("objects")
            }
        };
        check_storage(&database_path, false, "storage.database_path")?;
        check_storage(&object_dir, true, "storage.object_dir")?;
        let access_token = secret(
            "auth.bootstrap_access_token_file",
            "SPM_CLOUD_ACCESS_TOKEN",
            input.auth.bootstrap_access_token_file.as_deref(),
            base,
            environment,
            &mut sources,
        )?;
        let password_hash = secret(
            "auth.bootstrap_password_hash_file",
            "SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH",
            input.auth.bootstrap_password_hash_file.as_deref(),
            base,
            environment,
            &mut sources,
        )?;
        if let Some(token) = &access_token {
            validate_token(token)?;
        }
        if let Some(hash) = &password_hash {
            validate_password_hash(hash)?;
        }
        let provider_override = environment.contains_key("SPM_CLOUD_IDENTITY_PROVIDERS")
            || environment.contains_key("SPM_CLOUD_HAS_JOINED_URL");
        let providers = if provider_override {
            sources.insert(
                "auth.identity_providers".into(),
                "SPM_CLOUD_IDENTITY_PROVIDERS / SPM_CLOUD_HAS_JOINED_URL".into(),
            );
            crate::game_auth::providers_from_environment(
                environment
                    .get("SPM_CLOUD_IDENTITY_PROVIDERS")
                    .map(String::as_str),
                environment
                    .get("SPM_CLOUD_HAS_JOINED_URL")
                    .map(String::as_str),
            )?
        } else {
            input
                .auth
                .identity_providers
                .clone()
                .into_iter()
                .map(Into::into)
                .collect()
        };
        crate::game_auth::validate_providers(&providers)
            .map_err(|e| invalid("auth.identity_providers", e))?;
        let log_filter = environment
            .get("RUST_LOG")
            .cloned()
            .unwrap_or_else(|| input.logging.level.clone());
        tracing_subscriber::EnvFilter::try_new(&log_filter)
            .map_err(|_| invalid("logging.level", "invalid RUST_LOG filter"))?;
        sources.insert(
            "logging.filter".into(),
            if environment.contains_key("RUST_LOG") {
                "RUST_LOG".into()
            } else {
                "logging.level".into()
            },
        );
        let cloud = CloudConfig {
            instance_id: input.instance.instance_id.clone(),
            origin: origin.clone(),
            bind_addr,
            database_path: database_path.clone(),
            object_dir: object_dir.clone(),
            access_token,
            bootstrap_account_id: input.auth.bootstrap_account_id.clone(),
            bootstrap_password_hash: password_hash,
            allow_self_registration: input.auth.allow_self_registration,
            max_asset_bytes: input.limits.max_asset_bytes,
            max_message_bytes: input.limits.max_message_bytes,
            max_entity_query_count: input.limits.max_entity_query_count,
            visual: crate::visual::VisualSettings {
                features: input.features.clone(),
                limits: input.limits.clone(),
                retention: input.retention.clone(),
            },
            trusted_proxy_ips,
        };
        let mut effective = serde_json::to_value(&input).unwrap();
        effective["instance"]["origin"] = json!(origin);
        effective["storage"] =
            json!({"data_dir":data_dir,"database_path":database_path,"object_dir":object_dir});
        effective["auth"]["bootstrap_access_token_file"] = json!(if cloud.access_token.is_some() {
            Some("[REDACTED]")
        } else {
            None
        });
        effective["auth"]["bootstrap_password_hash_file"] =
            json!(if cloud.bootstrap_password_hash.is_some() {
                Some("[REDACTED]")
            } else {
                None
            });
        effective["auth"]["identity_providers"] = Value::Array(providers.iter().map(|p| json!({"provider_id":p.provider_id,"display_name":p.display_name,"base_url":p.base_url,"enabled":p.enabled,"session_path":p.session_path,"has_joined_url":p.has_joined_url})).collect());
        effective["logging"]["filter"] = json!(log_filter);
        let report = json!({"config_file":file,"effective":effective,"sources":sources,"notes":["Configured providers are upserted by ID; removing an entry does not delete database providers. Use enabled=false to disable.","player_display_state, vehicle_bindings and entity_motion are implemented and opt-in; projectile_snapshots remain unavailable. Visual limits apply to enabled visual inputs, not legacy player motion or uploads."]});
        Ok(Self {
            cloud,
            providers,
            logging: input.logging,
            log_filter,
            report,
        })
    }
}

pub fn current_environment() -> Result<Environment, CloudError> {
    let mut result = Environment::new();
    for (key, value) in std::env::vars_os() {
        if let Some(key) = key
            .to_str()
            .filter(|k| k.starts_with("SPM_CLOUD_") || *k == "RUST_LOG")
        {
            result.insert(
                key.to_owned(),
                value
                    .into_string()
                    .map_err(|_| invalid(key, "environment value is not Unicode"))?,
            );
        }
    }
    Ok(result)
}
pub fn select_file(
    explicit: Option<&Path>,
    cwd: &Path,
    env: &Environment,
) -> Result<Option<PathBuf>, CloudError> {
    if let Some(path) = explicit {
        return Ok(Some(absolute(cwd, path)));
    }
    if let Some(path) = env.get("SPM_CLOUD_CONFIG") {
        if path.is_empty() {
            return Err(invalid("SPM_CLOUD_CONFIG", "path must not be empty"));
        }
        return Ok(Some(absolute(cwd, Path::new(path))));
    }
    let default = cwd.join("config.json");
    match fs::symlink_metadata(&default) {
        Ok(_) => Ok(Some(default)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(invalid("config", "cannot inspect config.json")),
    }
}
fn invalid(field: &str, message: impl std::fmt::Display) -> CloudError {
    CloudError::configuration(format!("{field}: {message}"))
}
fn absolute(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    }
}
fn collect_sources(
    prefix: &str,
    effective: &Value,
    raw: Option<&Value>,
    sources: &mut BTreeMap<String, String>,
) {
    if let Some(object) = effective.as_object() {
        for (key, value) in object {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            collect_sources(&path, value, raw.and_then(|r| r.get(key)), sources);
        }
    } else {
        sources.insert(
            prefix.into(),
            if raw.is_some() { "JSON" } else { "default" }.into(),
        );
    }
}
fn resolve_path(
    field: &str,
    key: &str,
    file_value: Option<&Path>,
    base: &Path,
    cwd: &Path,
    env: &Environment,
    sources: &mut BTreeMap<String, String>,
) -> Result<PathBuf, CloudError> {
    let (path, root) = if let Some(value) = env.get(key) {
        sources.insert(field.into(), key.into());
        (Path::new(value), cwd)
    } else {
        (file_value.unwrap(), base)
    };
    if path.as_os_str().is_empty() {
        return Err(invalid(field, "path must not be empty"));
    }
    Ok(absolute(root, path))
}
fn check_storage(path: &Path, directory: bool, field: &str) -> Result<(), CloudError> {
    let mut candidate = Some(path);
    while let Some(current) = candidate {
        match fs::metadata(current) {
            Ok(meta) => {
                if current == path && meta.is_dir() != directory {
                    return Err(invalid(field, "wrong file/directory type"));
                }
                if current != path && !meta.is_dir() {
                    return Err(invalid(field, "parent is not a directory"));
                }
                if meta.permissions().readonly() {
                    return Err(invalid(
                        field,
                        "storage path or nearest parent is read-only",
                    ));
                }
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => candidate = current.parent(),
            Err(_) => return Err(invalid(field, "cannot inspect storage path")),
        }
    }
    Err(invalid(field, "no existing parent directory"))
}
fn secret(
    field: &str,
    key: &str,
    file: Option<&Path>,
    base: &Path,
    env: &Environment,
    sources: &mut BTreeMap<String, String>,
) -> Result<Option<String>, CloudError> {
    if let Some(value) = env.get(key) {
        sources.insert(field.into(), key.into());
        return Ok((!value.is_empty()).then(|| value.clone()));
    }
    let Some(file) = file else {
        return Ok(None);
    };
    if file.as_os_str().is_empty() {
        return Err(invalid(field, "path must not be empty"));
    }
    let path = absolute(base, file);
    let value =
        fs::read_to_string(path).map_err(|_| invalid(field, "cannot read credential file"))?;
    let value = value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(&value)
        .to_owned();
    if value.is_empty() {
        return Err(invalid(field, "credential file is empty"));
    }
    Ok(Some(value))
}
pub(crate) fn validate_password_hash(value: &str) -> Result<(), CloudError> {
    let parsed = argon2::PasswordHash::new(value)
        .map_err(|_| invalid("auth.bootstrap_password_hash_file", "invalid Argon2id hash"))?;
    if parsed.algorithm.as_str() != "argon2id"
        || parsed.version != Some(19)
        || parsed.salt.is_none()
        || parsed.hash.is_none()
        || argon2::Params::try_from(&parsed).is_err()
    {
        return Err(invalid(
            "auth.bootstrap_password_hash_file",
            "invalid Argon2id hash",
        ));
    }
    Ok(())
}
fn validate_token(value: &str) -> Result<(), CloudError> {
    let lower = value.to_ascii_lowercase();
    if value.trim().is_empty()
        || value.contains(['\r', '\n'])
        || value.chars().any(char::is_control)
        || lower.contains("change-me")
        || lower.contains("replace-me")
        || lower.contains("changeme")
        || lower.contains("your-token")
        || lower.contains("placeholder")
    {
        return Err(invalid(
            "auth.bootstrap_access_token_file",
            "empty, placeholder or invalid token",
        ));
    }
    Ok(())
}
fn validate(input: &FileConfig, file_mode: bool) -> Result<(), CloudError> {
    let limits = &input.limits;
    for (field, value, min, max) in [
        ("max_asset_bytes", limits.max_asset_bytes, 1, 4294967296),
        (
            "max_message_bytes",
            limits.max_message_bytes as u64,
            1024,
            1048576,
        ),
        (
            "max_entity_query_count",
            limits.max_entity_query_count as u64,
            1,
            256,
        ),
        (
            "max_visual_state_bytes",
            limits.max_visual_state_bytes as u64,
            1024,
            65536,
        ),
        (
            "max_visual_variables",
            limits.max_visual_variables as u64,
            0,
            128,
        ),
        (
            "max_projectile_snapshots_per_publisher",
            limits.max_projectile_snapshots_per_publisher as u64,
            1,
            4096,
        ),
        (
            "max_projectile_snapshots_per_world",
            limits.max_projectile_snapshots_per_world as u64,
            1,
            65536,
        ),
        (
            "visual_publish_requests_per_second",
            limits.visual_publish_requests_per_second as u64,
            1,
            1000,
        ),
        (
            "visual_publish_burst",
            limits.visual_publish_burst as u64,
            1,
            2000,
        ),
    ] {
        if !(min..=max).contains(&value) {
            return Err(invalid(
                &format!("limits.{field}"),
                format!("must be in {min}..={max}"),
            ));
        }
    }
    // Reserved visual budgets must not break legacy env-only deployments with small messages.
    if (file_mode
        || input.features.entity_motion
        || input.features.player_display_state
        || input.features.projectile_snapshots
        || input.features.vehicle_bindings)
        && limits.max_visual_state_bytes > limits.max_message_bytes
    {
        return Err(invalid(
            "limits.max_visual_state_bytes",
            "must not exceed max_message_bytes",
        ));
    }
    if limits.max_projectile_snapshots_per_publisher > limits.max_projectile_snapshots_per_world {
        return Err(invalid(
            "limits.max_projectile_snapshots_per_publisher",
            "must not exceed world limit",
        ));
    }
    if limits.visual_publish_burst < limits.visual_publish_requests_per_second {
        return Err(invalid(
            "limits.visual_publish_burst",
            "must be at least requests per second",
        ));
    }
    let r = &input.retention;
    for (field, value, min, max) in [
        (
            "projectile_idle_ttl_seconds",
            r.projectile_idle_ttl_seconds,
            30,
            86400,
        ),
        (
            "projectile_max_lifetime_seconds",
            r.projectile_max_lifetime_seconds,
            60,
            604800,
        ),
        (
            "cleanup_interval_seconds",
            r.cleanup_interval_seconds,
            1,
            3600,
        ),
    ] {
        if !(min..=max).contains(&value) {
            return Err(invalid(
                &format!("retention.{field}"),
                format!("must be in {min}..={max}"),
            ));
        }
    }
    if r.projectile_idle_ttl_seconds > r.projectile_max_lifetime_seconds {
        return Err(invalid(
            "retention.projectile_idle_ttl_seconds",
            "must not exceed max lifetime",
        ));
    }
    if r.cleanup_interval_seconds > r.projectile_idle_ttl_seconds {
        return Err(invalid(
            "retention.cleanup_interval_seconds",
            "must not exceed idle TTL",
        ));
    }
    for (name, value) in [("projectile_snapshots", input.features.projectile_snapshots)] {
        if value {
            return Err(invalid(
                &format!("features.{name}"),
                "capability is not implemented in this build; set false",
            ));
        }
    }
    if !["trace", "debug", "info", "warn", "error"].contains(&input.logging.level.as_str()) {
        return Err(invalid("logging.level", "use trace/debug/info/warn/error"));
    }
    if !["text", "json"].contains(&input.logging.format.as_str()) {
        return Err(invalid("logging.format", "use text/json"));
    }
    Ok(())
}

pub fn init_config(path: &Path) -> Result<(), CloudError> {
    let path = absolute(&std::env::current_dir()?, path);
    let parent = path
        .parent()
        .ok_or_else(|| invalid("config", "invalid destination"))?;
    let token_path = parent.join("secrets/bootstrap-token.txt");
    for candidate in [&path, &token_path] {
        match fs::symlink_metadata(candidate) {
            Ok(_) => {
                return Err(invalid(
                    "init-config",
                    format!("already exists: {}", candidate.display()),
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    fs::create_dir_all(token_path.parent().unwrap())?;
    let mut config = FileConfig::default();
    config.instance.origin = "https://cloud.example.com".into();
    config.auth.bootstrap_access_token_file = Some("secrets/bootstrap-token.txt".into());
    let serialized = serde_json::to_string_pretty(&config).unwrap();
    let mut config_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let result = (|| -> Result<(), CloudError> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut token_file = options.open(&token_path)?;
        let token = format!("spm_bootstrap_{}", hex::encode(rand::random::<[u8; 32]>()));
        let write_result = (|| -> std::io::Result<()> {
            writeln!(token_file, "{token}")?;
            token_file.sync_all()?;
            writeln!(config_file, "{serialized}")?;
            config_file.sync_all()
        })();
        if write_result.is_err() {
            drop(token_file);
            let _ = fs::remove_file(&token_path);
        }
        write_result?;
        Ok(())
    })();
    if result.is_err() {
        drop(config_file);
        let _ = fs::remove_file(&path);
    }
    result
}
