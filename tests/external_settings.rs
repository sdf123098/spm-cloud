//! Opt-in tests against dedicated disposable services. Never point these URLs at production.
use mysql::prelude::Queryable;
use serde_json::{Value, json};
use spm_cloud::{
    CloudStore,
    api::AppState,
    config_file::{Environment, LoadedConfig},
};

const TOKEN: &str = "external-settings-test-owner";

struct Database {
    root_url: String,
    url: String,
    name: String,
}

impl Database {
    fn isolated() -> Option<Self> {
        let root_url = std::env::var("SPM_TEST_MYSQL_URL").ok()?;
        let mut conn = Self::connect(&root_url);
        let name = format!("spm_settings_{}", uuid::Uuid::new_v4().simple());
        conn.query_drop(format!(
            "CREATE DATABASE `{name}` CHARACTER SET utf8mb4 COLLATE utf8mb4_bin"
        ))
        .unwrap();
        let mut url = reqwest::Url::parse(&root_url).unwrap();
        url.set_path(&name);
        Some(Self {
            root_url,
            url: url.to_string(),
            name,
        })
    }

    fn connect(url: &str) -> mysql::Conn {
        // Suppress URL-bearing errors; these environment values can contain credentials.
        mysql::Conn::new(mysql::Opts::from_url(url).expect("valid test MySQL URL"))
            .unwrap_or_else(|_| panic!("dedicated test MySQL connection failed"))
    }

    fn connection(&self) -> mysql::Conn {
        Self::connect(&self.url)
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        let mut conn = Self::connect(&self.root_url);
        conn.query_drop(format!("DROP DATABASE `{}`", self.name))
            .unwrap();
    }
}

struct Server {
    _dir: tempfile::TempDir,
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn server() -> Server {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("token.txt"), TOKEN).unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(
        &path,
        json!({
            "instance":{"instance_id":format!("external-{}",uuid::Uuid::new_v4().simple())},
            "auth":{"bootstrap_access_token_file":"token.txt"}
        })
        .to_string(),
    )
    .unwrap();
    let loaded = LoadedConfig::load_with(Some(&path), dir.path(), &Environment::new()).unwrap();
    let store = CloudStore::open(&loaded.cloud).unwrap();
    let state = AppState::new(loaded.cloud.clone(), store);
    state.initialize_settings(&loaded);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, spm_cloud::api::router(state))
            .await
            .unwrap();
    });
    Server {
        _dir: dir,
        url,
        task,
    }
}

async fn get(s: &Server, path: &str) -> Value {
    reqwest::Client::new()
        .get(format!("{}{path}", s.url))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn check(s: &Server, body: Value) -> Value {
    reqwest::Client::new()
        .post(format!("{}/admin/api/config/check-connection", s.url))
        .bearer_auth(TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn mysql_connection_probe_is_read_only_and_provider_schema_upgrades_preserve_rows() {
    let Some(db) = Database::isolated() else {
        eprintln!("SKIP: SPM_TEST_MYSQL_URL missing; no real MySQL verification");
        return;
    };
    let s = server().await;
    let options = mysql::Opts::from_url(&db.url).unwrap();
    let connection = json!({"host":options.get_ip_or_hostname(),"port":options.get_tcp_port(),
        "username":options.get_user(),"password":options.get_pass(),"database":db.name});
    let result = check(&s, json!({"kind":"mysql","connection":connection})).await;
    assert_eq!(
        result["connected"], true,
        "probe must connect to the real empty MySQL schema"
    );
    let mut raw = db.connection();
    let tables: Vec<String> = raw.query("SHOW TABLES").unwrap();
    assert!(
        tables.is_empty(),
        "connection checks must not migrate or import tables"
    );
    std::fs::write(s._dir.path().join("candidate-mysql.txt"), &db.url).unwrap();
    let current = get(&s, "/admin/api/config").await;
    let mut draft = current["config"].clone();
    draft["storage"] = json!({"backend":"mysql","mysql_url_file":"candidate-mysql.txt"});
    let result = check(
        &s,
        json!({"kind":"mysql","draft":{"config":draft,"revision":current["revision"]}}),
    )
    .await;
    assert_eq!(result["connected"], true);
    assert!(raw.query::<String, _>("SHOW TABLES").unwrap().is_empty());
    assert_eq!(
        get(&s, "/admin/api/config").await["active"]["storage"]["backend"],
        "sqlite"
    );
    raw.query_drop("CREATE TABLE identity_providers (provider_id VARCHAR(255) PRIMARY KEY, display_name TEXT NOT NULL, base_url TEXT NOT NULL, enabled BIGINT NOT NULL)").unwrap();
    raw.query_drop("INSERT INTO identity_providers VALUES ('legacy','Legacy provider','https://legacy.example.com',1)").unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mysql.txt"), &db.url).unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(
        &path,
        json!({"storage":{"backend":"mysql","mysql_url_file":"mysql.txt"}}).to_string(),
    )
    .unwrap();
    let loaded = LoadedConfig::load_with(Some(&path), dir.path(), &Environment::new()).unwrap();
    let store = CloudStore::open(&loaded.cloud).unwrap();
    let row: Option<(String, String, Option<String>)> = raw.query_first("SELECT display_name,session_path,authenticate_path FROM identity_providers WHERE provider_id='legacy'").unwrap();
    assert_eq!(
        row,
        Some((
            "Legacy provider".into(),
            "/session/minecraft/hasJoined".into(),
            None
        ))
    );
    drop(store);
    // A second startup exercises idempotence of the actual information_schema upgrade path.
    drop(CloudStore::open(&loaded.cloud).unwrap());
    let count: u64 = raw
        .query_first("SELECT COUNT(*) FROM identity_providers WHERE provider_id='legacy'")
        .unwrap()
        .unwrap();
    assert_eq!(count, 1);
    let before_probe: Vec<String> = raw.query("SHOW TABLES").unwrap();
    let populated = check(&s, json!({"kind":"mysql","connection":connection})).await;
    assert_eq!(populated["connected"], true);
    assert_eq!(populated["target"]["sqlite_import_eligible"], false);
    let after_probe: Vec<String> = raw.query("SHOW TABLES").unwrap();
    assert_eq!(before_probe, after_probe);
}

#[test]
fn mysql_imports_sqlite_once_and_preserves_a_nonempty_target() {
    let Some(db) = Database::isolated() else {
        eprintln!("SKIP: SPM_TEST_MYSQL_URL missing; no real MySQL verification");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.db");
    let sqlite = rusqlite::Connection::open(&source).unwrap();
    sqlite.execute_batch("CREATE TABLE accounts(account_id TEXT PRIMARY KEY,created_at TEXT NOT NULL); INSERT INTO accounts VALUES('import-me','2024-01-01 00:00:00')").unwrap();
    std::fs::write(dir.path().join("mysql.txt"), &db.url).unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path,json!({"storage":{"backend":"mysql","database_path":"source.db","mysql_url_file":"mysql.txt"}}).to_string()).unwrap();
    let loaded = LoadedConfig::load_with(Some(&path), dir.path(), &Environment::new()).unwrap();
    drop(CloudStore::open(&loaded.cloud).unwrap());
    let mut raw = db.connection();
    let imported: u64 = raw
        .query_first("SELECT COUNT(*) FROM accounts WHERE account_id='import-me'")
        .unwrap()
        .unwrap();
    assert_eq!(imported, 1);
    raw.query_drop(
        "UPDATE accounts SET created_at='2025-05-05 00:00:00' WHERE account_id='import-me'",
    )
    .unwrap();
    sqlite
        .execute_batch("INSERT INTO accounts VALUES('do-not-import','2024-02-02 00:00:00')")
        .unwrap();
    drop(CloudStore::open(&loaded.cloud).unwrap());
    let unchanged: String = raw
        .query_first("SELECT created_at FROM accounts WHERE account_id='import-me'")
        .unwrap()
        .unwrap();
    let not_imported: u64 = raw
        .query_first("SELECT COUNT(*) FROM accounts WHERE account_id='do-not-import'")
        .unwrap()
        .unwrap();
    assert_eq!(unchanged, "2025-05-05 00:00:00");
    assert_eq!(not_imported, 0);
}

async fn save_cache(s: &Server, backend: &str, connection: Option<Value>) {
    let current = get(s, "/admin/api/config").await;
    let mut config = current["config"].clone();
    config["cache"]["backend"] = json!(backend);
    config["cache"]["timeout_ms"] = json!(50);
    let mut body = json!({"config":config,"revision":current["revision"]});
    if let Some(mut connection) = connection {
        connection["operation"] = json!("replace");
        body["managed_secrets"] = json!({"redis":connection});
    }
    let response = reqwest::Client::new()
        .put(format!("{}/admin/api/config", s.url))
        .bearer_auth(TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "cache save rejected: {}",
        response.status()
    );
    assert_eq!(
        get(s, "/admin/api/config").await["active"]["cache"]["backend"],
        backend
    );
}

#[tokio::test]
async fn redis_probe_writes_nothing_and_live_cache_switches_use_redis_then_fallback() {
    let Ok(url) = std::env::var("SPM_TEST_REDIS_URL") else {
        eprintln!("SKIP: SPM_TEST_REDIS_URL missing; no real Redis verification");
        return;
    };
    let parsed = reqwest::Url::parse(&url).unwrap();
    let connection = json!({"host":parsed.host_str(),"port":parsed.port().unwrap_or(6379),
        "username":parsed.username(),"password":parsed.password(),
        "database":parsed.path().trim_start_matches('/').parse::<u32>().unwrap_or(0)});
    let client = redis::Client::open(url.as_str()).unwrap();
    let mut redis = client
        .get_multiplexed_async_connection()
        .await
        .expect("dedicated Redis available");
    let before: Vec<String> = redis::cmd("KEYS")
        .arg("*")
        .query_async(&mut redis)
        .await
        .unwrap();
    let s = server().await;
    let result = check(&s, json!({"kind":"redis","connection":connection})).await;
    assert_eq!(result["connected"], true);
    std::fs::write(s._dir.path().join("candidate-redis.txt"), &url).unwrap();
    let current = get(&s, "/admin/api/config").await;
    let mut draft = current["config"].clone();
    draft["cache"] = json!({"backend":"redis","redis_url_file":"candidate-redis.txt"});
    let result = check(
        &s,
        json!({"kind":"redis","draft":{"config":draft,"revision":current["revision"]}}),
    )
    .await;
    assert_eq!(result["connected"], true);
    assert_eq!(
        get(&s, "/admin/api/config").await["active"]["cache"]["backend"],
        "memory"
    );
    let after: Vec<String> = redis::cmd("KEYS")
        .arg("*")
        .query_async(&mut redis)
        .await
        .unwrap();
    assert_eq!(
        before.len(),
        after.len(),
        "PING probe must not write business cache keys"
    );
    save_cache(&s, "redis", Some(connection)).await;
    let original = get(&s, "/v1/assets?scope=accessible&limit=2").await;
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg("spm:catalog:v1:*")
        .query_async(&mut redis)
        .await
        .unwrap();
    let key = keys
        .iter()
        .find(|key| !before.contains(key) && !key.ends_with(":index"))
        .expect("hot-save must write to actual Redis");
    let mut marked = original.clone();
    marked["external_service_probe"] = json!(true);
    let _: String = redis::cmd("SET")
        .arg(key)
        .arg(marked.to_string())
        .arg("EX")
        .arg(20)
        .query_async(&mut redis)
        .await
        .unwrap();
    assert_eq!(
        get(&s, "/v1/assets?scope=accessible&limit=2").await["external_service_probe"],
        true,
        "must read remote Redis, not only local memory"
    );
    let _: String = redis::cmd("CLIENT")
        .arg("PAUSE")
        .arg(1000)
        .arg("ALL")
        .query_async(&mut redis)
        .await
        .unwrap();
    assert_eq!(
        get(&s, "/v1/assets?scope=accessible&limit=2").await,
        original,
        "real unresponsive Redis must fall back to the cached local page"
    );
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    save_cache(&s, "memory", None).await;
    assert_eq!(
        get(&s, "/v1/assets?scope=accessible&limit=2").await,
        original
    );
    save_cache(&s, "none", None).await;
    assert_eq!(
        get(&s, "/v1/assets?scope=accessible&limit=2").await,
        original
    );
    let new_keys: Vec<&String> = keys.iter().filter(|key| !before.contains(key)).collect();
    if !new_keys.is_empty() {
        let _: usize = redis::cmd("DEL")
            .arg(new_keys)
            .query_async(&mut redis)
            .await
            .unwrap();
    }
}
