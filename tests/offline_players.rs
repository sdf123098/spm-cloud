use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use spm_cloud::{
    api::{AppState, router},
    config_file::{Environment, LoadedConfig},
};
use tower::ServiceExt;

const TOKEN: &str = "isolated-offline-regression-token";

fn setup() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("token.txt"), TOKEN).unwrap();
    let file = dir.path().join("config.json");
    std::fs::write(
        &file,
        json!({"auth":{"bootstrap_access_token_file":"token.txt"}}).to_string(),
    )
    .unwrap();
    // The loader is covered separately. HTTP fixtures turn on the default-off feature.
    let value = json!({"auth":{"bootstrap_access_token_file":"token.txt"}, "offline_identity":{"authorization_mode":"automatic"}});
    std::fs::write(&file, value.to_string()).unwrap();
    let loaded = LoadedConfig::load_with(Some(&file), dir.path(), &Environment::new()).unwrap();
    let store = spm_cloud::CloudStore::open(&loaded.cloud).unwrap();
    (dir, AppState::new(loaded.cloud, store))
}

async fn request(
    state: &AppState,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
) -> (StatusCode, Value, bool) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = router(state.clone())
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let no_store = response
        .headers()
        .get("cache-control")
        .is_some_and(|v| v == "no-store");
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        no_store,
    )
}

#[tokio::test]
async fn offline_management_requires_login_and_never_lists_key_secrets() {
    let (_dir, state) = setup();
    let (status, _, _) = request(
        &state,
        "GET",
        "/account/api/offline-identities",
        json!({}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, issued, no_store) = request(
        &state,
        "POST",
        "/account/api/offline-identities",
        json!({"label":"Device A"}),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(no_store);
    let key = issued["offline_key"].as_str().expect("one-time key");
    assert!(key.starts_with("spm-offline-v1."));
    let (status, listed, no_store) = request(
        &state,
        "GET",
        "/account/api/offline-identities",
        json!({}),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(no_store);
    assert!(!listed.to_string().contains(key));
    assert!(!listed.to_string().contains("secret_hash"));
}

#[tokio::test]
async fn automatic_enrollment_is_idempotent_and_manual_approval_cannot_be_bypassed() {
    use spm_cloud::offline_identity::OfflineAuthorizationMode;
    let (_dir, state) = setup();
    let key = format!("spm-offline-v1.cok_{}.{}", "a".repeat(32), "b".repeat(43));
    let body = json!({"offline_key":key,"label":"Automatic client"});
    let (status, first, no_store) = request(
        &state,
        "POST",
        "/v1/offline-identities/request",
        body.clone(),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(no_store);
    assert_eq!(first["authorization_status"], "APPROVED");
    assert!(!first.to_string().contains(&key));
    let (_, second, _) = request(
        &state,
        "POST",
        "/v1/offline-identities/request",
        body.clone(),
        Some(TOKEN),
    )
    .await;
    assert_eq!(first["identity_id"], second["identity_id"]);
    let (_, list, _) = request(
        &state,
        "GET",
        "/v1/offline-identities/me",
        json!({}),
        Some(TOKEN),
    )
    .await;
    assert_eq!(list["identities"].as_array().unwrap().len(), 1);
    let account = first["account_id"].as_str().unwrap();
    let admin_path = format!("/admin/api/offline-authorizations/{account}");
    let (status, _, _) = request(
        &state,
        "PUT",
        &admin_path,
        json!({"status":"REVOKED"}),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, denied, _) = request(
        &state,
        "POST",
        "/v1/offline-identities/request",
        body.clone(),
        Some(TOKEN),
    )
    .await;
    assert_eq!(denied["authorization_status"], "REVOKED");
    let (_, error, _) = request(
        &state,
        "POST",
        "/account/api/offline-identities",
        json!({"label":"bypass"}),
        Some(TOKEN),
    )
    .await;
    assert_eq!(error["code"], "OFFLINE_AUTHORIZATION_DENIED");
    let mut config = state.runtime_config();
    config.offline_identity.authorization_mode = OfflineAuthorizationMode::Disabled;
    state.update_runtime_config(config.clone()).unwrap();
    let (_, error, _) = request(
        &state,
        "POST",
        "/v1/offline-identities/request",
        body,
        Some(TOKEN),
    )
    .await;
    assert_eq!(error["code"], "OFFLINE_IDENTITY_DISABLED");
    config.offline_identity.authorization_mode = OfflineAuthorizationMode::Manual;
    state.update_runtime_config(config).unwrap();
    state
        .store
        .create_account("manual_member", "isolated-test-password")
        .unwrap();
    let (status, login, _) = request(
        &state,
        "POST",
        "/v1/sessions",
        json!({"account_id":"manual_member","password":"isolated-test-password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = login["access_token"].as_str().unwrap().to_owned();
    let body = json!({"offline_key":format!("spm-offline-v1.cok_{}.{}","c".repeat(32),"d".repeat(43)),"label":"Manual client"});
    let (_, pending, _) = request(
        &state,
        "POST",
        "/v1/offline-identities/request",
        body.clone(),
        Some(&token),
    )
    .await;
    assert_eq!(pending["authorization_status"], "PENDING");
    let (_, list, _) = request(
        &state,
        "GET",
        "/v1/offline-identities/me",
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(list["identities"].as_array().unwrap().len(), 0);
    let (status, _, _) = request(
        &state,
        "PUT",
        "/admin/api/offline-authorizations/manual_member",
        json!({"status":"APPROVED"}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, error, _) = request(
        &state,
        "POST",
        "/account/api/offline-identities",
        json!({"label":"bypass"}),
        Some(&token),
    )
    .await;
    assert_eq!(error["code"], "OFFLINE_AUTHORIZATION_PENDING");
    let (status, _, _) = request(&state,"POST","/admin/api/accounts",json!({"account_id":"offline_reviewer","password":"isolated-reviewer-password","role":"admin"}),Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, reviewer, _) = request(
        &state,
        "POST",
        "/admin/api/login",
        json!({"account_id":"offline_reviewer","password":"isolated-reviewer-password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = request(
        &state,
        "PUT",
        "/admin/api/offline-authorizations/manual_member",
        json!({"status":"APPROVED"}),
        Some(reviewer["access_token"].as_str().unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, approved, _) = request(
        &state,
        "POST",
        "/v1/offline-identities/request",
        body,
        Some(&token),
    )
    .await;
    assert_eq!(approved["authorization_status"], "APPROVED");
    assert!(approved["identity_id"].is_string());
}

#[tokio::test]
async fn activation_cas_replaces_binding_and_rotation_invalidates_old_key() {
    let (_dir, state) = setup();
    let (status, issued, _) = request(
        &state,
        "POST",
        "/account/api/offline-identities",
        json!({"label":"Device A"}),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let key = issued["offline_key"].as_str().unwrap();
    let identity = issued["identity_id"].as_str().unwrap();
    let activate = json!({"offline_key":key,"entity_uuid":"11111111-1111-4111-8111-111111111111","profile_name":"测试角色","expected_binding_revision":0});
    let (status, session, no_store) = request(
        &state,
        "POST",
        "/v1/offline-identities/activate",
        activate.clone(),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(no_store);
    assert_eq!(session["binding_revision"], 1);
    let (status, stale, _) = request(
        &state,
        "POST",
        "/v1/offline-identities/activate",
        activate.clone(),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(stale["code"], "OFFLINE_BINDING_STALE");
    let path = format!("/account/api/offline-identities/{identity}/rotate");
    let (status, rotated, _) = request(
        &state,
        "POST",
        &path,
        json!({"expected_key_generation":1}),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rotated["identity_id"], identity);
    let (status, invalid, _) = request(
        &state,
        "POST",
        "/v1/offline-identities/activate",
        activate,
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(invalid["code"], "OFFLINE_KEY_INVALID");
}

#[tokio::test]
async fn disabled_experiment_blocks_creation_but_preserves_read_and_revoke() {
    let (_dir, state) = setup();
    let (status, issued, _) = request(
        &state,
        "POST",
        "/account/api/offline-identities",
        json!({"label":"Device"}),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let identity = issued["identity_id"].as_str().unwrap();
    let mut config = state.runtime_config();
    config.offline_identity.authorization_mode =
        spm_cloud::offline_identity::OfflineAuthorizationMode::Disabled;
    state.update_runtime_config(config).unwrap();
    let (status, error, _) = request(
        &state,
        "POST",
        "/account/api/offline-identities",
        json!({"label":"Blocked"}),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["code"], "OFFLINE_IDENTITY_DISABLED");
    let (status, _, _) = request(
        &state,
        "GET",
        "/account/api/offline-identities",
        json!({}),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let path = format!("/account/api/offline-identities/{identity}/key");
    for _ in 0..2 {
        let (status, _, _) = request(&state, "DELETE", &path, json!({}), Some(TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
    }
}

async fn publish(
    state: &AppState,
    session: &Value,
    body: Value,
    token: &str,
) -> (StatusCode, Value) {
    let path = format!(
        "/v1/offline-player-sessions/{}/appearance",
        session["session_id"].as_str().unwrap()
    );
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header(
                    "X-SPM-Offline-Session",
                    session["session_token"].as_str().unwrap(),
                )
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn offline_publication_is_opt_in_for_observers_and_old_sessions_cannot_publish() {
    let (dir, state) = setup();
    let (_, issued, _) = request(
        &state,
        "POST",
        "/account/api/offline-identities",
        json!({"label":"A"}),
        Some(TOKEN),
    )
    .await;
    let uuid = "11111111-1111-4111-8111-111111111111";
    let activate = json!({"offline_key":issued["offline_key"],"entity_uuid":uuid,"profile_name":"A","expected_binding_revision":0});
    let (_, first, _) = request(
        &state,
        "POST",
        "/v1/offline-identities/activate",
        activate.clone(),
        Some(TOKEN),
    )
    .await;
    let asset = dir.path().join("model");
    std::fs::write(&asset, b"test").unwrap();
    state
        .store
        .register_asset_with_visibility(
            "account_local",
            "public-model",
            "Test",
            "ysm",
            "test-sha",
            4,
            &asset,
            None,
            Some("PUBLIC"),
        )
        .unwrap();
    let body = json!({"binding_revision":first["binding_revision"],"expected_revision":first["appearance_revision"],"asset_id":"public-model","asset_revision":1,"raw_sha256":"test-sha","texture_id":"texture-a","motion":{"event_id":"play","animation_key":"dance","started_at_unix_ms":0}});
    let (status, result) = publish(&state, &first, body.clone(), TOKEN).await;
    assert_eq!(status, StatusCode::OK);
    assert!(result["revision"].as_i64().is_some());
    let (_, legacy, _) = request(
        &state,
        "POST",
        "/v1/players/appearances/query",
        json!({"entity_uuids":[uuid]}),
        Some(TOKEN),
    )
    .await;
    assert!(legacy["entries"][0]["selection"].is_null());
    let (_, query, _) = request(
        &state,
        "POST",
        "/v1/players/appearances/query",
        json!({"entity_uuids":[uuid],"include_offline":true}),
        Some(TOKEN),
    )
    .await;
    assert_eq!(query["entries"].as_array().unwrap().len(), 1);
    assert_eq!(query["entries"][0]["selection"]["texture_id"], "texture-a");
    assert_eq!(query["entries"][0]["identity_kind"], "CLOUD_OFFLINE");
    let mut next = activate;
    next["expected_binding_revision"] = json!(1);
    next["entity_uuid"] = json!("22222222-2222-4222-8222-222222222222");
    let (status, _, _) = request(
        &state,
        "POST",
        "/v1/offline-identities/activate",
        next,
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, error) = publish(&state, &first, body, TOKEN).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["code"], "OFFLINE_BINDING_STALE");
    let (_, old, _) = request(
        &state,
        "POST",
        "/v1/players/appearances/query",
        json!({"entity_uuids":[uuid],"include_offline":true}),
        Some(TOKEN),
    )
    .await;
    assert!(old["entries"][0]["selection"].is_null());
}

#[tokio::test]
async fn linked_web_account_issues_to_cloud_owner_and_other_accounts_cannot_use_key() {
    let (_dir, state) = setup();
    for account in ["web_user", "cloud_user", "other_user"] {
        state
            .store
            .create_account(account, "isolated-password")
            .unwrap();
    }
    let login = |account: &str| {
        state
            .store
            .issue_session(&spm_cloud::models::LoginRequest {
                account_id: account.into(),
                password: "isolated-password".into(),
            })
            .unwrap()
            .access_token
    };
    let web = login("web_user");
    let cloud = login("cloud_user");
    let other = login("other_user");
    let (status, _, _) = request(
        &state,
        "POST",
        "/account/api/link-cloud",
        json!({"account_id":"cloud_user","password":"isolated-password"}),
        Some(&web),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, issued, _) = request(
        &state,
        "POST",
        "/account/api/offline-identities",
        json!({"label":"Linked"}),
        Some(&web),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, listed, _) = request(
        &state,
        "GET",
        "/v1/offline-identities/me",
        json!({}),
        Some(&cloud),
    )
    .await;
    assert_eq!(listed["account_id"], "cloud_user");
    assert_eq!(listed["identities"].as_array().unwrap().len(), 1);
    let input = json!({"offline_key":issued["offline_key"],"entity_uuid":uuid::Uuid::new_v4(),"profile_name":"Linked","expected_binding_revision":0});
    for token in [&web, &other] {
        let (status, error, _) = request(
            &state,
            "POST",
            "/v1/offline-identities/activate",
            input.clone(),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(error["code"], "OFFLINE_KEY_INVALID");
    }
    let (status, _, _) = request(
        &state,
        "POST",
        "/v1/offline-identities/activate",
        input,
        Some(&cloud),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let db = rusqlite::Connection::open(&state.config.database_path).unwrap();
    let stored: String = db
        .query_row(
            "SELECT secret_hash FROM cloud_offline_keys WHERE key_id=?1",
            [issued["key_id"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!stored.contains(issued["offline_key"].as_str().unwrap()));
    assert_eq!(stored.len(), 64);
    let secret_leaks: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE details_json LIKE ?1",
            [format!("%{}%", issued["offline_key"].as_str().unwrap())],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(secret_leaks, 0);
}

#[tokio::test]
async fn multiple_offline_claims_are_ambiguous_and_verified_empty_selection_takes_precedence() {
    let (_dir, state) = setup();
    let uuid = uuid::Uuid::new_v4();
    for label in ["A", "B"] {
        let (_, issued, _) = request(
            &state,
            "POST",
            "/account/api/offline-identities",
            json!({"label":label}),
            Some(TOKEN),
        )
        .await;
        let (status,_,_)=request(&state,"POST","/v1/offline-identities/activate",json!({"offline_key":issued["offline_key"],"entity_uuid":uuid,"profile_name":label,"expected_binding_revision":0}),Some(TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
    }
    let query = json!({"entity_uuids":[uuid],"include_offline":true});
    let (_, result, _) = request(
        &state,
        "POST",
        "/v1/players/appearances/query",
        query.clone(),
        Some(TOKEN),
    )
    .await;
    assert_eq!(result["entries"].as_array().unwrap().len(), 1);
    assert_eq!(result["entries"][0]["discovery_status"], "AMBIGUOUS");
    assert!(result["entries"][0]["selection"].is_null());
    let db = rusqlite::Connection::open(&state.config.database_path).unwrap();
    db.execute("INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('verified','account_local','official',?1,'Verified',1)",[uuid.to_string()]).unwrap();
    drop(db);
    state
        .store
        .publish_player(
            "account_local",
            &serde_json::from_value(
                json!({"identity_id":"verified","entity_uuid":uuid,"expected_revision":0}),
            )
            .unwrap(),
        )
        .unwrap();
    let (_, result, _) = request(
        &state,
        "POST",
        "/v1/players/appearances/query",
        query,
        Some(TOKEN),
    )
    .await;
    assert_eq!(result["entries"][0]["identity_kind"], "VERIFIED");
    assert!(result["entries"][0]["selection"].is_null());
}

#[tokio::test]
async fn restart_and_hot_disable_never_revive_prior_sessions() {
    let (_dir, state) = setup();
    let (_, issued, _) = request(
        &state,
        "POST",
        "/account/api/offline-identities",
        json!({"label":"A"}),
        Some(TOKEN),
    )
    .await;
    let activate = json!({"offline_key":issued["offline_key"],"entity_uuid":uuid::Uuid::new_v4(),"profile_name":"A","expected_binding_revision":0});
    let (_, session, _) = request(
        &state,
        "POST",
        "/v1/offline-identities/activate",
        activate,
        Some(TOKEN),
    )
    .await;
    let body = json!({"binding_revision":session["binding_revision"],"expected_revision":session["appearance_revision"]});
    let mut config = state.runtime_config();
    config.offline_identity.authorization_mode =
        spm_cloud::offline_identity::OfflineAuthorizationMode::Disabled;
    state.update_runtime_config(config.clone()).unwrap();
    config.offline_identity.authorization_mode =
        spm_cloud::offline_identity::OfflineAuthorizationMode::Automatic;
    state.update_runtime_config(config).unwrap();
    let (status, _) = publish(&state, &session, body.clone(), TOKEN).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, listed, _) = request(
        &state,
        "GET",
        "/v1/offline-identities/me",
        json!({}),
        Some(TOKEN),
    )
    .await;
    let revision = listed["identities"][0]["binding_revision"].clone();
    let (status,fresh,_)=request(&state,"POST","/v1/offline-identities/activate",json!({"offline_key":issued["offline_key"],"entity_uuid":uuid::Uuid::new_v4(),"profile_name":"A","expected_binding_revision":revision}),Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    let restarted = spm_cloud::CloudStore::open(&state.runtime_config()).unwrap();
    let state2 = AppState::new(state.runtime_config(), restarted);
    let (status,_)=publish(&state2,&fresh,json!({"binding_revision":fresh["binding_revision"],"expected_revision":fresh["appearance_revision"]}),TOKEN).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn failed_key_attempts_are_rate_limited_too() {
    let (_dir, state) = setup();
    let mut config = state.runtime_config();
    config.offline_identity.activation_requests_per_minute = 2;
    state.update_runtime_config(config).unwrap();
    let invalid = json!({"offline_key":format!("spm-offline-v1.cok_{}.{}","a".repeat(32),"b".repeat(43)),"entity_uuid":uuid::Uuid::new_v4(),"profile_name":"A","expected_binding_revision":0});
    for _ in 0..2 {
        let (status, _, _) = request(
            &state,
            "POST",
            "/v1/offline-identities/activate",
            invalid.clone(),
            Some(TOKEN),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let (status, error, _) = request(
        &state,
        "POST",
        "/v1/offline-identities/activate",
        invalid,
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error["code"], "RATE_LIMITED");
}
