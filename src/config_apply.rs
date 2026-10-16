//! Shared file/API application policy. Callers hold the configuration edit lock.
use crate::{api::AppState, config::CloudConfig, config_file::LoadedConfig, error::CloudError};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub fn snapshot(config: &CloudConfig) -> Value {
    let mut value = json!({
        "schema_version":1,
        "instance":{"instance_id":config.instance_id,"origin":config.origin,"bind":config.bind_addr.to_string(),"trusted_proxy_ips":config.trusted_proxy_ips},
        "storage":{"backend":config.database_backend,"data_dir":null,"database_path":config.database_path,"object_dir":config.object_dir,"mysql_url_file":null},
        "auth":{"allow_self_registration":config.allow_self_registration,"bootstrap_account_id":config.bootstrap_account_id,"bootstrap_access_token_file":null,"bootstrap_password_hash_file":null,"identity_providers":null},
        "limits":config.visual.limits,"features":config.visual.features,"retention":config.visual.retention,
        "web_ui":config.web_ui,"moderation":config.moderation,"performance":config.performance,"updates":config.updates,"cache":config.cache,
        "logging":{"level":null,"format":null,"filter":null}
    });
    value["limits"]["max_asset_bytes"] = json!(config.max_asset_bytes);
    value["limits"]["max_message_bytes"] = json!(config.max_message_bytes);
    value["limits"]["max_entity_query_count"] = json!(config.max_entity_query_count);
    value["web_ui"]["microsoft_client_secret_file"] = Value::Null;
    value["cache"]["redis_url_file"] = Value::Null;
    resolved_performance(&mut value, config);
    value
}

pub(crate) fn resolved_performance(value: &mut Value, config: &CloudConfig) {
    if let Ok((uploads, processing)) = config.performance.resolved() {
        value["performance"]["upload_concurrency"] = json!(uploads);
        value["performance"]["model_processing_concurrency"] = json!(processing);
    }
}

pub fn leaves(value: &Value) -> BTreeMap<String, Value> {
    fn walk(value: &Value, prefix: &str, out: &mut BTreeMap<String, Value>) {
        if let Some(object) = value.as_object() {
            for (key, value) in object {
                walk(
                    value,
                    &if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    },
                    out,
                );
            }
        } else {
            out.insert(prefix.to_owned(), value.clone());
        }
    }
    let mut out = BTreeMap::new();
    walk(value, "", &mut out);
    out
}

pub fn application(path: &str) -> &'static str {
    if path == "auth.bootstrap_password_hash_file" {
        "bootstrap_only"
    } else if path.starts_with("storage.")
        || path.starts_with("logging.")
        || matches!(
            path,
            "instance.instance_id"
                | "instance.origin"
                | "instance.bind"
                | "auth.bootstrap_account_id"
        )
    {
        "restart"
    } else if path == "features.projectile_snapshots"
        || path.starts_with("retention.projectile_")
        || path.starts_with("limits.max_projectile_")
    {
        "unavailable"
    } else {
        "live"
    }
}

pub fn report(before: &Value, saved: &Value, active: &Value) -> Value {
    let old = leaves(before);
    let active = leaves(active);
    let mut applied = Vec::new();
    let mut pending = Vec::new();
    let mut unchanged = Vec::new();
    let mut bootstrap = Vec::new();
    for (path, value) in leaves(saved) {
        if path == "schema_version" {
            continue;
        }
        if old.get(&path) == Some(&value) {
            unchanged.push(path.clone());
        } else if application(&path) == "live" {
            applied.push(path.clone());
        } else if application(&path) == "bootstrap_only" {
            bootstrap.push(path.clone());
        }
        if application(&path) == "restart"
            && path != "storage.mysql_url_file"
            && active
                .get(&path)
                .is_some_and(|v| !v.is_null() && v != &value)
        {
            pending.push(path);
        }
    }
    json!({"applied_fields":applied,"pending_restart_fields":pending,"unchanged_fields":unchanged,"bootstrap_only_fields":bootstrap})
}

/// Same-directory, flushed replacement, also used by offline recovery and rollback.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), CloudError> {
    use std::io::Write;
    if std::fs::metadata(path)?.permissions().readonly() {
        return Err(CloudError::invalid_metadata(
            "config: selected file is read-only",
        ));
    }
    let mut temp = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| CloudError::invalid_metadata("config: missing parent directory"))?,
    )?;
    temp.as_file()
        .set_permissions(std::fs::metadata(path)?.permissions())?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| CloudError::Io(e.error))?;
    Ok(())
}

/// Preflight all fallible runtime validation, then publish provider/policy changes in one DB transaction.
/// A crash between file publication and DB commit is recovered by replaying the final file on startup.
pub fn apply(
    state: &AppState,
    loaded: &LoadedConfig,
    publication: Option<(&Path, &[u8])>,
    actor: Option<&str>,
) -> Result<(), CloudError> {
    let mut next = loaded.cloud.clone();
    let current = state.runtime_config();
    next.instance_id = current.instance_id;
    next.origin = current.origin;
    next.bind_addr = current.bind_addr;
    next.database_backend = current.database_backend;
    next.database_path = current.database_path;
    next.mysql_url = current.mysql_url;
    next.object_dir = current.object_dir;
    next.bootstrap_account_id = current.bootstrap_account_id;
    next.bootstrap_password_hash = current.bootstrap_password_hash;
    next.config_path = current.config_path;
    // OAuth defaults must be validated against the origin actually serving requests.
    crate::browser_auth::validate_settings(&next.web_ui, &next.origin)?;
    next.performance.resolved()?;
    next.cache.validate()?;
    next.updates.validate()?;
    let old_file = publication
        .map(|(path, _)| std::fs::read(path))
        .transpose()?;
    let mut conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let tx = conn.transaction()?;
    crate::admin_settings::reconcile_providers(&tx, &loaded.providers)?;
    tx.execute(
        "UPDATE moderation_policy SET block_r18=?1,block_unclassified=?2 WHERE id=1",
        rusqlite::params![
            next.moderation.block_r18,
            next.moderation.block_unclassified
        ],
    )?;
    if let Some(actor) = actor {
        crate::store::write_audit(&tx, actor, "CONFIG_UPDATED", None, None, None, json!({}))?;
    }
    if let Some((path, bytes)) = publication {
        atomic_write(path, bytes)?;
    }
    if let Err(error) = tx.commit() {
        if let (Some((path, _)), Some(old)) = (publication, old_file) {
            atomic_write(path, &old)?;
        }
        return Err(error.into());
    }
    drop(conn);
    state.install_runtime_config(next);
    let mut active = state
        .active_settings
        .write()
        .expect("active settings lock poisoned");
    let saved = &loaded.effective_report()["effective"];
    for group in [
        "limits",
        "features",
        "retention",
        "moderation",
        "performance",
        "updates",
        "cache",
        "web_ui",
    ] {
        active[group] = saved[group].clone();
    }
    active["instance"]["trusted_proxy_ips"] = saved["instance"]["trusted_proxy_ips"].clone();
    for field in [
        "allow_self_registration",
        "bootstrap_access_token_file",
        "identity_providers",
    ] {
        active["auth"][field] = saved["auth"][field].clone();
    }
    resolved_performance(&mut active, &state.runtime_config());
    state
        .settings_generation
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}
