use axum::{
    Json,
    extract::{Multipart, State},
    routing::post,
};
use mysql::prelude::Queryable;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use spm_cloud::{
    api::AppState,
    config_file::{Environment, LoadedConfig},
    moderation::{ModerationSettings, scan},
};

const OWNER_TOKEN: &str = "web-owner-token-for-integration-tests";
struct Server {
    url: String,
    state: AppState,
    task: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
    mysql_cleanup: Option<(String, String)>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
        if let Some((url, database)) = &self.mysql_cleanup {
            if let Ok(options) = mysql::Opts::from_url(url) {
                let mut connection = mysql::Conn::new(
                    mysql::OptsBuilder::from_opts(options).db_name(None::<String>),
                )
                .ok();
                if let Some(connection) = connection.as_mut() {
                    let _ = connection.query_drop(format!("DROP DATABASE `{database}`"));
                }
            }
        }
    }
}
async fn server() -> Server {
    server_with_sqlite_source(None).await
}
async fn server_with_sqlite_source(sqlite_source: Option<&std::path::Path>) -> Server {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("token.txt"), OWNER_TOKEN).unwrap();
    let config = dir.path().join("config.json");
    let mut config_json = json!({"auth":{"bootstrap_access_token_file":"token.txt"}});
    if let Some(source) = sqlite_source {
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        std::fs::copy(source, dir.path().join("data/spm-cloud.db")).unwrap();
    }
    let mut mysql_cleanup = None;
    if let Ok(mysql_url) = std::env::var("SPM_TEST_MYSQL_URL") {
        let database = format!("spm_test_{}", uuid::Uuid::new_v4().simple());
        let options = mysql::Opts::from_url(&mysql_url).unwrap();
        let mut connection =
            mysql::Conn::new(mysql::OptsBuilder::from_opts(options).db_name(None::<String>))
                .unwrap();
        connection
            .query_drop(format!(
                "CREATE DATABASE `{database}` CHARACTER SET utf8mb4 COLLATE utf8mb4_bin"
            ))
            .unwrap();
        drop(connection);
        let mut database_url = reqwest::Url::parse(&mysql_url).unwrap();
        database_url.set_path(&format!("/{database}"));
        std::fs::write(dir.path().join("mysql-url.txt"), database_url.as_str()).unwrap();
        config_json["storage"] = json!({"backend":"mysql","mysql_url_file":"mysql-url.txt"});
        mysql_cleanup = Some((mysql_url, database));
    }
    if let Ok(redis_url) = std::env::var("SPM_TEST_REDIS_URL") {
        std::fs::write(dir.path().join("redis-url.txt"), redis_url).unwrap();
        config_json["cache"] = json!({"backend":"redis","redis_url_file":"redis-url.txt","ttl_seconds":20,"max_pages":16,"timeout_ms":100});
    }
    std::fs::write(&config, config_json.to_string()).unwrap();
    let loaded = LoadedConfig::load_with(Some(&config), dir.path(), &Environment::new()).unwrap();
    let store = spm_cloud::CloudStore::open(&loaded.cloud).unwrap();
    let state = AppState::new(loaded.cloud, store);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = spm_cloud::api::router(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Server {
        url,
        state,
        task,
        _dir: dir,
        mysql_cleanup,
    }
}
async fn login(client: &Client, server: &Server, account: &str, password: &str) -> Value {
    let response = client
        .post(format!("{}/admin/api/login", server.url))
        .json(&json!({"account_id":account,"password":password}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response.json().await.unwrap()
}
async fn upload(client: &Client, server: &Server, id: &str, body: &str) -> reqwest::Response {
    client
        .post(format!("{}/admin/api/assets", server.url))
        .bearer_auth(OWNER_TOKEN)
        .header("Idempotency-Key", uuid::Uuid::new_v4().to_string())
        .header("X-Asset-Id", id)
        .header("X-Asset-Name", "model")
        .header("X-Asset-Format", "bbmodel")
        .header("X-Asset-Visibility", "PUBLIC")
        .body(body.to_owned())
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn catalog_returns_page_scoped_ownership_without_prefetching() {
    let s = server().await;
    let client = Client::new();
    for id in ["page-a", "page-b", "page-c"] {
        assert!(
            upload(&client, &s, id, r#"{"elements":[]}"#)
                .await
                .status()
                .is_success()
        );
    }
    let page: Value = client
        .get(format!("{}/v1/assets?scope=accessible&limit=2", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page["entries"].as_array().unwrap().len(), 2);
    assert_eq!(page["has_more"], true);
    assert_eq!(page["entries"][0]["owned"], true);
    assert_eq!(
        page["entries"][0]["owner_account_id"],
        s.state.config.bootstrap_account_id
    );
    let after = page["next_cursor"].as_str().unwrap();
    let next: Value = client
        .get(format!(
            "{}/v1/assets?scope=accessible&limit=2&after={after}",
            s.url
        ))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(next["entries"].as_array().unwrap().len(), 1);
    assert_eq!(next["has_more"], false);
}

#[tokio::test]
async fn mysql_backend_serves_accounts_catalog_and_sqlite_restore_compatible_backups() {
    if std::env::var("SPM_TEST_MYSQL_URL").is_err() {
        return;
    }
    let old_store = tempfile::tempdir().unwrap();
    let old_db = old_store.path().join("spm-cloud.db");
    {
        let legacy = rusqlite::Connection::open(&old_db).unwrap();
        legacy.execute_batch("CREATE TABLE accounts(account_id TEXT PRIMARY KEY,created_at TEXT NOT NULL); INSERT INTO accounts VALUES('account_local','2024-01-01 00:00:00'),('legacy-account','2024-01-02 00:00:00');").unwrap();
    }
    let s = server_with_sqlite_source(Some(&old_db)).await;
    let client = Client::new();
    let imported: Value = client
        .get(format!("{}/admin/api/accounts?limit=100", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        imported.to_string().contains("legacy-account"),
        "existing SQLite accounts should be imported on first MySQL startup"
    );
    let uploaded = upload(&client, &s, "mysql-backend-model", r#"{"elements":[]}"#).await;
    assert!(
        uploaded.status().is_success(),
        "{}",
        uploaded.text().await.unwrap()
    );
    let page: Value = client
        .get(format!("{}/v1/assets?scope=mine&limit=1", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page["entries"][0]["asset_id"], "mysql-backend-model");
    let response = client
        .get(format!("{}/admin/api/exports/backup", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(response.bytes().await.unwrap())).unwrap();
    let database_name = archive
        .file_names()
        .find(|name| name.ends_with("spm-cloud.db"))
        .unwrap()
        .to_owned();
    let mut exported = archive.by_name(&database_name).unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut exported, &mut bytes).unwrap();
    let export_path = s._dir.path().join("restored.db");
    std::fs::write(&export_path, bytes).unwrap();
    let restored = rusqlite::Connection::open(export_path).unwrap();
    let model_count: i64 = restored
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE asset_id='mysql-backend-model'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        model_count, 1,
        "MySQL backup export must produce a populated, SQLite-restorable database"
    );
}

#[test]
fn optional_cache_configuration_accepts_memory_redis_and_disables_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    for backend in ["memory", "redis", "none"] {
        std::fs::write(dir.path().join("redis-url.txt"), "redis://127.0.0.1:6379/0").unwrap();
        std::fs::write(&config, json!({"cache":{"backend":backend,"redis_url_file":"redis-url.txt","ttl_seconds":20,"max_pages":8}}).to_string()).unwrap();
        let loaded = LoadedConfig::load_with(Some(&config), dir.path(), &Environment::new());
        assert!(loaded.is_ok(), "{backend} must be supported");
    }
}

#[test]
fn mysql_configuration_is_optional_and_redacts_connection_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    std::fs::write(
        dir.path().join("mysql-url.txt"),
        "mysql://tester:secret-password@localhost:3306/spm_test",
    )
    .unwrap();
    std::fs::write(
        &config,
        json!({"storage":{"backend":"mysql","mysql_url_file":"mysql-url.txt"}}).to_string(),
    )
    .unwrap();
    let loaded = LoadedConfig::load_with(Some(&config), dir.path(), &Environment::new())
        .expect("MySQL config should validate without connecting");
    assert!(
        !loaded
            .effective_report()
            .to_string()
            .contains("secret-password")
    );
    assert!(!format!("{:?}", loaded.cloud).contains("secret-password"));
}

#[tokio::test]
async fn paged_owner_first_preview_exports_and_search_are_authorized_and_complete() {
    use std::io::Read;
    let s = server().await;
    let client = Client::new();
    s.state
        .store
        .create_account("aaa-first", "fixture-password")
        .unwrap();
    s.state
        .store
        .create_account("zzz-last", "fixture-password")
        .unwrap();
    let accounts: Value = client
        .get(format!("{}/admin/api/accounts?page=1&page_size=1", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(accounts["entries"][0]["role"], "owner");
    assert_eq!(accounts["total"], 3);
    let page: Value = client
        .get(format!("{}/admin/api/accounts?page=2&page_size=1", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page["entries"][0]["account_id"], "aaa-first");
    let body = r#"{"elements":[{"uuid":"cube","from":[0,0,0],"to":[8,16,8]}]}"#;
    assert!(
        upload(&client, &s, "asset-one", body)
            .await
            .status()
            .is_success()
    );
    assert!(
        upload(&client, &s, "asset-two", body)
            .await
            .status()
            .is_success()
    );
    let list: Value = client
        .get(format!(
            "{}/admin/api/assets?page=2&page_size=1&q=bbmodel",
            s.url
        ))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list["total"], 2);
    assert_eq!(list["entries"].as_array().unwrap().len(), 1);
    assert_eq!(list["entries"][0]["byte_length"], body.len());
    let preview: Value = client
        .get(format!("{}/admin/api/assets/asset-one/preview", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(preview["documents"][0]["value"]["elements"][0]["to"][1], 16);
    let exported = client
        .get(format!("{}/admin/api/assets/asset-one/export", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap();
    assert!(
        exported.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .ends_with(".bbmodel\"")
    );
    assert_eq!(exported.text().await.unwrap(), body);
    let backup = client
        .get(format!("{}/admin/api/exports/backup", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(backup.status(), StatusCode::OK);
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(backup.bytes().await.unwrap())).unwrap();
    let manifest: Value =
        serde_json::from_reader(archive.by_name("manifest.json").unwrap()).unwrap();
    assert_eq!(manifest["schema"], "spm.cloud.backup.v1");
    assert_eq!(manifest["objects"]["file_count"], 1);
    let mut bytes = Vec::new();
    archive
        .by_name("spm-cloud.db")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let snapshot = s._dir.path().join("snapshot.db");
    std::fs::write(&snapshot, bytes).unwrap();
    let db = rusqlite::Connection::open(snapshot).unwrap();
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
    let logs = client
        .get(format!("{}/admin/api/exports/logs", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(logs.status(), StatusCode::OK);
    let audit: Value = client
        .get(format!(
            "{}/admin/api/audit?q=MODEL_EXPORTED&page=1&page_size=1",
            s.url
        ))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(audit["total"], 1);
    let exported: Value = client
        .get(format!(
            "{}/admin/api/exports/audit?q=MODEL_EXPORTED",
            s.url
        ))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(exported["entries"].as_array().unwrap().len(), 1);
    for path in [
        "/admin/api/exports/backup",
        "/admin/api/exports/models",
        "/admin/api/exports/logs",
        "/admin/api/assets/asset-one/preview",
    ] {
        assert_eq!(
            client
                .get(format!("{}{path}", s.url))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    s.state
        .store
        .set_admin_account(
            "account_local",
            "aaa-first",
            "account_local",
            "admin",
            false,
            None,
        )
        .unwrap();
    let admin = login(&client, &s, "aaa-first", "fixture-password").await;
    assert_eq!(
        client
            .get(format!("{}/admin/api/exports/backup", s.url))
            .bearer_auth(admin["access_token"].as_str().unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn saved_custom_zero_concurrency_reaches_live_admission_and_config_revision() {
    let s = server().await;
    let client = Client::new();
    let config: Value = client
        .get(format!("{}/admin/api/config", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut value = config["config"].clone();
    value["performance"] =
        json!({"preset":"custom","upload_concurrency":0,"model_processing_concurrency":0});
    let response = client
        .put(format!("{}/admin/api/config", s.url))
        .bearer_auth(OWNER_TOKEN)
        .json(&json!({"config":value,"revision":config["revision"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let info: Value = client
        .get(format!("{}/admin/api/maintenance", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(info["upload_concurrency"], 0);
    assert_eq!(info["model_processing_concurrency"], 0);
    assert_eq!(
        s.state.runtime_config().performance.resolved().unwrap(),
        (0, 0)
    );
}

#[tokio::test]
async fn owner_distributes_admins_and_revocation_invalidates_sessions() {
    let s = server().await;
    let client = Client::new();
    let page = client
        .get(format!("{}/admin/", s.url))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert!(
        page.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert!(page.headers().get("access-control-allow-origin").is_none());
    assert!(
        page.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("font-src 'self'")
    );
    assert!(page.text().await.unwrap().contains("服主分发"));
    for path in [
        "/admin/app.js",
        "/admin/style.css",
        "/admin/fonts.css",
        "/admin/icons.svg",
        "/admin/credits",
    ] {
        assert_eq!(
            client
                .get(format!("{}{path}", s.url))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    let font = client
        .get(format!("{}/admin/fonts/HarmonyOS_Sans_Regular.ttf", s.url))
        .send()
        .await
        .unwrap();
    assert_eq!(font.status(), StatusCode::OK);
    assert_eq!(font.headers()["content-type"], "font/ttf");
    assert_eq!(font.headers()["cache-control"], "public, max-age=604800");
    assert_eq!(&font.bytes().await.unwrap()[..4], &[0, 1, 0, 0]);
    assert_eq!(
        client
            .get(format!("{}/admin/fonts/config.json", s.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        client
            .get(format!("{}/admin/api/assets", s.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let bootstrap: Value = client
        .post(format!("{}/admin/api/login", s.url))
        .json(&json!({"bootstrap_token":OWNER_TOKEN}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(bootstrap["role"], "owner");
    s.state
        .store
        .create_account("ordinary", "ordinary-password")
        .unwrap();
    let ordinary = s
        .state
        .store
        .issue_session(&spm_cloud::models::LoginRequest {
            account_id: "ordinary".into(),
            password: "ordinary-password".into(),
        })
        .unwrap();
    assert_eq!(
        client
            .post(format!("{}/admin/api/accounts", s.url))
            .bearer_auth(&ordinary.access_token)
            .json(&json!({"account_id":"promoted","password":"password","role":"admin"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .post(format!("{}/admin/api/login", s.url))
            .json(&json!({"account_id":"ordinary","password":"ordinary-password"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .post(format!("{}/admin/api/accounts", s.url))
            .bearer_auth(OWNER_TOKEN)
            .json(&json!({"account_id":"staff","password":"distributed-password","role":"admin"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let staff = login(&client, &s, "staff", "distributed-password").await;
    assert_eq!(staff["role"], "admin");
    let token = staff["access_token"].as_str().unwrap();
    assert_eq!(
        client
            .get(format!("{}/admin/api/assets", s.url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .get(format!("{}/admin/api/config", s.url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .get(format!("{}/admin/api/accounts", s.url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .post(format!("{}/admin/api/accounts", s.url))
            .bearer_auth(token)
            .json(&json!({"account_id":"rogue","password":"distributed-password","role":"admin"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let config: Value = client
        .get(format!("{}/admin/api/config", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        client
            .put(format!("{}/admin/api/config", s.url))
            .bearer_auth(token)
            .json(&json!({"config":config["config"],"revision":config["revision"]}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .put(format!("{}/admin/api/accounts/staff", s.url))
            .bearer_auth(OWNER_TOKEN)
            .json(&json!({"role":"admin","disabled":true}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        client
            .get(format!("{}/admin/api/assets", s.url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(
        s.state
            .store
            .refresh_session(staff["refresh_token"].as_str().unwrap())
            .is_err()
    );
    assert_eq!(
        client
            .post(format!("{}/admin/api/login", s.url))
            .json(&json!({"account_id":"staff","password":"distributed-password"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .put(format!("{}/admin/api/accounts/account_local", s.url))
            .bearer_auth(OWNER_TOKEN)
            .json(&json!({"role":"member","disabled":true}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        client
            .put(format!("{}/admin/api/accounts/staff", s.url))
            .bearer_auth(OWNER_TOKEN)
            .json(&json!({"role":"admin","disabled":false,"password":"replacement-password"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    login(&client, &s, "staff", "replacement-password").await;
    assert_eq!(
        client
            .put(format!("{}/admin/api/accounts/account_local", s.url))
            .bearer_auth(OWNER_TOKEN)
            .json(&json!({"role":"owner","disabled":false,"password":"new-owner-password"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let owner = login(&client, &s, "account_local", "new-owner-password").await;
    assert_eq!(owner["role"], "owner");
}

#[tokio::test]
async fn config_save_validates_conflicts_hot_reload_and_ui_disable() {
    let s = server().await;
    let client = Client::new();
    let endpoint = format!("{}/admin/api/config", s.url);
    let response = client
        .get(&endpoint)
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap();
    let text = response.text().await.unwrap();
    assert!(!text.contains(OWNER_TOKEN));
    let initial: Value = serde_json::from_str(&text).unwrap();
    let mut config = serde_json::to_value(spm_cloud::config_file::FileConfig::default()).unwrap();
    config["auth"]["bootstrap_access_token_file"] =
        initial["config"]["auth"]["bootstrap_access_token_file"].clone();
    config["moderation"] = json!({"block_r18":true,"detection_enabled":false});
    config["limits"] = json!({"max_asset_bytes":4096,"max_entity_query_count":1,"max_message_bytes":1024,"max_visual_state_bytes":1024});
    let save = client
        .put(&endpoint)
        .bearer_auth(OWNER_TOKEN)
        .json(&json!({"config":config,"revision":initial["revision"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), StatusCode::OK);
    assert!(s.state.runtime_config().moderation.block_r18);
    assert_eq!(s.state.runtime_config().max_asset_bytes, 4096);
    assert!(matches!(
        s.state.store.entity_entries(
            "account_local",
            "valid-world",
            &[uuid::Uuid::new_v4(), uuid::Uuid::new_v4()]
        ),
        Err(spm_cloud::error::CloudError::InvalidMetadata(_))
    ));

    assert_eq!(
        client
            .put(&endpoint)
            .bearer_auth(OWNER_TOKEN)
            .json(&json!({"config":config,"revision":initial["revision"]}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let saved: Value = client
        .get(&endpoint)
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    config["moderation"]["score_threshold"] = json!(1.2);
    assert_eq!(
        client
            .put(&endpoint)
            .bearer_auth(OWNER_TOKEN)
            .json(&json!({"config":config,"revision":saved["revision"]}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let reread: Value = client
        .get(&endpoint)
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved["revision"], reread["revision"]);
    config = saved["config"].clone();
    config["web_ui"] = json!({"enabled":false});
    assert_eq!(
        client
            .put(&endpoint)
            .bearer_auth(OWNER_TOKEN)
            .json(&json!({"config":config,"revision":saved["revision"]}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .get(format!("{}/admin/", s.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        client
            .get(&endpoint)
            .bearer_auth(OWNER_TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn independent_recognition_and_blocking_plus_manual_mark_and_reversible_delete() {
    let s = server().await;
    let client = Client::new();
    for detection in [false, true] {
        for block in [false, true] {
            let mut runtime = s.state.runtime_config();
            runtime.moderation.detection_enabled = detection;
            runtime.moderation.block_r18 = block;
            s.state.update_runtime_config(runtime).unwrap();
            let id = format!("combination-{detection}-{block}");
            let response = upload(
                &client,
                &s,
                &id,
                &json!({"r18":true,"unique":id}).to_string(),
            )
            .await;
            assert_eq!(
                response.status(),
                if detection && block {
                    StatusCode::FORBIDDEN
                } else {
                    StatusCode::CREATED
                }
            );
        }
    }
    let mut runtime = s.state.runtime_config();
    runtime.moderation.detection_enabled = false;
    runtime.moderation.block_r18 = false;
    s.state.update_runtime_config(runtime).unwrap();
    let response = upload(
        &client,
        &s,
        "manual-model",
        r#"{"metadata":{"title":"model"}}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let asset: Value = response.json().await.unwrap();
    let mark = format!("{}/admin/api/assets/manual-model/moderation", s.url);
    assert_eq!(
        client
            .put(&mark)
            .bearer_auth(OWNER_TOKEN)
            .json(&json!({"status":"R18"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let mut runtime = s.state.runtime_config();
    runtime.moderation.block_r18 = true;
    s.state.update_runtime_config(runtime).unwrap();
    let download = format!("{}/v1/assets/manual-model/revisions/1/content", s.url);
    assert_ne!(
        client
            .get(&download)
            .bearer_auth(OWNER_TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let list: Value = client
        .get(format!("{}/v1/assets", s.url))
        .bearer_auth(OWNER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        !list["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["asset_id"] == "manual-model")
    );
    // Re-uploading identical bytes under another ID cannot bypass a content-addressed manual verdict.
    assert_eq!(
        upload(
            &client,
            &s,
            "same-bytes",
            r#"{"metadata":{"title":"model"}}"#
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .put(&mark)
            .bearer_auth(OWNER_TOKEN)
            .json(&json!({"status":"CLEAN"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        client
            .get(&download)
            .bearer_auth(OWNER_TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let deleted = format!("{}/admin/api/assets/manual-model", s.url);
    assert_eq!(
        client
            .delete(&deleted)
            .bearer_auth(OWNER_TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_ne!(
        client
            .get(&download)
            .bearer_auth(OWNER_TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        upload(
            &client,
            &s,
            "manual-model",
            r#"{"metadata":{"title":"changed"}}"#
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert!(
        s.state
            .store
            .object_path_for_sha(asset["raw_sha256"].as_str().unwrap())
            .exists()
    );
    assert_eq!(
        client
            .post(format!("{deleted}/restore"))
            .bearer_auth(OWNER_TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        client
            .get(&download)
            .bearer_auth(OWNER_TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let mut runtime = s.state.runtime_config();
    runtime.moderation.block_unclassified = true;
    s.state.update_runtime_config(runtime).unwrap();
    assert_eq!(
        upload(&client, &s, "unclassified", "encrypted-or-unreadable")
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

async fn scanner(State(reply): State<Value>, mut multipart: Multipart) -> Json<Value> {
    let field = multipart.next_field().await.unwrap().unwrap();
    assert_eq!(field.name(), Some("f1"));
    assert_eq!(field.bytes().await.unwrap().as_ref(), b"image-payload");
    Json(reply)
}
#[tokio::test]
async fn texture_scanner_contract_threshold_failure_and_extraction_budgets() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("model.bbmodel");
    std::fs::write(
        &file,
        r#"{"textures":[{"source":"data:image/png;base64,aW1hZ2UtcGF5bG9hZA=="}]}"#,
    )
    .unwrap();
    let glb = dir.path().join("model.glb");
    let metadata=json!({"buffers":[{"byteLength":13}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":13}],"images":[{"bufferView":0,"mimeType":"image/png"}]}).to_string();
    let mut json_bytes = metadata.into_bytes();
    json_bytes.resize(json_bytes.len().next_multiple_of(4), b' ');
    let mut binary = b"image-payload".to_vec();
    binary.resize(binary.len().next_multiple_of(4), 0);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&((12 + 8 + json_bytes.len() + 8 + binary.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0x4e4f534au32.to_le_bytes());
    bytes.extend_from_slice(&json_bytes);
    bytes.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0x004e4942u32.to_le_bytes());
    bytes.extend_from_slice(&binary);
    std::fs::write(&glb, bytes).unwrap();
    for (reply, threshold, expected) in [
        (
            json!({"success":true,"prediction":[[{"class":"FEMALE_BREAST_EXPOSED","score":0.8}]]}),
            0.7,
            "R18",
        ),
        (
            json!({"success":true,"prediction":[[{"class":"FEMALE_BREAST_EXPOSED","score":0.8}]]}),
            0.9,
            "CLEAN",
        ),
        (json!({"success":true,"prediction":[[]]}), 0.0, "CLEAN"),
        (json!({"success":false}), 0.7, "UNKNOWN"),
        (json!({"success":true,"prediction":[]}), 0.7, "UNKNOWN"),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/infer", listener.local_addr().unwrap());
        let router = axum::Router::new()
            .route("/infer", post(scanner))
            .with_state(reply);
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let settings = ModerationSettings {
            detection_enabled: true,
            texture_detection_enabled: true,
            scanner_url: Some(endpoint),
            score_threshold: threshold,
            ..Default::default()
        };
        let result = scan(&file, "model", &settings).await;
        assert_eq!(scan(&glb, "model", &settings).await.status, expected);
        // A user-supplied display name must not poison another asset's shared SHA verdict.
        assert_eq!(scan(&file, "NSFW R18", &settings).await.status, expected);
        task.abort();
        assert_eq!(result.status, expected);
        if expected != "UNKNOWN" {
            assert_eq!(result.textures_scanned, 1);
        }
    }
    let settings = ModerationSettings {
        detection_enabled: true,
        texture_detection_enabled: true,
        scanner_url: Some("http://127.0.0.1:1/infer".into()),
        ..Default::default()
    };
    assert_eq!(scan(&file, "model", &settings).await.status, "UNKNOWN");
    let archive = dir.path().join("model.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    zip.start_file(
        "textures/large.png",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    std::io::Write::write_all(&mut zip, &vec![0u8; 2048]).unwrap();
    zip.finish().unwrap();
    let bounded = ModerationSettings {
        max_texture_bytes: 1024,
        ..settings
    };
    assert_eq!(scan(&archive, "model", &bounded).await.status, "UNKNOWN");
}

#[tokio::test]
async fn members_link_their_own_accounts_without_management_permissions() {
    let s = server().await;
    let client = Client::new();
    for name in ["web_member", "cloud_member", "stranger"] {
        s.state
            .store
            .create_account(name, "self-service-test-password")
            .unwrap();
    }
    let response = client
        .post(format!("{}/account/api/login", s.url))
        .json(&json!({"account_id":"web_member","password":"self-service-test-password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let login: Value = response.json().await.unwrap();
    let token = login["access_token"].as_str().unwrap();
    assert_eq!(login["role"], "member");
    let link = format!("{}/account/api/link-cloud", s.url);
    let wrong = client
        .post(&link)
        .bearer_auth(token)
        .json(&json!({"account_id":"cloud_member","password":"wrong-password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        s.state.store.linked_cloud_account("web_member").unwrap(),
        "web_member"
    );
    let connected = client
        .post(&link)
        .bearer_auth(token)
        .json(&json!({"account_id":"cloud_member","password":"self-service-test-password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(connected.status(), StatusCode::OK);
    let profile: Value = connected.json().await.unwrap();
    assert_eq!(profile["cloud_account_id"], "cloud_member");
    assert_eq!(profile["role"], "member");
    for path in [
        "/admin/api/config",
        "/admin/api/accounts",
        "/admin/api/model-owners",
    ] {
        assert_eq!(
            client
                .get(format!("{}{path}", s.url))
                .bearer_auth(token)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        client
            .put(format!("{}/admin/api/assets/anything/owner", s.url))
            .bearer_auth(token)
            .json(&json!({"owner_account_id":"stranger","expected_owner_account_id":"web_member"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn public_default_private_owner_and_transfer_enforce_download_permissions() {
    let s = server().await;
    let client = Client::new();
    for name in ["first_owner", "second_owner"] {
        s.state
            .store
            .create_account(name, "ownership-test-password")
            .unwrap();
    }
    let mut tokens = Vec::new();
    for name in ["first_owner", "second_owner"] {
        let session = s
            .state
            .store
            .issue_session(&spm_cloud::models::LoginRequest {
                account_id: name.into(),
                password: "ownership-test-password".into(),
            })
            .unwrap();
        tokens.push(session.access_token);
    }
    let make = |id: &str| {
        client
            .post(format!("{}/admin/api/assets", s.url))
            .bearer_auth(OWNER_TOKEN)
            .header("Idempotency-Key", uuid::Uuid::new_v4().to_string())
            .header("X-Asset-Id", id)
            .header("X-Asset-Name", "ownership-test")
            .header("X-Asset-Format", "bbmodel")
            .body(r#"{"metadata":{"title":"model"}}"#)
    };
    let default = make("default-public").send().await.unwrap();
    assert_eq!(default.status(), StatusCode::CREATED);
    assert_eq!(
        default.json::<Value>().await.unwrap()["visibility"],
        "PUBLIC"
    );
    assert_eq!(
        make("missing-owner")
            .header("X-Asset-Visibility", "PRIVATE")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let private = make("private-model")
        .header("X-Asset-Visibility", "PRIVATE")
        .header("X-Asset-Owner-Account", "first_owner")
        .send()
        .await
        .unwrap();
    assert_eq!(private.status(), StatusCode::CREATED);
    let asset: Value = private.json().await.unwrap();
    assert_eq!(asset["asset_id"], "private-model");
    let download = format!("{}/v1/assets/private-model/revisions/1/content", s.url);
    assert_eq!(
        client
            .get(&download)
            .bearer_auth(&tokens[0])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_ne!(
        client
            .get(&download)
            .bearer_auth(&tokens[1])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let owner_url = format!("{}/admin/api/assets/private-model/owner", s.url);
    let transfer = client
        .put(&owner_url)
        .bearer_auth(OWNER_TOKEN)
        .json(&json!({"owner_account_id":"second_owner","expected_owner_account_id":"first_owner"}))
        .send()
        .await
        .unwrap();
    assert_eq!(transfer.status(), StatusCode::NO_CONTENT);
    assert_ne!(
        client
            .get(&download)
            .bearer_auth(&tokens[0])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .get(&download)
            .bearer_auth(&tokens[1])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .put(&owner_url)
            .bearer_auth(OWNER_TOKEN)
            .json(
                &json!({"owner_account_id":"first_owner","expected_owner_account_id":"first_owner"})
            )
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        client
            .post(format!("{}/v1/assets", s.url))
            .bearer_auth(&tokens[0])
            .header("X-Asset-Owner-Account", "second_owner")
            .body("model")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn oauth_start_protects_binding_and_emits_pkce_browser_cookie() {
    let s = server().await;
    let client = Client::new();
    let mut config = s.state.runtime_config();
    config.web_ui.microsoft_client_id = Some(uuid::Uuid::new_v4().to_string());
    config.web_ui.microsoft_redirect_uri =
        Some(format!("{}/admin/oauth/microsoft/callback", s.url));
    s.state.update_runtime_config(config).unwrap();
    let path = format!("{}/admin/oauth/microsoft/start", s.url);
    assert_eq!(
        client
            .post(&path)
            .json(&json!({"purpose":"link","admin":true}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let start = client
        .post(&path)
        .bearer_auth(OWNER_TOKEN)
        .header("Origin", &s.url)
        .json(&json!({"purpose":"link","admin":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = start.headers()["set-cookie"].to_str().unwrap().to_owned();
    assert!(cookie.contains("HttpOnly; SameSite=Lax"));
    let value: Value = start.json().await.unwrap();
    let url = reqwest::Url::parse(value["url"].as_str().unwrap()).unwrap();
    assert_eq!(url.host_str(), Some("login.microsoftonline.com"));
    let params: std::collections::HashMap<_, _> = url.query_pairs().collect();
    assert_eq!(params["code_challenge_method"], "S256");
    assert_eq!(params["state"].len(), 64);
    assert!(!params.contains_key("client_secret"));
    let callback = format!(
        "{}/admin/oauth/microsoft/callback?state={}&error=access_denied",
        s.url, params["state"]
    );
    let no_redirect = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let missing = no_redirect.get(&callback).send().await.unwrap();
    assert_eq!(missing.status(), StatusCode::SEE_OTHER);
    assert!(
        missing.headers()["location"]
            .to_str()
            .unwrap()
            .contains("oauth_error=IDENTITY_CHALLENGE_EXPIRED")
    );
    let cancelled = no_redirect
        .get(&callback)
        .header("cookie", cookie.split(';').next().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(cancelled.status(), StatusCode::SEE_OTHER);
    assert!(
        cancelled.headers()["location"]
            .to_str()
            .unwrap()
            .contains("oauth_error=INVALID_METADATA")
    );
    let replay = no_redirect
        .get(&callback)
        .header("cookie", cookie.split(';').next().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::SEE_OTHER);
    assert!(
        replay.headers()["location"]
            .to_str()
            .unwrap()
            .contains("oauth_error=IDENTITY_CHALLENGE_EXPIRED")
    );
}

#[tokio::test]
async fn oauth_missing_browser_cookie_returns_actionable_login_page() {
    let s = server().await;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client
        .get(format!(
            "{}/admin/oauth/microsoft/callback?state={}&code=not-a-real-authorization-code",
            s.url,
            "a".repeat(64)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::SEE_OTHER,
        "Missing/expired browser cookies must lead to recovery, not raw unauthenticated JSON"
    );
    let destination = reqwest::Url::parse(&format!(
        "{}{}",
        s.url,
        response.headers()["location"].to_str().unwrap()
    ))
    .unwrap();
    assert_eq!(destination.path(), "/admin/");
    let params: std::collections::HashMap<_, _> = reqwest::Url::parse(&format!(
        "https://localhost/?{}",
        destination.fragment().unwrap()
    ))
    .unwrap()
    .query_pairs()
    .into_owned()
    .collect();
    assert_eq!(params["oauth_error"], "IDENTITY_CHALLENGE_EXPIRED");
    assert!(params["oauth_message"].contains("重新"));
    assert!(!params.contains_key("oauth"));
}
