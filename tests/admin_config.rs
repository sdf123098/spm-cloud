use serde_json::{Value, json};
use spm_cloud::config_file::{Environment, FileConfig, LoadedConfig, init_config, select_file};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn load(
    value: Value,
    env: Environment,
) -> (
    tempfile::TempDir,
    Result<LoadedConfig, spm_cloud::error::CloudError>,
) {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("config");
    fs::create_dir(&config_dir).unwrap();
    let file = config_dir.join("config.json");
    fs::write(&file, value.to_string()).unwrap();
    let result = LoadedConfig::load_with(Some(&file), dir.path(), &env);
    (dir, result)
}
fn cli_command(cwd: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_spm-cloud"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("SPM_CLOUD_") || key == "RUST_LOG" {
            command.env_remove(key);
        }
    }
    command.current_dir(cwd);
    command
}
fn cli(args: &[&std::ffi::OsStr], cwd: &Path) -> Output {
    cli_command(cwd).args(args).output().unwrap()
}

#[test]
fn file_paths_and_env_paths_use_distinct_roots_and_report_sources() {
    let (dir, loaded) = load(
        json!({"storage":{"database_path":"db.sqlite","object_dir":"objects"}}),
        Environment::new(),
    );
    let loaded = loaded.unwrap();
    assert_eq!(
        loaded.cloud.database_path,
        dir.path().join("config/db.sqlite")
    );
    assert_eq!(loaded.cloud.object_dir, dir.path().join("config/objects"));
    assert!(!loaded.cloud.database_path.exists());
    let mut env = Environment::new();
    env.insert("SPM_CLOUD_DATA_DIR".into(), "env-data".into());
    env.insert("SPM_CLOUD_OBJECT_DIR".into(), "env-objects".into());
    env.insert("SPM_CLOUD_MAX_ASSET_BYTES".into(), "321".into());
    env.insert("SPM_CLOUD_ALLOW_SELF_REGISTRATION".into(), "false".into());
    let (dir, loaded) = load(json!({}), env);
    let loaded = loaded.unwrap();
    assert_eq!(
        loaded.cloud.database_path,
        dir.path().join("env-data/spm-cloud.db")
    );
    assert_eq!(loaded.cloud.object_dir, dir.path().join("env-objects"));
    assert_eq!(loaded.cloud.max_asset_bytes, 321);
    assert!(!loaded.cloud.allow_self_registration);
    assert_eq!(
        loaded.effective_report()["sources"]["limits.max_asset_bytes"],
        "SPM_CLOUD_MAX_ASSET_BYTES"
    );
}

#[test]
fn upload_limit_defaults_to_128_mib_and_json_value_reaches_runtime_config() {
    let dir = tempfile::tempdir().unwrap();
    let defaults = LoadedConfig::load_with(None, dir.path(), &Environment::new()).unwrap();
    assert_eq!(defaults.cloud.max_asset_bytes, 128 * 1024 * 1024);

    let (dir, loaded) = load(
        json!({"limits":{"max_asset_bytes":67108864}}),
        Environment::new(),
    );
    let loaded = loaded.unwrap();
    assert_eq!(loaded.cloud.max_asset_bytes, 64 * 1024 * 1024);
    assert_eq!(
        loaded.effective_report()["effective"]["limits"]["max_asset_bytes"],
        67108864
    );
    drop(dir);
}

#[test]
fn explicit_file_paths_are_independent_of_data_dir() {
    let (dir, loaded) = load(
        json!({"storage":{"data_dir":"data","database_path":"data/spm-cloud.db","object_dir":"data/objects"}}),
        Environment::new(),
    );
    let loaded = loaded.unwrap();
    assert_eq!(
        loaded.cloud.database_path,
        dir.path().join("config/data/spm-cloud.db")
    );
    assert_eq!(
        loaded.cloud.object_dir,
        dir.path().join("config/data/objects")
    );
}

#[test]
fn strict_json_rejects_unknown_duplicate_wrong_types_versions_and_trailing_data() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.json");
    for text in [
        r#"{"schema_version":2}"#,
        r#"{"limits":{"max_assset_bytes":42}}"#,
        r#"{"limits":{"max_asset_bytes":1,"max_asset_bytes":2}}"#,
        r#"{"instance":{},"instance":{}}"#,
        r#"{"auth":{"allow_self_registration":"true"}}"#,
        r#"{"limits":{"max_asset_bytes":1.5}}"#,
        r#"{"limits":{"max_asset_bytes":-1}}"#,
        r#"{"auth":{"identity_providers":[{"provider_id":"custom","display_name":"Auth","has_joined_url":"https://auth.example.com/hasJoined","extra":1}]}}"#,
        "{} {}",
    ] {
        fs::write(&file, text).unwrap();
        assert!(
            LoadedConfig::load_with(Some(&file), dir.path(), &Environment::new()).is_err(),
            "accepted {text}"
        );
    }
    fs::write(&file, r#"{"limits":{"max_asset_bytes":"bad"}}"#).unwrap();
    let error = LoadedConfig::load_with(Some(&file), dir.path(), &Environment::new())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("limits.max_asset_bytes"), "{error}");
}

#[test]
fn semantic_limits_and_unimplemented_capabilities_are_rejected() {
    for value in [
        json!({"limits":{"max_entity_query_count":257}}),
        json!({"limits":{"max_visual_variables":129}}),
        json!({"limits":{"max_message_bytes":1024}}),
        json!({"limits":{"max_projectile_snapshots_per_publisher":500,"max_projectile_snapshots_per_world":400}}),
        json!({"limits":{"visual_publish_burst":10}}),
        json!({"retention":{"projectile_idle_ttl_seconds":100,"projectile_max_lifetime_seconds":60}}),
        json!({"retention":{"cleanup_interval_seconds":601}}),
        json!({"features":{"projectile_snapshots":true}}),
        json!({"logging":{"format":"xml"}}),
        json!({"logging":{"level":"verbose"}}),
    ] {
        assert!(load(value, Environment::new()).1.is_err());
    }
    let mut env = Environment::new();
    env.insert("SPM_CLOUD_MAX_ASSET_BYTES".into(), "invalid".into());
    assert!(load(json!({}), env).1.is_err());
    let dir = tempfile::tempdir().unwrap();
    let env = Environment::from([("SPM_CLOUD_MAX_MESSAGE_BYTES".into(), "1024".into())]);
    // Existing environment-only 1KiB deployments remain valid with visual features off.
    assert_eq!(
        LoadedConfig::load_with(None, dir.path(), &env)
            .unwrap()
            .cloud
            .max_message_bytes,
        1024
    );
}

#[test]
fn credential_files_strip_one_newline_and_overrides_do_not_read_missing_files() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.json");
    let token = dir.path().join("token.txt");
    fs::write(
        &file,
        r#"{"auth":{"bootstrap_access_token_file":"token.txt"}}"#,
    )
    .unwrap();
    fs::write(&token, "private token with spaces\r\n").unwrap();
    let loaded = LoadedConfig::load_with(Some(&file), dir.path(), &Environment::new()).unwrap();
    assert_eq!(
        loaded.cloud.access_token.as_deref(),
        Some("private token with spaces")
    );
    assert!(
        !loaded
            .effective_report()
            .to_string()
            .contains("private token")
    );
    assert!(!format!("{:?}", loaded.cloud).contains("private token"));
    for token_value in ["", "replace-me\n", "token\n\n", "   \n"] {
        fs::write(&token, token_value).unwrap();
        assert!(LoadedConfig::load_with(Some(&file), dir.path(), &Environment::new()).is_err());
    }
    fs::remove_file(&token).unwrap();
    let env = Environment::from([("SPM_CLOUD_ACCESS_TOKEN".into(), "environment-secret".into())]);
    let loaded = LoadedConfig::load_with(Some(&file), dir.path(), &env).unwrap();
    assert_eq!(
        loaded.cloud.access_token.as_deref(),
        Some("environment-secret")
    );
    assert!(
        !loaded
            .effective_report()
            .to_string()
            .contains("environment-secret")
    );
    let env = Environment::from([("SPM_CLOUD_ACCESS_TOKEN".into(), "".into())]);
    assert!(
        LoadedConfig::load_with(Some(&file), dir.path(), &env)
            .unwrap()
            .cloud
            .access_token
            .is_none()
    );
}

#[test]
fn provider_validation_is_shared_and_environment_replaces_whole_array() {
    let provider = json!({"provider_id":"custom","display_name":"Auth","has_joined_url":"https://auth.example.com/hasJoined","enabled":false});
    let config = json!({"auth":{"identity_providers":[provider.clone()]}});
    let loaded = load(config.clone(), Environment::new()).1.unwrap();
    assert!(!loaded.providers[0].enabled);
    assert_eq!(loaded.providers[0].provider_id, "custom");
    let env = Environment::from([("SPM_CLOUD_IDENTITY_PROVIDERS".into(), "[]".into())]);
    assert!(load(config, env).1.unwrap().providers.is_empty());
    for providers in [
        json!([provider.clone(), provider.clone()]),
        json!([{"provider_id":"official","display_name":"Override","has_joined_url":"https://auth.example.com/hasJoined"}]),
        json!([{"provider_id":"custom","display_name":"Auth","base_url":"https://auth.example.com","has_joined_url":"https://auth.example.com/hasJoined"}]),
    ] {
        assert!(
            load(
                json!({"auth":{"identity_providers":providers}}),
                Environment::new()
            )
            .1
            .is_err()
        );
    }
    let env = Environment::from([
        (
            "SPM_CLOUD_IDENTITY_PROVIDERS".into(),
            json!([provider]).to_string(),
        ),
        (
            "SPM_CLOUD_HAS_JOINED_URL".into(),
            "https://auth.example.com/hasJoined".into(),
        ),
    ]);
    assert!(load(json!({}), env).1.is_err());
}

#[test]
fn selected_missing_or_invalid_file_never_falls_back_to_environment() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.json");
    let env = Environment::from([("SPM_CLOUD_CONFIG".into(), "from-env.json".into())]);
    assert_eq!(
        select_file(Some(&missing), dir.path(), &env).unwrap(),
        Some(missing.clone())
    );
    assert_eq!(
        select_file(None, dir.path(), &env).unwrap(),
        Some(dir.path().join("from-env.json"))
    );
    assert!(LoadedConfig::load_with(Some(&missing), dir.path(), &Environment::new()).is_err());
    fs::write(dir.path().join("config.json"), "bad").unwrap();
    assert_eq!(
        select_file(None, dir.path(), &Environment::new()).unwrap(),
        Some(dir.path().join("config.json"))
    );
}

#[test]
fn initialization_is_persistent_and_does_not_overwrite_either_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.json");
    init_config(&file).unwrap();
    let token = fs::read(dir.path().join("secrets/bootstrap-token.txt")).unwrap();
    let config = fs::read(&file).unwrap();
    let loaded = LoadedConfig::load_with(Some(&file), dir.path(), &Environment::new()).unwrap();
    assert!(loaded.cloud.access_token.is_some());
    assert!(init_config(&file).is_err());
    assert_eq!(fs::read(&file).unwrap(), config);
    assert_eq!(
        fs::read(dir.path().join("secrets/bootstrap-token.txt")).unwrap(),
        token
    );
    fs::remove_file(&file).unwrap();
    assert!(init_config(&file).is_err());
    assert!(!file.exists());
    assert_eq!(
        fs::read(dir.path().join("secrets/bootstrap-token.txt")).unwrap(),
        token
    );
}

#[test]
fn check_and_print_commands_have_no_database_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.json");
    init_config(&file).unwrap();
    let result = cli(
        &[
            "--config".as_ref(),
            file.as_os_str(),
            "--check-config".as_ref(),
        ],
        dir.path(),
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = cli(&["--print-effective-config".as_ref()], dir.path());
    assert!(result.status.success());
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        value["effective"]["auth"]["bootstrap_access_token_file"],
        "[REDACTED]"
    );
    assert!(!dir.path().join("data").exists());
    assert!(!dir.path().join("data/spm-cloud.db").exists());
    for args in [
        vec!["--config"],
        vec!["--unknown"],
        vec!["--check-config", "--print-effective-config"],
    ] {
        let args: Vec<_> = args.iter().map(std::ffi::OsStr::new).collect();
        assert!(!cli(&args, dir.path()).status.success());
    }
}

#[test]
fn shipped_example_matches_current_defaults_and_features_remain_off() {
    let example: FileConfig = serde_json::from_str(include_str!("../config.example.json")).unwrap();
    let defaults = FileConfig::default();
    assert_eq!(
        serde_json::to_value(example.limits).unwrap(),
        serde_json::to_value(defaults.limits).unwrap()
    );
    assert_eq!(
        serde_json::to_value(example.retention).unwrap(),
        serde_json::to_value(defaults.retention).unwrap()
    );
    assert_eq!(
        serde_json::to_value(example.features).unwrap(),
        serde_json::to_value(defaults.features).unwrap()
    );
    let schema: Value = serde_json::from_str(include_str!("../config.schema.json")).unwrap();
    let defaults = serde_json::to_value(FileConfig::default()).unwrap();
    for (section, object) in defaults.as_object().unwrap() {
        if let Some(fields) = object.as_object() {
            for (field, value) in fields {
                assert_eq!(
                    &schema["properties"][section]["properties"][field]["default"], value,
                    "schema default drift: {section}.{field}"
                );
            }
        } else {
            assert_eq!(&schema["properties"][section]["default"], object);
        }
    }
}

#[tokio::test]
async fn normal_binary_startup_uses_json_config_and_structured_logs() {
    use std::process::Stdio;
    struct ChildGuard(Option<std::process::Child>);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.json");
    init_config(&file).unwrap();
    let token = fs::read_to_string(dir.path().join("secrets/bootstrap-token.txt")).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut config: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    config["instance"]["bind"] = json!(format!("127.0.0.1:{port}"));
    config["instance"]["instance_id"] = json!("json-startup-test");
    config["limits"]["max_entity_query_count"] = json!(2);
    config["logging"]["format"] = json!("json");
    fs::write(&file, config.to_string()).unwrap();
    let child = cli_command(dir.path())
        .args(["--config".as_ref(), file.as_os_str()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut guard = ChildGuard(Some(child));
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}/v1/instance");
    let mut found = None;
    for _ in 0..100 {
        if let Ok(response) = client.get(&url).send().await {
            if response.status().is_success() {
                found = Some(response.json::<Value>().await.unwrap());
                break;
            }
        }
        if guard.0.as_mut().unwrap().try_wait().unwrap().is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let child = guard.0.as_mut().unwrap();
    let _ = child.kill();
    let output = guard.0.take().unwrap().wait_with_output().unwrap();
    let log = String::from_utf8(output.stdout).unwrap();
    let error = String::from_utf8(output.stderr).unwrap();
    let found = found.unwrap_or_else(|| panic!("startup did not listen: {error} {log}"));
    assert_eq!(found["instance_id"], "json-startup-test");
    assert_eq!(found["limits"]["max_entity_query_count"], 2);
    assert!(dir.path().join("data/spm-cloud.db").is_file());
    assert!(!log.contains(token.trim()));
    assert!(!error.contains(token.trim()));
    let events: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        events
            .iter()
            .any(|e| e["fields"]["message"] == "SPM Cloud listening"),
        "{log}"
    );
}

#[test]
fn invalid_password_hash_is_rejected_before_database_creation_and_restart_preserves_password() {
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString},
    };
    let dir = tempfile::tempdir().unwrap();
    let salt = SaltString::encode_b64(b"spm-test-salt-value").unwrap();
    let first = Argon2::default()
        .hash_password(b"original-password", &salt)
        .unwrap()
        .to_string();
    let second = Argon2::default()
        .hash_password(b"replacement-password", &salt)
        .unwrap()
        .to_string();
    let env = Environment::from([("SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH".into(), first.clone())]);
    let loaded = LoadedConfig::load_with(None, dir.path(), &env).unwrap();
    drop(spm_cloud::CloudStore::open(&loaded.cloud).unwrap());
    let env = Environment::from([("SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH".into(), second)]);
    let loaded = LoadedConfig::load_with(None, dir.path(), &env).unwrap();
    drop(spm_cloud::CloudStore::open(&loaded.cloud).unwrap());
    let conn = rusqlite::Connection::open(&loaded.cloud.database_path).unwrap();
    let stored: String = conn
        .query_row(
            "SELECT password_hash FROM account_credentials WHERE account_id='account_local'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, first);
    for invalid in [
        "plaintext-password".to_owned(),
        first.replace("argon2id", "argon2i"),
    ] {
        let env = Environment::from([("SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH".into(), invalid)]);
        assert!(LoadedConfig::load_with(None, dir.path(), &env).is_err());
    }
}

#[tokio::test]
async fn configured_query_limit_is_enforced_and_discovery_does_not_advertise_reserved_features() {
    let (_dir, loaded) = load(
        json!({"limits":{"max_entity_query_count":1}}),
        Environment::from([("SPM_CLOUD_ACCESS_TOKEN".into(), "test-query-secret".into())]),
    );
    let loaded = loaded.unwrap();
    let store = spm_cloud::CloudStore::open(&loaded.cloud).unwrap();
    let state = spm_cloud::api::AppState::new(loaded.cloud, store);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, spm_cloud::api::router(state))
            .await
            .unwrap();
    });
    let client = reqwest::Client::new();
    let response: Value = client
        .get(format!("http://{address}/v1/instance"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["limits"]["max_entity_query_count"], 1);
    for feature in [
        "entity_motion_v1",
        "player_display_state_v1",
        "vehicle_appearance_v1",
        "projectile_snapshot_v1",
    ] {
        assert!(
            !response["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c == feature)
        );
    }
    let world = "a".repeat(64);
    let response = client
        .post(format!(
            "http://{address}/v1/entity-worlds/{world}/appearances/query"
        ))
        .bearer_auth("test-query-secret")
        .json(&json!({"entity_uuids":[uuid::Uuid::new_v4(),uuid::Uuid::new_v4()]}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    server.abort();
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("at most 1 UUIDs"), "{body}");
}

#[tokio::test]
async fn enabled_display_discovery_reports_executed_budgets() {
    let (_dir, loaded) = load(
        json!({"features":{"player_display_state":true,"vehicle_bindings":true,"entity_motion":true},"limits":{"max_visual_state_bytes":1024,"max_visual_variables":2}}),
        Environment::from([("SPM_CLOUD_ACCESS_TOKEN".into(), "display-secret".into())]),
    );
    let loaded = loaded.unwrap();
    let store = spm_cloud::CloudStore::open(&loaded.cloud).unwrap();
    let state = spm_cloud::api::AppState::new(loaded.cloud, store);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, spm_cloud::api::router(state))
            .await
            .unwrap();
    });
    let response: Value = reqwest::get(format!("http://{address}/v1/instance"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    server.abort();
    assert_eq!(response["limits"]["max_visual_state_bytes"], 1024);
    assert_eq!(response["limits"]["max_visual_variables"], 2);
    assert!(
        response["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "entity_motion_v1")
    );
    assert!(
        response["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "vehicle_appearance_v1")
    );
    assert!(
        response["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "player_display_state_v1")
    );
    assert!(
        !response["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "projectile_snapshot_v1")
    );
}
