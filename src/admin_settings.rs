//! Owner-only configuration drafts, managed credentials, and bounded diagnostics.
use crate::{
    admin::authorize,
    api::AppState,
    config_apply,
    config_file::{Environment, LoadedConfig},
    database::Connection,
    error::CloudError,
    models::IdentityProviderUpdate,
};
use axum::{Json, extract::State, http::HeaderMap};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

const SECRET_FIELDS: [(&str, &str, &str); 5] = [
    ("mysql", "storage.mysql_url_file", "SPM_CLOUD_MYSQL_URL"),
    ("redis", "cache.redis_url_file", "SPM_CLOUD_REDIS_URL"),
    (
        "microsoft",
        "web_ui.microsoft_client_secret_file",
        "SPM_CLOUD_MICROSOFT_CLIENT_SECRET",
    ),
    (
        "bootstrap_token",
        "auth.bootstrap_access_token_file",
        "SPM_CLOUD_ACCESS_TOKEN",
    ),
    (
        "bootstrap_password_hash",
        "auth.bootstrap_password_hash_file",
        "SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH",
    ),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigUpdate {
    config: Value,
    revision: String,
    #[serde(default)]
    managed_secrets: BTreeMap<String, SecretInput>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretInput {
    #[serde(default)]
    operation: SecretOperation,
    value: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
    database: Option<Value>,
    #[serde(default)]
    tls: bool,
}
#[derive(Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum SecretOperation {
    #[default]
    Keep,
    Replace,
    Clear,
    Generate,
}

fn field(value: &Value, path: &str) -> Value {
    let (group, key) = path.split_once('.').unwrap();
    value[group][key].clone()
}
fn set_field(value: &mut Value, path: &str, replacement: Value) -> Result<(), CloudError> {
    let (group, key) = path.split_once('.').unwrap();
    let object = value
        .as_object_mut()
        .ok_or_else(|| CloudError::invalid_metadata("config: must be an object"))?;
    let group = object
        .entry(group)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| CloudError::invalid_metadata("config: group must be an object"))?;
    group.insert(key.to_owned(), replacement);
    Ok(())
}
fn invalid(path: &str, message: &str) -> CloudError {
    CloudError::invalid_metadata(format!("{path}: {message}"))
}

fn decode_request<T>(
    input: Result<Json<T>, axum::extract::rejection::JsonRejection>,
) -> Result<T, CloudError> {
    // Serde rejection messages may quote a secret entered in a field of the wrong type.
    input.map(|Json(value)|value).map_err(|_|CloudError::SettingsValidation(vec![json!({"path":"request","message":"Invalid request shape; check field names and types"})]))
}

fn connection_url(kind: &str, input: &SecretInput) -> Result<String, CloudError> {
    let host = input
        .host
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid(kind, "host is required"))?;
    if host != host.trim() || host.contains(['/', '\\', '@', '?', '#']) {
        return Err(invalid(kind, "invalid host"));
    }
    if kind == "mysql" && input.tls {
        return Err(invalid(
            kind,
            "use an existing MySQL credential file for custom TLS options",
        ));
    }
    let scheme = if kind == "mysql" {
        "mysql"
    } else if input.tls {
        "rediss"
    } else {
        "redis"
    };
    let mut url = reqwest::Url::parse(&format!("{scheme}://localhost")).unwrap();
    url.set_host(Some(host))
        .map_err(|_| invalid(kind, "invalid host"))?;
    url.set_port(Some(input.port.unwrap_or(if kind == "mysql" {
        3306
    } else {
        6379
    })))
    .map_err(|_| invalid(kind, "invalid port"))?;
    if input.port == Some(0) {
        return Err(invalid(kind, "port must be 1..65535"));
    }
    url.set_username(input.username.as_deref().unwrap_or(""))
        .map_err(|_| invalid(kind, "invalid username"))?;
    url.set_password(input.password.as_deref())
        .map_err(|_| invalid(kind, "invalid password"))?;
    let database = if kind == "mysql" {
        input
            .database
            .as_ref()
            .and_then(Value::as_str)
            .filter(|s| {
                !s.is_empty() && !s.contains(['/', '\\']) && !s.chars().any(char::is_control)
            })
            .ok_or_else(|| invalid(kind, "database name is required"))?
            .to_owned()
    } else {
        input
            .database
            .as_ref()
            .map_or(Some(0), |v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .filter(|v| *v <= i32::MAX as u64)
            .ok_or_else(|| invalid(kind, "database must be a nonnegative integer"))?
            .to_string()
    };
    url.path_segments_mut()
        .map_err(|_| invalid(kind, "invalid URL"))?
        .clear()
        .push(&database);
    let text = url.to_string();
    if kind == "mysql" {
        mysql::Opts::from_url(&text)
            .map_err(|_| invalid(kind, "invalid MySQL connection fields"))?;
    } else {
        redis::Client::open(text.as_str())
            .map_err(|_| invalid(kind, "invalid Redis connection fields"))?;
    }
    Ok(text)
}

struct Draft {
    config: Value,
    loaded: LoadedConfig,
    files: Vec<(PathBuf, String)>,
    generated: BTreeMap<String, String>,
}
fn prepare(
    state: &AppState,
    path: &Path,
    input: ConfigUpdate,
    previous: &Value,
) -> Result<Draft, CloudError> {
    let mut config = input.config;
    let mut overrides = Environment::new();
    let mut files = Vec::new();
    let mut generated = BTreeMap::new();
    for key in input.managed_secrets.keys() {
        if !SECRET_FIELDS.iter().any(|(name, _, _)| name == key) {
            return Err(invalid("managed_secrets", "unknown secret type"));
        }
    }
    for (name, field_path, env) in SECRET_FIELDS {
        let (group, key) = field_path.split_once('.').unwrap();
        if config.get(group).and_then(|g| g.get(key)).is_none() {
            set_field(&mut config, field_path, field(previous, field_path))?;
        }
        let Some(secret) = input.managed_secrets.get(name) else {
            continue;
        };
        match secret.operation {
            SecretOperation::Keep => {
                set_field(&mut config, field_path, field(previous, field_path))?;
            }
            SecretOperation::Clear => {
                set_field(&mut config, field_path, Value::Null)?;
            }
            SecretOperation::Replace | SecretOperation::Generate => {
                let value = if secret.operation == SecretOperation::Generate {
                    if name != "bootstrap_token" {
                        return Err(invalid(
                            field_path,
                            "generation is supported only for the bootstrap token",
                        ));
                    }
                    let token = format!(
                        "{}{}",
                        uuid::Uuid::new_v4().simple(),
                        uuid::Uuid::new_v4().simple()
                    );
                    generated.insert(name.to_owned(), token.clone());
                    token
                } else if name == "mysql" || name == "redis" {
                    connection_url(name, secret)?
                } else {
                    secret.value.clone().unwrap_or_default()
                };
                // Empty input is preservation, including the original credential reference.
                if value.is_empty() {
                    set_field(&mut config, field_path, field(previous, field_path))?;
                    continue;
                }
                if value.contains(['\r', '\n']) || value.chars().any(char::is_control) {
                    return Err(invalid(field_path, "secret must be a single nonempty line"));
                }
                let relative = PathBuf::from(".managed-secrets")
                    .join(format!("{name}-{}.txt", uuid::Uuid::new_v4().simple()));
                set_field(&mut config, field_path, json!(relative))?;
                overrides.insert(env.into(), value.clone());
                files.push((path.parent().unwrap().join(relative), value));
            }
        }
    }
    let loaded =
        LoadedConfig::candidate_with_secrets(&config, path, &overrides).map_err(draft_error)?;
    if loaded.cloud.bootstrap_account_id != state.config.bootstrap_account_id {
        return Err(invalid(
            "auth.bootstrap_account_id",
            "the owner is fixed for this instance",
        ));
    }
    crate::browser_auth::validate_settings(&loaded.cloud.web_ui, &state.runtime_config().origin)
        .map_err(draft_error)?;
    Ok(Draft {
        config,
        loaded,
        files,
        generated,
    })
}
fn draft_error(error: CloudError) -> CloudError {
    match error {
        CloudError::Configuration(message) | CloudError::InvalidMetadata(message) => {
            CloudError::invalid_metadata(message.trim_start_matches("config.").to_owned())
        }
        CloudError::IdentityProviderUntrusted => invalid(
            "auth.identity_providers",
            "use a public HTTPS domain and a valid relative interface path",
        ),
        _ => invalid("config", "candidate configuration could not be validated"),
    }
}
fn field_error(error: &CloudError) -> Value {
    let detail = match error {
        CloudError::InvalidMetadata(s) => s.as_str(),
        _ => "config: validation failed",
    };
    let (path, message) = detail.split_once(": ").unwrap_or(("config", detail));
    json!({"path":path,"message":message})
}
fn revision(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn current(state: &AppState, expected: &str) -> Result<(PathBuf, Value, Vec<u8>), CloudError> {
    let path = state
        .runtime_config()
        .config_path
        .ok_or_else(|| invalid("config", "start Cloud with --config to edit settings"))?;
    let bytes = std::fs::read(&path)?;
    if revision(&bytes) != expected {
        return Err(CloudError::RevisionConflict);
    }
    let value = serde_json::from_slice(&bytes)
        .map_err(|_| invalid("config", "selected file is invalid JSON"))?;
    Ok((path, value, bytes))
}

fn comparison_settings(loaded: &LoadedConfig, raw: &Value) -> Value {
    let mut value = loaded.effective_report()["effective"].clone();
    for (_, path, _) in SECRET_FIELDS {
        let _ = set_field(&mut value, path, field(raw, path));
    }
    value
}
fn apply_report(state: &AppState, before: &Value, loaded: &LoadedConfig, raw: &Value) -> Value {
    let previous = LoadedConfig::candidate(
        before,
        state.runtime_config().config_path.as_deref().unwrap(),
    )
    .ok();
    let old = previous
        .as_ref()
        .map(|p| comparison_settings(p, before))
        .unwrap_or_else(|| before.clone());
    let saved = comparison_settings(loaded, raw);
    let active = state.active_settings.read().unwrap().clone();
    let mut result = config_apply::report(&old, &saved, &active);
    if loaded.cloud.mysql_url != state.runtime_config().mysql_url {
        let fields = result["pending_restart_fields"].as_array_mut().unwrap();
        if !fields.contains(&json!("storage.mysql_url_file")) {
            fields.push(json!("storage.mysql_url_file"));
        }
    }
    result
}

pub async fn validate(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: Result<Json<ConfigUpdate>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Value>, CloudError> {
    authorize(&state, &headers, true)?;
    let input = decode_request(input)?;
    let _lock = state.config_edit_guard().await;
    let (path, before, _) = current(&state, &input.revision)?;
    match prepare(&state, &path, input, &before) {
        Ok(draft) => {
            let mut report = apply_report(&state, &before, &draft.loaded, &draft.config);
            report["valid"] = json!(true);
            report["errors"] = json!([]);
            Ok(Json(report))
        }
        Err(error) => Ok(Json(json!({"valid":false,"errors":[field_error(&error)]}))),
    }
}

struct Staged(Vec<PathBuf>);
impl Drop for Staged {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}
fn stage(files: &[(PathBuf, String)]) -> Result<Staged, CloudError> {
    use std::io::Write;
    let mut staged = Staged(Vec::new());
    for (path, value) in files {
        let directory = path.parent().unwrap();
        if let Ok(meta) = std::fs::symlink_metadata(directory) {
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err(invalid(
                    "managed_secrets",
                    "managed secret directory must be a real directory",
                ));
            }
        } else {
            let builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = builder;
                builder.mode(0o700);
                builder
            };
            builder.create(directory)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        staged.0.push(path.clone());
        file.write_all(value.as_bytes())?;
        file.sync_all()?;
    }
    Ok(staged)
}
pub async fn save(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: Result<Json<ConfigUpdate>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Value>, CloudError> {
    let (actor, _) = authorize(&state, &headers, true)?;
    let input = decode_request(input)?;
    let _lock = state.config_edit_guard().await;
    let (path, before, original) = current(&state, &input.revision)?;
    let draft = prepare(&state, &path, input, &before)
        .map_err(|error| CloudError::SettingsValidation(vec![field_error(&error)]))?;
    let mut report = apply_report(&state, &before, &draft.loaded, &draft.config);
    let mut staged = stage(&draft.files).map_err(|_|CloudError::SettingsValidation(vec![json!({"path":"managed_secrets","message":"Cannot create credential files in the managed secret directory"})]))?;
    let bytes = serde_json::to_vec_pretty(&draft.config).unwrap();
    // Detect an external editor racing our preflight before publishing any configuration.
    if std::fs::read(&path)? != original {
        return Err(CloudError::RevisionConflict);
    }
    config_apply::apply(&state, &draft.loaded, Some((&path, &bytes)), Some(&actor)).map_err(|_|CloudError::SettingsValidation(vec![json!({"path":"config","message":"Configuration publication failed; check the selected configuration file and retry"})]))?;
    staged.0.clear();
    report["saved"] = json!(true);
    report["revision"] = json!(revision(&bytes));
    report["generated_secrets"] = json!(draft.generated);
    Ok(Json(report))
}

pub(crate) fn upsert_provider(
    conn: &Connection,
    input: &IdentityProviderUpdate,
) -> Result<Value, CloudError> {
    crate::game_auth::validate_providers(std::slice::from_ref(input))?;
    let (base, path) = crate::game_auth::provider_endpoint(input)?;
    conn.execute("INSERT INTO identity_providers(provider_id,display_name,base_url,enabled,session_path,authenticate_path) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(provider_id) DO UPDATE SET display_name=excluded.display_name,base_url=excluded.base_url,enabled=excluded.enabled,session_path=excluded.session_path,authenticate_path=excluded.authenticate_path",rusqlite::params![input.provider_id,input.display_name,base,input.enabled,path,input.authenticate_path])?;
    Ok(
        json!({"provider_id":input.provider_id,"display_name":input.display_name,"base_url":base,"enabled":input.enabled,"session_path":path,"authenticate_path":input.authenticate_path}),
    )
}
pub(crate) fn reconcile_providers(
    conn: &Connection,
    providers: &[IdentityProviderUpdate],
) -> Result<(), CloudError> {
    crate::game_auth::validate_providers(providers)?;
    let mut query = conn.prepare("SELECT provider_id FROM config_managed_providers")?;
    let ids = query
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        if !providers.iter().any(|p| p.provider_id == id) {
            conn.execute("UPDATE identity_providers SET enabled=0 WHERE provider_id=?1 AND provider_id<>'official'",[id])?;
        }
    }
    for provider in providers {
        upsert_provider(conn, provider)?;
        conn.execute(
            "INSERT OR IGNORE INTO config_managed_providers(provider_id) VALUES(?1)",
            [&provider.provider_id],
        )?;
    }
    Ok(())
}
fn provider_entries(
    store: &crate::CloudStore,
    configured: &Value,
    writable: bool,
) -> Result<Vec<Value>, CloudError> {
    let conn = store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let mut stmt=conn.prepare("SELECT p.provider_id,p.display_name,p.base_url,p.enabled,p.session_path,p.authenticate_path,m.provider_id IS NOT NULL FROM identity_providers p LEFT JOIN config_managed_providers m ON p.provider_id=m.provider_id ORDER BY p.provider_id")?;
    let entries=stmt.query_map([],|r|Ok(json!({"provider_id":r.get::<_,String>(0)?,"display_name":r.get::<_,String>(1)?,"base_url":r.get::<_,String>(2)?,"enabled":r.get::<_,bool>(3)?,"session_path":r.get::<_,String>(4)?,"authenticate_path":r.get::<_,Option<String>>(5)?,"source":if r.get::<_,bool>(6)? {"managed"} else {"database"}})))?.collect::<Result<Vec<_>,_>>()?;
    let mut entries = entries;
    if let Some(providers) = configured["auth"]["identity_providers"].as_array() {
        for provider in providers {
            if !entries
                .iter()
                .any(|p| p["provider_id"] == provider["provider_id"])
            {
                let mut entry = provider.clone();
                entry["active_enabled"] = Value::Null;
                entries.push(entry);
            }
        }
    }
    Ok(entries
        .into_iter()
        .map(|mut entry| {
            if entry["provider_id"] == "official" {
                entry["source"] = json!("official");
                entry["editable"] = json!(false);
            } else {
                entry["editable"] = json!(writable);
            }
            if entry.get("active_enabled").is_none() {
                entry["active_enabled"] = entry["enabled"].clone();
            }
            entry["has_joined_url"] = Value::Null;
            if let Some(p) = configured["auth"]["identity_providers"]
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .find(|p| p["provider_id"] == entry["provider_id"])
                })
            {
                let provider: crate::config_file::Provider =
                    serde_json::from_value(p.clone()).expect("validated provider");
                entry.as_object_mut().unwrap().extend(
                    serde_json::to_value(provider)
                        .unwrap()
                        .as_object()
                        .unwrap()
                        .clone(),
                );
                entry["source"] = json!("config");
            } else if !configured.is_null() && entry["source"] == "managed" {
                entry["enabled"] = json!(false);
            }
            entry
        })
        .collect())
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, CloudError> {
    let (_, role) = authorize(&state, &headers, false)?;
    let _lock = state.config_edit_guard().await;
    let runtime = state.runtime_config();
    let mut active = state.active_settings.read().unwrap().clone();
    active["auth"]["identity_providers"] =
        json!(provider_entries(&state.store, &Value::Null, false)?);
    let (config, revision, effective, loaded) = if let Some(path) = &runtime.config_path {
        let bytes = std::fs::read(path)?;
        let raw: Value = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("config", "invalid selected JSON"))?;
        let loaded = LoadedConfig::candidate(&raw, path).map_err(draft_error)?;
        let effective = loaded.effective_report().clone();
        (raw, json!(revision(&bytes)), effective, Some(loaded))
    } else {
        (
            Value::Null,
            Value::Null,
            json!({"effective":active,"config_file":null,"sources":{}}),
            None,
        )
    };
    let saved = loaded
        .as_ref()
        .map(|l| comparison_settings(l, &config))
        .unwrap_or_else(|| active.clone());
    let pending = loaded
        .as_ref()
        .map(|loaded| {
            apply_report(&state, &config, loaded, &config)["pending_restart_fields"].clone()
        })
        .unwrap_or_else(|| json!([]));
    let mut statuses = BTreeMap::new();
    for path in config_apply::leaves(&saved)
        .keys()
        .filter(|p| p.as_str() != "schema_version")
    {
        let apply = config_apply::application(path);
        let editable = role == "owner"
            && runtime.config_path.is_some()
            && path != "auth.bootstrap_account_id"
            && path != "logging.filter";
        let reason = if runtime.config_path.is_none() {
            Some("environment_mode")
        } else if role != "owner" {
            Some("owner_only")
        } else if path == "auth.bootstrap_account_id" {
            Some("fixed_owner")
        } else if path == "logging.filter" {
            Some("derived_value")
        } else {
            None
        };
        statuses.insert(
            path.clone(),
            json!({"editable":editable,"apply":apply,"reason":reason,"active_state":if matches!(apply,"bootstrap_only"){"not_applicable"}else if matches!(path.as_str(),"logging.level"|"logging.format"|"logging.filter"|"storage.data_dir") && field(&active,path).is_null(){"unknown"}else{"known"}}),
        );
    }
    let recovery = runtime.config_path.as_ref().map(|path| {
        format!(
            "spm-cloud --config {} --enable-web-ui",
            quote_argument(&path.to_string_lossy())
        )
    });
    Ok(Json(
        json!({"config":config,"revision":revision,"effective":effective,"active":active,"pending_restart_fields":pending,"field_status":statuses,"secret_status":secret_status(loaded.as_ref().map(|l|&l.cloud).unwrap_or(&runtime)),"active_secret_status":secret_status(&runtime),"provider_admin_entries":provider_entries(&state.store,&config,role=="owner" && runtime.config_path.is_some())?,"capabilities":{"native_update_supported":crate::maintenance::native_supported(),"web_ui_recovery_command":recovery,"database_migration":"sqlite_to_empty_mysql_only"},"writable":role=="owner" && runtime.config_path.is_some(),"schema":serde_json::from_str::<Value>(include_str!("../config.schema.json")).unwrap(),"restart_fields":["instance.instance_id","instance.origin","instance.bind","storage","logging"]}),
    ))
}
fn quote_argument(value: &str) -> String {
    #[cfg(windows)]
    {
        format!("'{}'", value.replace('\'', "''"))
    }
    #[cfg(not(windows))]
    {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn secret_status(cloud: &crate::CloudConfig) -> Value {
    let mysql=cloud.mysql_url.as_deref().and_then(|s|mysql::Opts::from_url(s).ok()).map(|opts|json!({"host":opts.get_ip_or_hostname(),"port":opts.get_tcp_port(),"database":opts.get_db_name(),"tls":opts.get_ssl_opts().is_some()}));
    let redis=cloud.cache.redis_url.as_deref().and_then(|s|reqwest::Url::parse(s).ok()).map(|url|json!({"host":url.host_str(),"port":url.port().unwrap_or(6379),"database":url.path().trim_start_matches('/'),"tls":url.scheme()=="rediss"}));
    json!({"mysql":{"configured":cloud.mysql_url.is_some(),"connection":mysql},"redis":{"configured":cloud.cache.redis_url.is_some(),"connection":redis},"microsoft":{"configured":cloud.microsoft_client_secret.is_some() || cloud.web_ui.microsoft_client_secret_file.is_some()},"bootstrap_token":{"configured":cloud.access_token.is_some()},"bootstrap_password_hash":{"configured":cloud.bootstrap_password_hash.is_some(),"apply":"bootstrap_only"}})
}

async fn bounded_probe(
    client: &reqwest::Client,
    url: reqwest::Url,
) -> Result<(u16, bool), &'static str> {
    use futures_util::StreamExt;
    let response = client
        .get(url)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(http_failure)?;
    let status = response.status();
    if status.is_redirection() {
        return Err("redirect_rejected");
    }
    if response.content_length().is_some_and(|n| n > 65536) {
        return Err("response_too_large");
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(http_failure)?;
        if bytes.len() + chunk.len() > 65536 {
            return Err("response_too_large");
        }
        bytes.extend_from_slice(&chunk);
    }
    let recognized = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .is_some_and(|v| {
            (status.is_success() && v["meta"].is_object())
                || (matches!(status.as_u16(), 400 | 401 | 403 | 405) && v["error"].is_string())
        });
    Ok((status.as_u16(), recognized))
}
fn http_failure(error: reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connection_or_tls_failed"
    } else {
        "protocol_failed"
    }
}
async fn probe_provider_interfaces(
    client: &reqwest::Client,
    url: reqwest::Url,
    provider: &IdentityProviderUpdate,
    path: &str,
) -> Result<Value, &'static str> {
    let (status, recognized) = bounded_probe(client, url.clone()).await?;
    let mut session = url.clone();
    session.set_path(&format!("{}{}", session.path().trim_end_matches('/'), path));
    let (session_status, session_recognized) = bounded_probe(client, session).await?;
    // Match the actual browser login client's backwards-compatible path selection.
    let authenticate_path =
        provider
            .authenticate_path
            .as_deref()
            .unwrap_or(if path == "/session/hasJoined" {
                "/auth/authenticate"
            } else {
                "/authserver/authenticate"
            });
    let mut authenticate = url;
    authenticate.set_path(&format!(
        "{}{}",
        authenticate.path().trim_end_matches('/'),
        authenticate_path
    ));
    // GET contains no account credentials and cannot create a login or binding.
    let (authenticate_status, authenticate_recognized) =
        bounded_probe(client, authenticate).await?;
    let metadata_recognized = (200..300).contains(&status) && recognized;
    Ok(
        json!({"valid":true,"reachable":true,"metadata_status":status,"metadata_recognized":metadata_recognized,"session_status":session_status,"session_recognized":session_recognized,"authenticate_path":authenticate_path,"authenticate_status":authenticate_status,"authenticate_recognized":authenticate_recognized,"interfaces_recognized":metadata_recognized || session_recognized || authenticate_recognized,"login_verified":false,"message":"服务可连接，登录尚未验证"}),
    )
}
pub async fn provider_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: Result<Json<crate::config_file::Provider>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Value>, CloudError> {
    authorize(&state, &headers, true)?;
    let input = decode_request(input)?;
    let _permit = state
        .diagnostic_slots
        .try_acquire()
        .map_err(|_| CloudError::RateLimited)?;
    let provider: IdentityProviderUpdate = input.into();
    let result = async {
        crate::game_auth::validate_providers(std::slice::from_ref(&provider))
            .map_err(|_| "invalid_provider")?;
        let (base, path) =
            crate::game_auth::provider_endpoint(&provider).map_err(|_| "invalid_provider")?;
        let (client, url) = crate::game_auth::provider_client(&base, Duration::from_secs(5))
            .await
            .map_err(|e| {
                if matches!(e, CloudError::IdentityProviderUntrusted) {
                    "public_dns_required"
                } else {
                    "dns_or_connection_failed"
                }
            })?;
        probe_provider_interfaces(&client, url, &provider, &path).await
    };
    let result = tokio::time::timeout(Duration::from_secs(12), result)
        .await
        .unwrap_or(Err("timeout"));
    Ok(Json(match result {
        Ok(report) => report,
        Err(reason) => {
            json!({"valid":!matches!(reason,"invalid_provider"|"public_dns_required"),"reachable":false,"login_verified":false,"reason":reason})
        }
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionCheck {
    kind: String,
    connection: Option<SecretInput>,
    scanner_url: Option<String>,
    draft: Option<ConfigUpdate>,
}
pub async fn check_connection(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: Result<Json<ConnectionCheck>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Value>, CloudError> {
    authorize(&state, &headers, true)?;
    let input = decode_request(input)?;
    let permit = state
        .diagnostic_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| CloudError::RateLimited)?;
    let mut runtime = state.runtime_config();
    let kind = input.kind.as_str();
    let current_backend = runtime.database_backend.clone();
    if !["mysql", "redis", "nudenet"].contains(&kind) {
        return Err(invalid("kind", "choose mysql, redis or nudenet"));
    }
    if let Some(draft) = input.draft {
        let _lock = state.config_edit_guard().await;
        let (path, before, _) = current(&state, &draft.revision)?;
        runtime = prepare(&state, &path, draft, &before)
            .map_err(|error| CloudError::SettingsValidation(vec![field_error(&error)]))?
            .loaded
            .cloud;
    }
    let url = if kind == "nudenet" {
        input
            .scanner_url
            .or(runtime.moderation.scanner_url)
            .ok_or_else(|| invalid("moderation.scanner_url", "scanner address is required"))?
    } else if let Some(connection) = &input.connection {
        connection_url(kind, connection)?
    } else if kind == "mysql" {
        runtime
            .mysql_url
            .ok_or_else(|| invalid("storage.mysql_url_file", "no configured MySQL connection"))?
    } else {
        runtime
            .cache
            .redis_url
            .ok_or_else(|| invalid("cache.redis_url_file", "no configured Redis connection"))?
    };
    let check = async {
        match kind {
            "mysql" => {
                // No CloudStore::open: this path must never create business tables or import data.
                tokio::task::spawn_blocking(move || {
                    use mysql::prelude::Queryable;
                    let _permit = permit;
                    let opts = mysql::Opts::from_url(&url).map_err(|_| "invalid_address")?;
                    let opts = mysql::OptsBuilder::from_opts(opts)
                        .tcp_connect_timeout(Some(Duration::from_secs(3)))
                        .read_timeout(Some(Duration::from_secs(3)))
                        .write_timeout(Some(Duration::from_secs(3)));
                    let mut conn = mysql::Conn::new(opts).map_err(mysql_failure)?;
                    conn.query_drop("SELECT 1").map_err(mysql_failure)?;
                    let tables:Vec<String>=conn.query("SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME IN ('accounts','assets')").map_err(mysql_failure)?;
                    let accounts:Option<u64>=if tables.iter().any(|t|t=="accounts") {conn.query_first("SELECT COUNT(*) FROM accounts").map_err(mysql_failure)?}else{None};
                    let assets:Option<u64>=if tables.iter().any(|t|t=="assets") {conn.query_first("SELECT COUNT(*) FROM assets").map_err(mysql_failure)?}else{None};
                    let eligible=current_backend=="sqlite" && accounts.unwrap_or(0)==0 && assets.unwrap_or(0)==0;
                    Ok::<_, &'static str>(
                        json!({"stage":"query","target":{"accounts":accounts,"assets":assets,"sqlite_import_eligible":eligible},"requires_restart":true,"message":"MySQL 可连接；未创建表或迁移数据。仅支持现有 SQLite 导入空 MySQL，请先备份再保存并重启"}),
                    )
                })
                .await
                .map_err(|_| "worker_failed")?
            }
            "redis" => {
                let _permit = permit;
                let client = redis::Client::open(url.as_str()).map_err(|_| "invalid_address")?;
                let mut conn = client
                    .get_multiplexed_async_connection()
                    .await
                    .map_err(redis_failure)?;
                let response: String = redis::cmd("PING")
                    .query_async(&mut conn)
                    .await
                    .map_err(redis_failure)?;
                if response != "PONG" {
                    return Err("protocol_failed");
                }
                Ok(json!({"stage":"ping","message":"Redis 可连接；未写入缓存"}))
            }
            _ => {
                let _permit = permit;
                let settings = crate::moderation::ModerationSettings {
                    scanner_url: Some(url.clone()),
                    ..Default::default()
                };
                settings.validate().map_err(|_| "invalid_address")?;
                let client = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .no_proxy()
                    .timeout(Duration::from_secs(4))
                    .build()
                    .map_err(|_| "client_failed")?;
                let (status, _) = bounded_probe(
                    &client,
                    reqwest::Url::parse(&url).map_err(|_| "invalid_address")?,
                )
                .await?;
                Ok(
                    json!({"connected":(200..300).contains(&status),"reachable":true,"stage":"http","http_status":status,"inference_verified":false,"message":"扫描服务已响应，尚未验证图片识别"}),
                )
            }
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(8), check)
        .await
        .unwrap_or(Err("timeout"));
    Ok(Json(match result {
        Ok(mut value) => {
            if value.get("connected").is_none() {
                value["connected"] = json!(true);
            }
            value
        }
        Err(reason) => json!({"connected":false,"reason":reason}),
    }))
}
fn mysql_failure(error: mysql::Error) -> &'static str {
    match error {
        mysql::Error::MySqlError(e) if e.code == 1045 => "authentication_failed",
        mysql::Error::MySqlError(e) if e.code == 1049 => "database_not_found",
        _ => "connection_or_protocol_failed",
    }
}
fn redis_failure(error: redis::RedisError) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.kind() == redis::ErrorKind::AuthenticationFailed {
        "authentication_failed"
    } else {
        "connection_or_protocol_failed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn microsoft_environment_secret_status_is_configured_and_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let env = Environment::from([
            (
                "SPM_CLOUD_ACCESS_TOKEN".into(),
                "microsoft-status-owner-token".into(),
            ),
            (
                "SPM_CLOUD_MICROSOFT_CLIENT_SECRET".into(),
                "hidden-environment-microsoft-secret".into(),
            ),
        ]);
        let loaded = LoadedConfig::load_with(None, dir.path(), &env).unwrap();
        let store = crate::CloudStore::open(&loaded.cloud).unwrap();
        let state = AppState::new(loaded.cloud.clone(), store);
        state.initialize_settings(&loaded);
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            "Bearer microsoft-status-owner-token".parse().unwrap(),
        );
        let Json(result) = get(State(state), headers).await.unwrap();
        assert_eq!(result["secret_status"]["microsoft"]["configured"], true);
        assert_eq!(
            result["active_secret_status"]["microsoft"]["configured"],
            true
        );
        assert_eq!(result["writable"], false);
        assert!(
            !result
                .to_string()
                .contains("hidden-environment-microsoft-secret")
        );
    }

    #[tokio::test]
    async fn provider_check_reports_custom_and_compatible_authentication_interfaces() {
        use axum::{Router, http::StatusCode, routing::get};
        let app = Router::new()
            .route(
                "/api",
                get(|| async { (StatusCode::NOT_FOUND, Json(json!({"error":"not found"}))) }),
            )
            .route(
                "/api/custom/authenticate",
                get(|| async {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error":"IllegalArgumentException"})),
                    )
                }),
            )
            .route(
                "/api/auth/authenticate",
                get(|| async {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error":"IllegalArgumentException"})),
                    )
                }),
            )
            .route(
                "/api/authserver/authenticate",
                get(|| async {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error":"IllegalArgumentException"})),
                    )
                }),
            )
            .fallback(|| async { (StatusCode::NOT_FOUND, Json(json!({"error":"not found"}))) });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url =
            reqwest::Url::parse(&format!("http://{}/api", listener.local_addr().unwrap())).unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        for (session_path, custom, expected) in [
            (
                "/custom/hasJoined",
                Some("/custom/authenticate"),
                "/custom/authenticate",
            ),
            ("/session/hasJoined", None, "/auth/authenticate"),
            (
                "/sessionserver/session/minecraft/hasJoined",
                None,
                "/authserver/authenticate",
            ),
        ] {
            let provider: IdentityProviderUpdate = serde_json::from_value(json!({"provider_id":"candidate","display_name":"Candidate","base_url":"https://example.com/api","enabled":true,"authenticate_path":custom})).unwrap();
            let report = probe_provider_interfaces(&client, url.clone(), &provider, session_path)
                .await
                .unwrap();
            assert_eq!(report["authenticate_status"], 400, "{report}");
            assert_eq!(report["authenticate_path"], expected);
            assert_eq!(report["authenticate_recognized"], true);
            assert_eq!(report["metadata_recognized"], false);
            assert_eq!(report["session_recognized"], false);
            assert_eq!(report["login_verified"], false);
            assert_eq!(report["reachable"], true);
        }
        let unknown: IdentityProviderUpdate = serde_json::from_value(json!({"provider_id":"unknown","display_name":"Unknown","base_url":"https://example.com/api","enabled":true,"authenticate_path":"/unknown/authenticate"})).unwrap();
        let report = probe_provider_interfaces(&client, url, &unknown, "/unknown/hasJoined")
            .await
            .unwrap();
        assert_eq!(
            report["reachable"], true,
            "404 does not prove the provider is unreachable"
        );
        assert_eq!(report["interfaces_recognized"], false);
        assert_eq!(report["login_verified"], false);
        task.abort();
    }

    #[test]
    fn managed_secret_structured_urls_preserve_special_characters() {
        let fields:SecretInput=serde_json::from_value(json!({"host":"db.example.com","port":3307,"username":"u@:/","password":"p@:/?#%汉字","database":"cloud data"})).unwrap();
        let url = connection_url("mysql", &fields).unwrap();
        let opts = mysql::Opts::from_url(&url).unwrap();
        assert_eq!(opts.get_user(), Some("u@:/"));
        assert_eq!(opts.get_pass(), Some("p@:/?#%汉字"));
        assert_eq!(opts.get_db_name(), Some("cloud data"));
        assert_eq!(opts.get_tcp_port(), 3307);
        let (state, _dir) = crate::api::tests::test_state(true);
        let mut config = state.runtime_config();
        config.mysql_url = Some(url);
        let status = secret_status(&config).to_string();
        assert!(!status.contains("u@"));
        assert!(!status.contains("汉字"));
        assert!(!status.contains("p@"));
        assert_eq!(
            secret_status(&config)["mysql"]["connection"]["database"],
            "cloud data"
        );
    }

    #[tokio::test]
    async fn provider_check_transport_bounds_responses_and_never_follows_redirects() {
        use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let hits = Arc::new(AtomicUsize::new(0));
        let visits = hits.clone();
        let app = Router::new()
            .route("/", get(|| async { StatusCode::NOT_FOUND }))
            .route(
                "/redirect",
                get(|| async {
                    (StatusCode::FOUND, [("location", "/target")], "redirect").into_response()
                }),
            )
            .route(
                "/target",
                get(move || {
                    let visits = visits.clone();
                    async move {
                        visits.fetch_add(1, Ordering::Relaxed);
                        "target"
                    }
                }),
            )
            .route("/large", get(|| async { "x".repeat(65537) }))
            .route(
                "/chunked",
                get(|| async {
                    axum::body::Body::from_stream(futures_util::stream::iter([
                        Ok::<_, std::io::Error>(vec![b'x'; 32768]),
                        Ok(vec![b'x'; 32769]),
                    ]))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        assert_eq!(
            bounded_probe(&client, reqwest::Url::parse(&base).unwrap())
                .await
                .unwrap(),
            (404, false)
        );
        for (path, reason) in [
            ("redirect", "redirect_rejected"),
            ("large", "response_too_large"),
            ("chunked", "response_too_large"),
        ] {
            assert_eq!(
                bounded_probe(
                    &client,
                    reqwest::Url::parse(&format!("{base}/{path}")).unwrap()
                )
                .await
                .unwrap_err(),
                reason
            );
        }
        assert_eq!(hits.load(Ordering::Relaxed), 0);
        task.abort();
    }
}
