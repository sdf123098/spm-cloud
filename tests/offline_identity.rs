use serde_json::{Value, json};
use spm_cloud::config_file::{Environment, LoadedConfig};

fn load(value: Value) -> Result<LoadedConfig, spm_cloud::error::CloudError> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.json");
    std::fs::write(&file, value.to_string()).unwrap();
    LoadedConfig::load_with(Some(&file), dir.path(), &Environment::new())
}

#[test]
fn default_configuration_automatically_authorizes_offline_sync() {
    let loaded = load(json!({})).unwrap();
    let effective = loaded.effective_report();
    assert_eq!(
        effective["effective"]["offline_identity"]["authorization_mode"],
        "automatic"
    );
    assert_eq!(
        effective["effective"]["offline_identity"]["max_identities_per_account"],
        3
    );
    assert_eq!(
        effective["effective"]["offline_identity"]["session_ttl_seconds"],
        60
    );
}

#[test]
fn explicit_configuration_enables_offline_identity_and_rejects_invalid_limits() {
    let loaded = load(json!({"offline_identity":{"authorization_mode":"automatic"}})).unwrap();
    assert_eq!(
        loaded.effective_report()["effective"]["offline_identity"]["authorization_mode"],
        "automatic"
    );
    for field in [
        "max_identities_per_account",
        "session_ttl_seconds",
        "activation_requests_per_minute",
        "issuance_requests_per_hour",
    ] {
        assert!(
            load(json!({"offline_identity":{field:0}})).is_err(),
            "{field}"
        );
    }
    assert!(load(json!({"offline_identity":{"unknown_setting":true}})).is_err());
}

#[tokio::test]
async fn instance_capability_follows_the_experiment_switch() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    for enabled in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.json");
        std::fs::write(
            &file,
            json!({"offline_identity":{"authorization_mode":if enabled {"automatic"} else {"disabled"}}}).to_string(),
        )
        .unwrap();
        let loaded = LoadedConfig::load_with(Some(&file), dir.path(), &Environment::new()).unwrap();
        let store = spm_cloud::CloudStore::open(&loaded.cloud).unwrap();
        let app = spm_cloud::api::router(spm_cloud::api::AppState::new(loaded.cloud, store));
        let response = app
            .oneshot(Request::get("/v1/instance").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(response.status().is_success());
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(
            body["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "cloud_offline_identity_v1"),
            enabled
        );
    }
}

#[test]
fn migration_is_idempotent_and_preserves_existing_accounts() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = LoadedConfig::load_with(None, dir.path(), &Environment::new()).unwrap();
    let first = spm_cloud::CloudStore::open(&loaded.cloud).unwrap();
    first
        .create_account("existing_user", "isolated-test-password")
        .unwrap();
    drop(first);
    let second = spm_cloud::CloudStore::open(&loaded.cloud).unwrap();
    let db = rusqlite::Connection::open(&loaded.cloud.database_path).unwrap();
    for table in [
        "cloud_offline_identities",
        "cloud_offline_keys",
        "cloud_offline_sessions",
        "cloud_offline_appearances",
    ] {
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "missing {table}");
    }
    let preserved: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id='existing_user')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(preserved);
    drop(second);
}
