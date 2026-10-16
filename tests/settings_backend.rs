use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use spm_cloud::{
    api::AppState,
    config_file::{Environment, LoadedConfig},
};
use tower::ServiceExt;

const TOKEN: &str = "settings-owner-integration-token";
fn state() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("token"), TOKEN).unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, r#"{"auth":{"bootstrap_access_token_file":"token"}}"#).unwrap();
    let loaded = LoadedConfig::load_with(Some(&path), dir.path(), &Environment::new()).unwrap();
    let store = spm_cloud::CloudStore::open(&loaded.cloud).unwrap();
    let state = AppState::new(loaded.cloud.clone(), store);
    state.initialize_settings(&loaded);
    (dir, state)
}
async fn request(state: &AppState, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    request_as(state, method, path, body, TOKEN).await
}
async fn request_as(
    state: &AppState,
    method: &str,
    path: &str,
    body: Value,
    token: &str,
) -> (StatusCode, Value) {
    let response = spm_cloud::api::router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn get(state: &AppState) -> Value {
    let (status, value) = request(state, "GET", "/admin/api/config", Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}

#[tokio::test]
async fn config_state_saved_active_and_restart_are_distinct() {
    let (_dir, state) = state();
    let original = get(&state).await;
    assert_eq!(original["pending_restart_fields"], json!([]));
    assert_eq!(
        original["active"]["instance"]["origin"],
        "https://localhost"
    );
    let mut config = original["config"].clone();
    config["instance"] = json!({"origin":"https://changed.example.com"});
    config["logging"] = json!({"level":"debug"});
    config["limits"] = json!({"max_asset_bytes":321});
    let (status, saved) = request(
        &state,
        "PUT",
        "/admin/api/config",
        json!({"config":config,"revision":original["revision"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(
        saved["applied_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("limits.max_asset_bytes"))
    );
    assert!(
        saved["pending_restart_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("logging.level"))
    );
    let current = get(&state).await;
    assert_eq!(current["active"]["instance"]["origin"], "https://localhost");
    assert_eq!(current["active"]["logging"]["level"], "info");
    assert_eq!(current["active"]["limits"]["max_asset_bytes"], 321);
    let (_, again) = request(
        &state,
        "PUT",
        "/admin/api/config",
        json!({"config":current["config"],"revision":current["revision"]}),
    )
    .await;
    assert_eq!(again["applied_fields"], json!([]));
    assert!(
        again["unchanged_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("logging.level"))
    );
}

#[tokio::test]
async fn config_validate_has_field_errors_and_no_publication() {
    let (dir, state) = state();
    let before = get(&state).await;
    let mut config = before["config"].clone();
    config["limits"] = json!({"max_asset_bytes":0});
    let (status, result) = request(
        &state,
        "POST",
        "/admin/api/config/validate",
        json!({"config":config,"revision":before["revision"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["valid"], false);
    assert_eq!(result["errors"][0]["path"], "limits.max_asset_bytes");
    assert_eq!(get(&state).await["revision"], before["revision"]);
    assert!(!dir.path().join(".managed-secrets").exists());
}

#[tokio::test]
async fn managed_secret_replace_conflict_cleanup_keep_and_clear() {
    let (dir, state) = state();
    let before = get(&state).await;
    let mut config = before["config"].clone();
    config["cache"] = json!({"backend":"redis"});
    let secrets = json!({"redis":{"operation":"replace","host":"127.0.0.1","port":6379,"username":"user","password":"p@ss:/word","database":2}});
    let (status, preview) = request(
        &state,
        "POST",
        "/admin/api/config/validate",
        json!({"config":config,"revision":before["revision"],"managed_secrets":secrets}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["valid"], true);
    assert!(!dir.path().join(".managed-secrets").exists());
    assert_eq!(state.runtime_config().cache.backend, "memory");
    let (status, result) = request(
        &state,
        "PUT",
        "/admin/api/config",
        json!({"config":config,"revision":"stale","managed_secrets":secrets}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{result}");
    assert!(!dir.path().join(".managed-secrets").exists());
    let (status, result) = request(
        &state,
        "PUT",
        "/admin/api/config",
        json!({"config":config,"revision":before["revision"],"managed_secrets":secrets}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let current = get(&state).await;
    assert!(!current.to_string().contains("p@ss"));
    assert_eq!(current["secret_status"]["redis"]["configured"], true);
    let path = current["config"]["cache"]["redis_url_file"]
        .as_str()
        .unwrap();
    let secret = std::fs::read_to_string(dir.path().join(path)).unwrap();
    let info = redis::Client::open(secret.trim())
        .unwrap()
        .get_connection_info()
        .clone();
    assert_eq!(info.redis.password.as_deref(), Some("p@ss:/word"));
    config = current["config"].clone();
    config["cache"]["backend"] = json!("none");
    let (status, result) = request(&state,"PUT","/admin/api/config",json!({"config":config,"revision":current["revision"],"managed_secrets":{"redis":{"operation":"clear"}}})).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        get(&state).await["secret_status"]["redis"]["configured"],
        false
    );
    assert!(
        dir.path().join(path).exists(),
        "old files must remain for rollback"
    );
}

#[tokio::test]
async fn provider_admin_removal_disables_managed_but_preserves_legacy() {
    let (_dir, state) = state();
    let original = get(&state).await;
    assert!(
        original["provider_admin_entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["provider_id"] == "littleskin" && p["source"] == "database")
    );
    let mut config = original["config"].clone();
    config["auth"]["identity_providers"] = json!([{"provider_id":"custom","display_name":"Custom","base_url":"https://example.com/api","enabled":true,"authenticate_path":"/auth/login"}]);
    let (status, result) = request(
        &state,
        "PUT",
        "/admin/api/config",
        json!({"config":config,"revision":original["revision"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let current = get(&state).await;
    config = current["config"].clone();
    config["auth"]["identity_providers"] = json!([]);
    let (status, result) = request(
        &state,
        "PUT",
        "/admin/api/config",
        json!({"config":config,"revision":current["revision"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let entries = get(&state).await["provider_admin_entries"]
        .as_array()
        .unwrap()
        .clone();
    assert!(
        entries
            .iter()
            .any(|p| p["provider_id"] == "custom" && p["enabled"] == false)
    );
    assert!(
        state
            .store
            .list_providers()
            .unwrap()
            .iter()
            .any(|p| p["provider_id"] == "littleskin")
    );
    assert!(
        !state
            .store
            .list_providers()
            .unwrap()
            .iter()
            .any(|p| p["provider_id"] == "custom")
    );
}

#[tokio::test]
async fn provider_check_rejects_unsafe_candidates_without_saving() {
    let (_dir, state) = state();
    let before = get(&state).await;
    for address in [
        "http://example.com",
        "https://user:password@example.com",
        "https://127.0.0.1",
        "https://example.com:8443",
    ] {
        let (status,result)=request(&state,"POST","/admin/api/providers/check",json!({"provider_id":"candidate","display_name":"Candidate","enabled":true,"base_url":address})).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["valid"], false);
        assert_eq!(result["login_verified"], false);
    }
    assert_eq!(get(&state).await["revision"], before["revision"]);
}

#[tokio::test]
async fn managed_secret_validation_failure_and_publication_failure_preserve_every_resource() {
    let (dir, state) = state();
    let before = get(&state).await;
    let original = std::fs::read(dir.path().join("config.json")).unwrap();
    let mut config = before["config"].clone();
    config["auth"]["identity_providers"] = json!([{"provider_id":"rollback","display_name":"Rollback","base_url":"https://example.com","enabled":true}]);
    config["limits"] = json!({"max_asset_bytes":0});
    let secrets = json!({"microsoft":{"operation":"replace","value":"must-not-leak-secret"}});
    let (status, result) = request(
        &state,
        "PUT",
        "/admin/api/config",
        json!({"config":config,"revision":before["revision"],"managed_secrets":secrets}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{result}");
    assert!(!result.to_string().contains("must-not-leak"));
    assert!(!dir.path().join(".managed-secrets").exists());
    config["limits"] = json!({"max_asset_bytes":777});
    let path = dir.path().join("config.json");
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    let original_permissions = permissions.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    let response = request(
        &state,
        "PUT",
        "/admin/api/config",
        json!({"config":config,"revision":before["revision"],"managed_secrets":secrets}),
    )
    .await;
    std::fs::set_permissions(&path, original_permissions).unwrap();
    assert_eq!(response.0, StatusCode::BAD_REQUEST, "{}", response.1);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(state.runtime_config().max_asset_bytes, 134217728);
    assert!(
        !state
            .store
            .list_providers()
            .unwrap()
            .iter()
            .any(|p| p["provider_id"] == "rollback")
    );
    assert_eq!(
        std::fs::read_dir(dir.path().join(".managed-secrets"))
            .unwrap()
            .count(),
        0
    );
    let db = rusqlite::Connection::open(&state.runtime_config().database_path).unwrap();
    let audits: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE action='CONFIG_UPDATED'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(audits, 0);
}

#[tokio::test]
async fn config_save_database_commit_failure_restores_file_providers_and_secrets() {
    let (dir, state) = state();
    let before = get(&state).await;
    let db = rusqlite::Connection::open(&state.runtime_config().database_path).unwrap();
    db.execute_batch("CREATE TABLE test_commit_failure (account_id TEXT REFERENCES accounts(account_id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_config_commit AFTER INSERT ON config_managed_providers BEGIN INSERT INTO test_commit_failure VALUES('nonexistent-account'); END;").unwrap();
    let mut config = before["config"].clone();
    config["auth"]["identity_providers"] = json!([{"provider_id":"rollback","display_name":"Rollback","base_url":"https://example.com","enabled":true}]);
    let (status,result)=request(&state,"PUT","/admin/api/config",json!({"config":config,"revision":before["revision"],"managed_secrets":{"microsoft":{"operation":"replace","value":"commit-failure-secret"}}})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{result}");
    assert_eq!(get(&state).await["revision"], before["revision"]);
    assert!(
        !state
            .store
            .list_providers()
            .unwrap()
            .iter()
            .any(|p| p["provider_id"] == "rollback")
    );
    assert!(
        state
            .runtime_config()
            .web_ui
            .microsoft_client_secret_file
            .is_none()
    );
    assert_eq!(
        std::fs::read_dir(dir.path().join(".managed-secrets"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn settings_mutations_and_checks_are_owner_only_and_admin_reads_are_redacted() {
    let (_dir, state) = state();
    let initial = get(&state).await;
    for role in ["admin", "member"] {
        state
            .store
            .create_account(role, "test-role-password")
            .unwrap();
        state
            .store
            .set_admin_account("account_local", role, "account_local", role, false, None)
            .unwrap();
        let session = state
            .store
            .issue_session(&spm_cloud::models::LoginRequest {
                account_id: role.into(),
                password: "test-role-password".into(),
            })
            .unwrap();
        for endpoint in [
            "/admin/api/config/validate",
            "/admin/api/providers/check",
            "/admin/api/config/check-connection",
        ] {
            let body = match endpoint {
                "/admin/api/config/validate" => {
                    json!({"config":initial["config"],"revision":initial["revision"]})
                }
                "/admin/api/providers/check" => {
                    json!({"provider_id":"safe","display_name":"Safe","base_url":"https://example.com","enabled":true})
                }
                _ => json!({"kind":"redis"}),
            };
            assert_eq!(
                request_as(&state, "POST", endpoint, body, &session.access_token)
                    .await
                    .0,
                StatusCode::FORBIDDEN
            );
        }
        let (status, response) = request_as(
            &state,
            "GET",
            "/admin/api/config",
            Value::Null,
            &session.access_token,
        )
        .await;
        assert!(!response.to_string().contains(TOKEN));
        if role == "admin" {
            assert_eq!(status, StatusCode::OK);
            assert_eq!(response["writable"], false);
            assert_eq!(response["field_status"]["logging.level"]["editable"], false);
        } else {
            assert_eq!(status, StatusCode::FORBIDDEN);
        }
    }
}

#[tokio::test]
async fn config_state_file_application_preserves_startup_logging_and_reconciles_after_restart() {
    let (dir, state) = state();
    let mut config = get(&state).await["config"].clone();
    config["logging"] = json!({"level":"debug"});
    config["limits"] = json!({"max_asset_bytes":4321});
    config["auth"]["identity_providers"] = json!([{"provider_id":"file-provider","display_name":"From file","base_url":"https://example.com","enabled":true}]);
    let path = dir.path().join("config.json");
    std::fs::write(&path, config.to_string()).unwrap();
    let loaded = LoadedConfig::load(Some(&path)).unwrap();
    {
        let _lock = state.config_edit_guard().await;
        spm_cloud::config_apply::apply(&state, &loaded, None, None).unwrap();
        spm_cloud::config_apply::apply(&state, &loaded, None, None).unwrap();
    }
    assert_eq!(state.runtime_config().max_asset_bytes, 4321);
    let current = get(&state).await;
    assert_eq!(current["active"]["logging"]["level"], "info");
    assert!(
        current["pending_restart_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("logging.level"))
    );
    let db = rusqlite::Connection::open(&loaded.cloud.database_path).unwrap();
    db.execute("INSERT INTO identities(identity_id,account_id,identity_kind,provider_id,profile_uuid,display_name,verified) VALUES('binding','account_local','yggdrasil','file-provider',?1,'Bound player',1)",[uuid::Uuid::new_v4().to_string()]).unwrap();
    // Model a crash after publishing the file but before committing provider state.
    config["auth"]["identity_providers"] = json!([]);
    std::fs::write(&path, config.to_string()).unwrap();
    let loaded = LoadedConfig::load(Some(&path)).unwrap();
    let store = spm_cloud::CloudStore::open(&loaded.cloud).unwrap();
    let restarted = AppState::new(loaded.cloud.clone(), store);
    spm_cloud::config_apply::apply(&restarted, &loaded, None, None).unwrap();
    assert!(
        !restarted
            .store
            .list_providers()
            .unwrap()
            .iter()
            .any(|p| p["provider_id"] == "file-provider")
    );
    assert!(
        restarted
            .store
            .list_providers()
            .unwrap()
            .iter()
            .any(|p| p["provider_id"] == "littleskin")
    );
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM identities WHERE identity_id='binding'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn config_state_environment_values_remain_nested_and_readonly() {
    let dir = tempfile::tempdir().unwrap();
    let env = Environment::from([
        ("SPM_CLOUD_ACCESS_TOKEN".into(), TOKEN.into()),
        ("SPM_CLOUD_ORIGIN".into(), "https://env.example.com".into()),
        ("SPM_CLOUD_MAX_ASSET_BYTES".into(), "12345".into()),
        ("RUST_LOG".into(), "warn,spm_cloud=debug".into()),
    ]);
    let loaded = LoadedConfig::load_with(None, dir.path(), &env).unwrap();
    let store = spm_cloud::CloudStore::open(&loaded.cloud).unwrap();
    let state = AppState::new(loaded.cloud.clone(), store);
    state.initialize_settings(&loaded);
    let response = get(&state).await;
    assert_eq!(
        response["active"]["instance"]["origin"],
        "https://env.example.com"
    );
    assert_eq!(response["active"]["limits"]["max_asset_bytes"], 12345);
    assert_eq!(
        response["active"]["logging"]["filter"],
        "warn,spm_cloud=debug"
    );
    assert_eq!(response["writable"], false);
    assert_eq!(response["config"], Value::Null);
    assert!(!response.to_string().contains(TOKEN));
}

#[tokio::test]
async fn connection_check_allows_local_scanner_and_rejects_general_proxy_requests() {
    let (_dir, state) = state();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route("/health", axum::routing::get(|| async { "healthy" })),
        )
        .await
        .unwrap();
    });
    let (status, result) = request(
        &state,
        "POST",
        "/admin/api/config/check-connection",
        json!({"kind":"nudenet","scanner_url":format!("http://{address}/health")}),
    )
    .await;
    task.abort();
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["connected"], true);
    assert_eq!(result["inference_verified"], false);
    assert_eq!(
        request(
            &state,
            "POST",
            "/admin/api/config/check-connection",
            json!({"kind":"http","scanner_url":"http://127.0.0.1"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn managed_secret_generated_token_is_returned_once_and_immediately_replaces_old_token() {
    let (dir, state) = state();
    let before = get(&state).await;
    let (status,saved)=request(&state,"PUT","/admin/api/config",json!({"config":before["config"],"revision":before["revision"],"managed_secrets":{"bootstrap_token":{"operation":"generate"}}})).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let token = saved["generated_secrets"]["bootstrap_token"]
        .as_str()
        .unwrap();
    assert_eq!(token.len(), 64);
    assert_eq!(
        request(&state, "GET", "/admin/api/config", Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, current) =
        request_as(&state, "GET", "/admin/api/config", Value::Null, token).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert!(!current.to_string().contains(token));
    assert!(current.get("generated_secrets").is_none());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("token")).unwrap(),
        TOKEN
    );
    let loaded = LoadedConfig::load(Some(&dir.path().join("config.json"))).unwrap();
    spm_cloud::config_apply::apply(&state, &loaded, None, None).unwrap();
    assert_eq!(
        request_as(&state, "GET", "/admin/api/config", Value::Null, token)
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn provider_admin_gateway_metadata_roundtrips_without_conflicting_base_url() {
    let (_dir, state) = state();
    let before = get(&state).await;
    let mut config = before["config"].clone();
    config["auth"]["identity_providers"] = json!([{"provider_id":"gateway","display_name":"Gateway","has_joined_url":"https://example.com/custom/hasJoined","authenticate_path":"/custom/authenticate","enabled":false}]);
    let (status, result) = request(
        &state,
        "PUT",
        "/admin/api/config",
        json!({"config":config,"revision":before["revision"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let response = get(&state).await;
    let provider = response["provider_admin_entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["provider_id"] == "gateway")
        .unwrap();
    assert_eq!(provider["base_url"], "");
    assert_eq!(provider["session_path"], Value::Null);
    assert_eq!(provider["authenticate_path"], "/custom/authenticate");
    assert_eq!(provider["enabled"], false);
    let parsed: spm_cloud::models::IdentityProviderUpdate =
        serde_json::from_value(provider.clone()).unwrap();
    assert!(spm_cloud::game_auth::provider_endpoint(&parsed).is_ok());
}

#[tokio::test]
async fn managed_secret_malformed_input_never_echoes_secret_values() {
    let (_dir, state) = state();
    let before = get(&state).await;
    for endpoint in ["/admin/api/config", "/admin/api/config/validate"] {
        let (status,body)=request(&state,if endpoint.ends_with("validate"){"POST"}else{"PUT"},endpoint,json!({"config":before["config"],"revision":before["revision"],"managed_secrets":{"redis":{"tls":"accidentally-entered-secret"}}})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(!body.to_string().contains("accidentally-entered-secret"));
    }
}

#[tokio::test]
async fn config_validate_advanced_errors_point_to_the_exact_control() {
    let (_dir, state) = state();
    let before = get(&state).await;
    for (group, key, value) in [
        ("cache", "ttl_seconds", json!(0)),
        ("performance", "preset", json!("unknown")),
        ("moderation", "score_threshold", json!(2)),
        ("updates", "check_interval_hours", json!(0)),
        ("web_ui", "microsoft_client_id", json!("invalid")),
    ] {
        let mut config = before["config"].clone();
        config[group] = json!({key:value});
        let (status, result) = request(
            &state,
            "POST",
            "/admin/api/config/validate",
            json!({"config":config,"revision":before["revision"]}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(
            result["errors"][0]["path"],
            format!("{group}.{key}"),
            "{result}"
        );
    }
}
