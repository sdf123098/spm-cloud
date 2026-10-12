//! Optional, disposable catalog-page cache. Authentication and permissions remain in the store.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CacheSettings {
    pub backend: String,
    pub redis_url_file: Option<PathBuf>,
    pub ttl_seconds: u64,
    pub max_pages: usize,
    pub timeout_ms: u64,
    #[serde(skip)]
    pub(crate) redis_url: Option<String>,
}
impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            backend: "memory".into(),
            redis_url_file: None,
            ttl_seconds: 30,
            max_pages: 256,
            timeout_ms: 300,
            redis_url: None,
        }
    }
}
impl std::fmt::Debug for CacheSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CacheSettings")
            .field("backend", &self.backend)
            .field("ttl_seconds", &self.ttl_seconds)
            .field("max_pages", &self.max_pages)
            .finish_non_exhaustive()
    }
}
impl CacheSettings {
    pub fn validate(&self) -> Result<(), crate::error::CloudError> {
        if !["memory", "redis", "none"].contains(&self.backend.as_str())
            || !(1..=3600).contains(&self.ttl_seconds)
            || !(1..=4096).contains(&self.max_pages)
            || !(50..=5000).contains(&self.timeout_ms)
        {
            return Err(crate::error::CloudError::configuration(
                "cache requires backend memory/redis/none, TTL 1..3600, pages 1..4096 and timeout 50..5000 ms",
            ));
        }
        if self.backend == "redis" && self.redis_url.is_none() {
            return Err(crate::error::CloudError::configuration(
                "cache.redis_url_file or SPM_CLOUD_REDIS_URL is required for Redis",
            ));
        }
        if let Some(url) = &self.redis_url {
            redis::Client::open(url.as_str()).map_err(|_| {
                crate::error::CloudError::configuration("invalid Redis connection URL")
            })?;
        }
        Ok(())
    }
}
struct MemoryEntry {
    key: String,
    value: Value,
    expires: Instant,
}
struct RedisState {
    connection: Option<redis::aio::MultiplexedConnection>,
    retry_at: Instant,
}
pub struct CatalogCache {
    settings: CacheSettings,
    namespace: String,
    memory: Mutex<VecDeque<MemoryEntry>>,
    redis: tokio::sync::Mutex<RedisState>,
}
impl CatalogCache {
    pub fn new(settings: CacheSettings, instance: &str) -> Self {
        Self {
            settings,
            namespace: format!("spm:catalog:v1:{}", hex::encode(Sha256::digest(instance))),
            memory: Mutex::new(VecDeque::new()),
            redis: tokio::sync::Mutex::new(RedisState {
                connection: None,
                retry_at: Instant::now(),
            }),
        }
    }
    pub fn key(&self, parts: &Value) -> String {
        format!(
            "{}:{}",
            self.namespace,
            hex::encode(Sha256::digest(parts.to_string()))
        )
    }
    fn memory_get(&self, key: &str) -> Option<Value> {
        let mut pages = self.memory.lock().unwrap();
        pages.retain(|entry| entry.expires > Instant::now());
        let index = pages.iter().position(|entry| entry.key == key)?;
        let entry = pages.remove(index)?;
        let value = entry.value.clone();
        pages.push_back(entry);
        Some(value)
    }
    async fn connection(&self) -> Option<redis::aio::MultiplexedConnection> {
        let mut state = self.redis.lock().await;
        if let Some(connection) = &state.connection {
            return Some(connection.clone());
        }
        if Instant::now() < state.retry_at {
            return None;
        }
        let client = redis::Client::open(self.settings.redis_url.as_deref()?).ok()?;
        let timeout = Duration::from_millis(self.settings.timeout_ms);
        let config = redis::AsyncConnectionConfig::new()
            .set_connection_timeout(timeout)
            .set_response_timeout(timeout);
        match tokio::time::timeout(
            timeout,
            client.get_multiplexed_async_connection_with_config(&config),
        )
        .await
        {
            Ok(Ok(connection)) => {
                state.connection = Some(connection.clone());
                Some(connection)
            }
            _ => {
                state.retry_at = Instant::now() + Duration::from_secs(5);
                None
            }
        }
    }
    async fn disconnected(&self) {
        let mut state = self.redis.lock().await;
        state.connection = None;
        state.retry_at = Instant::now() + Duration::from_secs(5);
    }
    pub async fn get(&self, key: &str) -> Option<Value> {
        if self.settings.backend == "none" {
            return None;
        }
        if self.settings.backend == "redis" {
            if let Some(mut connection) = self.connection().await {
                match tokio::time::timeout(
                    Duration::from_millis(self.settings.timeout_ms),
                    redis::cmd("GET")
                        .arg(key)
                        .query_async::<Option<String>>(&mut connection),
                )
                .await
                {
                    Ok(Ok(Some(value))) => {
                        if value.len() <= 262_144 {
                            if let Ok(value) = serde_json::from_str(&value) {
                                return Some(value);
                            }
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ => self.disconnected().await,
                }
            }
        }
        self.memory_get(key)
    }
    pub async fn put(&self, key: String, value: Value) {
        if self.settings.backend == "none" {
            return;
        }
        let body = value.to_string();
        if body.len() > 262_144 {
            return;
        }
        {
            let mut pages = self.memory.lock().unwrap();
            pages.retain(|entry| entry.key != key && entry.expires > Instant::now());
            pages.push_back(MemoryEntry {
                key: key.clone(),
                value,
                expires: Instant::now() + Duration::from_secs(self.settings.ttl_seconds),
            });
            while pages.len() > self.settings.max_pages {
                pages.pop_front();
            }
        }
        if self.settings.backend == "redis" {
            if let Some(mut connection) = self.connection().await {
                // Bound Redis pages as well as their TTL; the index itself expires when idle.
                let script = redis::Script::new(
                    "redis.call('SET',KEYS[1],ARGV[1],'EX',ARGV[2]); redis.call('ZADD',KEYS[2],ARGV[3],KEYS[1]); local extra=redis.call('ZCARD',KEYS[2])-tonumber(ARGV[4]); if extra>0 then local old=redis.call('ZRANGE',KEYS[2],0,extra-1); for _,k in ipairs(old) do redis.call('DEL',k) end; redis.call('ZREMRANGEBYRANK',KEYS[2],0,extra-1) end; redis.call('EXPIRE',KEYS[2],tonumber(ARGV[2])*2); return 1",
                );
                let mut call = script.prepare_invoke();
                call.key(&key)
                    .key(format!("{}:index", self.namespace))
                    .arg(body)
                    .arg(self.settings.ttl_seconds)
                    .arg(chrono::Utc::now().timestamp_millis())
                    .arg(self.settings.max_pages);
                if !matches!(
                    tokio::time::timeout(
                        Duration::from_millis(self.settings.timeout_ms),
                        call.invoke_async::<i64>(&mut connection)
                    )
                    .await,
                    Ok(Ok(_))
                ) {
                    self.disconnected().await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn bounds_pages_and_keeps_account_keys_separate() {
        let cache = CatalogCache::new(
            CacheSettings {
                max_pages: 2,
                ..Default::default()
            },
            "test",
        );
        assert_ne!(
            cache.key(&serde_json::json!(["a", "mine"])),
            cache.key(&serde_json::json!(["b", "mine"]))
        );
        for key in ["a", "b", "c"] {
            cache
                .put(key.into(), serde_json::json!({"entries":[]}))
                .await;
        }
        assert!(cache.get("a").await.is_none());
        assert!(cache.get("b").await.is_some());
        let disabled = CatalogCache::new(
            CacheSettings {
                backend: "none".into(),
                ..Default::default()
            },
            "test",
        );
        disabled.put("a".into(), serde_json::json!([])).await;
        assert!(disabled.get("a").await.is_none());
    }
    #[tokio::test]
    async fn redis_outage_falls_back_without_losing_durable_data() {
        let cache = CatalogCache::new(
            CacheSettings {
                backend: "redis".into(),
                redis_url: Some("redis://127.0.0.1:1/".into()),
                timeout_ms: 50,
                ..Default::default()
            },
            "test",
        );
        cache
            .put("a".into(), serde_json::json!({"entries":[]}))
            .await;
        assert!(cache.get("a").await.is_some());
    }
}
