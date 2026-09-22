use std::{path::{Path, PathBuf}, sync::{Arc, Mutex}};

use rusqlite::{params, Connection, OptionalExtension};
use sha2::Digest;

use crate::{config::{validate_slug, CloudConfig}, error::CloudError, identity::GameIdentity, models::{AccountSummary, AclEntry, AclUpdate, AppearanceState, AssetSummary, CreateIdentity, CreateScope, CreateTarget, IdentitySummary, OfflineBindingRequest, ScopeSummary, TargetKind, TargetSummary}};

#[derive(Clone)]
pub struct CloudStore {
    connection: Arc<Mutex<Connection>>,
    object_dir: Arc<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct AssetContent {
    pub path: PathBuf,
    pub length: u64,
    pub raw_sha256: String,
    pub name: String,
    pub format: String,
}

impl CloudStore {
    pub fn open(config: &CloudConfig) -> Result<Self, CloudError> {
        if let Some(parent) = config.database_path.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::create_dir_all(&config.object_dir)?;
        let connection = Connection::open(&config.database_path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Self::migrate(&connection)?;
        let store = Self { connection: Arc::new(Mutex::new(connection)), object_dir: Arc::new(config.object_dir.clone()) };
        store.seed_official_provider()?;
        Ok(store)
    }

    fn migrate(connection: &Connection) -> Result<(), CloudError> {
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS accounts (
                 account_id TEXT PRIMARY KEY,
                 created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS identity_providers (
                 provider_id TEXT PRIMARY KEY,
                 display_name TEXT NOT NULL,
                 base_url TEXT NOT NULL,
                 enabled INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS identities (
                 identity_id TEXT PRIMARY KEY,
                 account_id TEXT NOT NULL REFERENCES accounts(account_id),
                 identity_kind TEXT NOT NULL,
                 provider_id TEXT,
                 scope_id TEXT,
                 profile_uuid TEXT NOT NULL,
                 display_name TEXT NOT NULL,
                 verified INTEGER NOT NULL DEFAULT 0,
                 UNIQUE(identity_kind, provider_id, scope_id, profile_uuid)
             );
             CREATE TABLE IF NOT EXISTS scopes (
                 scope_id TEXT PRIMARY KEY,
                 tenant_id TEXT NOT NULL,
                 name TEXT NOT NULL,
                 world_epoch TEXT NOT NULL,
                 created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS targets (
                 target_id TEXT PRIMARY KEY,
                 scope_id TEXT NOT NULL REFERENCES scopes(scope_id),
                 target_kind TEXT NOT NULL,
                 display_name TEXT NOT NULL,
                 owner_account_id TEXT NOT NULL REFERENCES accounts(account_id),
                 revision INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS target_acl (
                 target_id TEXT NOT NULL REFERENCES targets(target_id),
                 account_id TEXT NOT NULL REFERENCES accounts(account_id),
                 role TEXT NOT NULL,
                 PRIMARY KEY(target_id, account_id)
             );
             CREATE TABLE IF NOT EXISTS appearances (
                 target_id TEXT PRIMARY KEY REFERENCES targets(target_id),
                 revision INTEGER NOT NULL DEFAULT 0,
                 asset_id TEXT,
                 asset_revision INTEGER,
                 raw_sha256 TEXT,
                 texture_id TEXT,
                 scale REAL,
                 disabled INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS assets (
                 asset_id TEXT PRIMARY KEY,
                 owner_account_id TEXT NOT NULL REFERENCES accounts(account_id),
                 current_revision INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS asset_revisions (
                 asset_id TEXT NOT NULL REFERENCES assets(asset_id),
                 revision INTEGER NOT NULL,
                 name TEXT NOT NULL,
                 format TEXT NOT NULL,
                 raw_sha256 TEXT NOT NULL,
                 byte_length INTEGER NOT NULL,
                 object_path TEXT NOT NULL,
                 created_at TEXT NOT NULL,
                 PRIMARY KEY(asset_id, revision),
                 UNIQUE(raw_sha256)
             );
             CREATE TABLE IF NOT EXISTS idempotency (
                 account_id TEXT NOT NULL,
                 request_id TEXT NOT NULL,
                 request_hash TEXT NOT NULL,
                 response_json TEXT NOT NULL,
                 PRIMARY KEY(account_id, request_id)
             );
             CREATE INDEX IF NOT EXISTS idx_targets_scope ON targets(scope_id);
             CREATE INDEX IF NOT EXISTS idx_asset_revisions_sha ON asset_revisions(raw_sha256);
             COMMIT;"
        )?;
        Ok(())
    }

    fn seed_official_provider(&self) -> Result<(), CloudError> {
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        conn.execute("INSERT OR IGNORE INTO identity_providers(provider_id, display_name, base_url, enabled) VALUES ('official', 'Minecraft official', 'https://sessionserver.mojang.com', 1)", [])?;
        conn.execute("INSERT OR IGNORE INTO accounts(account_id, created_at) VALUES (?1, datetime('now'))", ["account_local"])?;
        Ok(())
    }

    pub fn list_providers(&self) -> Result<Vec<serde_json::Value>, CloudError> {
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let mut stmt = conn.prepare("SELECT provider_id, display_name, base_url, enabled FROM identity_providers ORDER BY provider_id")?;
        let rows = stmt.query_map([], |row| Ok(serde_json::json!({"provider_id": row.get::<_, String>(0)?, "display_name": row.get::<_, String>(1)?, "base_url": row.get::<_, String>(2)?, "enabled": row.get::<_, i64>(3)? != 0})))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn create_account(&self, account_id: &str) -> Result<AccountSummary, CloudError> {
        validate_slug(account_id, "account_id")?;
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        conn.execute("INSERT INTO accounts(account_id, created_at) VALUES (?1, datetime('now')) ON CONFLICT(account_id) DO NOTHING", [account_id])?;
        Ok(AccountSummary { account_id: account_id.to_owned() })
    }

    pub fn create_identity(&self, account_id: &str, input: &CreateIdentity) -> Result<IdentitySummary, CloudError> {
        let identity: GameIdentity = input.identity.parse()?;
        if input.display_name.trim().is_empty() || input.display_name.len() > 16 * 1024 { return Err(CloudError::invalid_metadata("invalid identity display name")); }
        let identity_id = format!("identity_{}", uuid::Uuid::new_v4().simple());
        let (kind, provider_id, scope_id, profile_uuid) = match &identity {
            GameIdentity::Official { profile_uuid } => ("official", None, None, profile_uuid.to_string()),
            GameIdentity::Yggdrasil { provider_id, profile_uuid } => ("yggdrasil", Some(provider_id.as_str()), None, profile_uuid.to_string()),
            GameIdentity::Offline { scope_id, profile_uuid } => ("offline", None, Some(scope_id.as_str()), profile_uuid.to_string()),
        };
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if let Some(provider_id) = provider_id {
            let trusted: Option<i64> = conn.query_row("SELECT enabled FROM identity_providers WHERE provider_id = ?1", [provider_id], |row| row.get(0)).optional()?;
            if trusted != Some(1) { return Err(CloudError::AccessDenied); }
        }
        conn.execute("INSERT INTO accounts(account_id, created_at) VALUES (?1, datetime('now')) ON CONFLICT(account_id) DO NOTHING", [account_id])?;
        conn.execute("INSERT INTO identities(identity_id, account_id, identity_kind, provider_id, scope_id, profile_uuid, display_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![identity_id, account_id, kind, provider_id, scope_id, profile_uuid, input.display_name])?;
        Ok(IdentitySummary { identity_id, account_id: account_id.to_owned(), identity: identity.wire_string(), display_name: input.display_name.clone(), verification_status: "PENDING_VERIFICATION".to_owned() })
    }

    pub fn list_identities(&self, account_id: &str) -> Result<Vec<IdentitySummary>, CloudError> {
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let mut stmt = conn.prepare("SELECT identity_id, account_id, identity_kind, provider_id, scope_id, profile_uuid, display_name, verified FROM identities WHERE account_id = ?1 ORDER BY identity_id")?;
        let rows = stmt.query_map([account_id], |row| {
            let kind: String = row.get(2)?;
            let provider: Option<String> = row.get(3)?;
            let scope: Option<String> = row.get(4)?;
            let uuid: String = row.get(5)?;
            let identity = match kind.as_str() {
                "official" => format!("official:{uuid}"),
                "yggdrasil" => format!("yggdrasil:{}:{uuid}", provider.ok_or_else(|| rusqlite::Error::InvalidQuery)?),
                "offline" => format!("offline:{}:{uuid}", scope.ok_or_else(|| rusqlite::Error::InvalidQuery)?),
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            let verified: i64 = row.get(7)?;
            Ok(IdentitySummary { identity_id: row.get(0)?, account_id: row.get(1)?, identity, display_name: row.get(6)?, verification_status: if verified == 1 { "VERIFIED".to_owned() } else { "PENDING_VERIFICATION".to_owned() } })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn create_offline_binding(&self, account_id: &str, input: &OfflineBindingRequest) -> Result<serde_json::Value, CloudError> {
        validate_slug(&input.scope_id, "scope_id")?;
        validate_slug(&input.world_epoch, "world_epoch")?;
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let identity: Option<(String, Option<String>, i64)> = conn.query_row("SELECT identity_kind, scope_id, verified FROM identities WHERE identity_id = ?1 AND account_id = ?2", params![input.identity_id, account_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
        let Some((kind, identity_scope, verified)) = identity else { return Err(CloudError::NotFound); };
        if kind != "offline" || identity_scope.as_deref() != Some(input.scope_id.as_str()) { return Err(CloudError::AccessDenied); }
        if verified != 1 { return Err(CloudError::IdentityNotVerified); }
        let scope_exists: Option<String> = conn.query_row("SELECT scope_id FROM scopes WHERE scope_id = ?1 AND tenant_id = ?2 AND world_epoch = ?3", params![input.scope_id, account_id, input.world_epoch], |row| row.get(0)).optional()?;
        if scope_exists.is_none() { return Err(CloudError::NotFound); }
        Ok(serde_json::json!({"identity_id": input.identity_id, "scope_id": input.scope_id, "world_epoch": input.world_epoch, "status": "PENDING_APPROVAL"}))
    }

    pub fn create_scope(&self, account_id: &str, input: &CreateScope) -> Result<ScopeSummary, CloudError> {
        validate_slug(&input.scope_id, "scope_id")?;
        if input.name.trim().is_empty() || input.name.len() > 16 * 1024 { return Err(CloudError::invalid_metadata("invalid scope name")); }
        validate_slug(&input.world_epoch, "world_epoch")?;
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        conn.execute("INSERT INTO accounts(account_id, created_at) VALUES (?1, datetime('now')) ON CONFLICT(account_id) DO NOTHING", [account_id])?;
        conn.execute("INSERT INTO scopes(scope_id, tenant_id, name, world_epoch, created_at) VALUES (?1, ?2, ?3, ?4, datetime('now'))", params![input.scope_id, account_id, input.name, input.world_epoch])?;
        Ok(ScopeSummary { scope_id: input.scope_id.clone(), tenant_id: account_id.to_owned(), name: input.name.clone(), world_epoch: input.world_epoch.clone() })
    }

    pub fn list_scopes(&self, account_id: &str) -> Result<Vec<ScopeSummary>, CloudError> {
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let mut stmt = conn.prepare("SELECT scope_id, tenant_id, name, world_epoch FROM scopes WHERE tenant_id = ?1 ORDER BY scope_id")?;
        let rows = stmt.query_map([account_id], |row| Ok(ScopeSummary { scope_id: row.get(0)?, tenant_id: row.get(1)?, name: row.get(2)?, world_epoch: row.get(3)? }))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn create_target(&self, account_id: &str, input: &CreateTarget) -> Result<serde_json::Value, CloudError> {
        let id = input.id();
        validate_slug(&id, "target_id")?;
        if input.display_name.trim().is_empty() || input.display_name.len() > 16 * 1024 { return Err(CloudError::invalid_metadata("invalid target display name")); }
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let scope_exists: Option<String> = conn.query_row("SELECT scope_id FROM scopes WHERE scope_id = ?1 AND tenant_id = ?2", params![input.scope_id, account_id], |row| row.get(0)).optional()?;
        if scope_exists.is_none() { return Err(CloudError::NotFound); }
        let kind = serde_json::to_string(&input.kind).unwrap().trim_matches('"').to_owned();
        conn.execute("INSERT INTO targets(target_id, scope_id, target_kind, display_name, owner_account_id) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, input.scope_id, kind, input.display_name, account_id])?;
        conn.execute("INSERT INTO target_acl(target_id, account_id, role) VALUES (?1, ?2, 'manage')", params![id, account_id])?;
        conn.execute("INSERT INTO appearances(target_id) VALUES (?1)", [&id])?;
        Ok(serde_json::json!({"target_id": id, "scope_id": input.scope_id, "kind": input.kind.clone(), "display_name": input.display_name, "revision": 0}))
    }

    pub fn list_targets(&self, account_id: &str, scope_id: &str) -> Result<Vec<TargetSummary>, CloudError> {
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let mut stmt = conn.prepare("SELECT t.target_id, t.scope_id, t.target_kind, t.display_name, t.revision FROM targets t JOIN target_acl acl ON acl.target_id = t.target_id WHERE t.scope_id = ?1 AND acl.account_id = ?2 ORDER BY t.target_id")?;
        let rows = stmt.query_map(params![scope_id, account_id], |row| {
            let kind = match row.get::<_, String>(2)?.as_str() {
                "PLAYER" => TargetKind::Player,
                "DUMMY" => TargetKind::Dummy,
                "MAID" => TargetKind::Maid,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(TargetSummary { target_id: row.get(0)?, scope_id: row.get(1)?, kind, display_name: row.get(3)?, revision: row.get(4)? })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn list_acl(&self, account_id: &str, target_id: &str) -> Result<Vec<AclEntry>, CloudError> {
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let owner: Option<String> = conn.query_row("SELECT owner_account_id FROM targets WHERE target_id = ?1", [target_id], |row| row.get(0)).optional()?;
        if owner.as_deref() != Some(account_id) { return Err(CloudError::AccessDenied); }
        let mut stmt = conn.prepare("SELECT account_id, role FROM target_acl WHERE target_id = ?1 ORDER BY account_id")?;
        let rows = stmt.query_map([target_id], |row| Ok(AclEntry { account_id: row.get(0)?, role: row.get(1)? }))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn set_acl(&self, account_id: &str, target_id: &str, update: &AclUpdate) -> Result<AclEntry, CloudError> {
        validate_slug(&update.account_id, "account_id")?;
        if !matches!(update.role.as_str(), "manage" | "edit" | "viewer") { return Err(CloudError::invalid_metadata("invalid target ACL role")); }
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let owner: Option<String> = conn.query_row("SELECT owner_account_id FROM targets WHERE target_id = ?1", [target_id], |row| row.get(0)).optional()?;
        if owner.as_deref() != Some(account_id) { return Err(CloudError::AccessDenied); }
        conn.execute("INSERT INTO accounts(account_id, created_at) VALUES (?1, datetime('now')) ON CONFLICT(account_id) DO NOTHING", [&update.account_id])?;
        conn.execute("INSERT INTO target_acl(target_id, account_id, role) VALUES (?1, ?2, ?3) ON CONFLICT(target_id, account_id) DO UPDATE SET role = excluded.role", params![target_id, update.account_id, update.role])?;
        Ok(AclEntry { account_id: update.account_id.clone(), role: update.role.clone() })
    }

    pub fn get_appearance(&self, account_id: &str, target_id: &str) -> Result<AppearanceState, CloudError> {
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        conn.query_row("SELECT a.target_id, a.revision, a.asset_id, a.asset_revision, a.raw_sha256, a.texture_id, a.scale, a.disabled FROM appearances a JOIN targets t ON t.target_id = a.target_id WHERE a.target_id = ?1 AND t.owner_account_id = ?2", params![target_id, account_id], |row| Ok(AppearanceState { target_id: row.get(0)?, revision: row.get(1)?, asset_id: row.get(2)?, asset_revision: row.get(3)?, raw_sha256: row.get(4)?, texture_id: row.get(5)?, scale: row.get(6)?, disabled: row.get::<_, i64>(7)? != 0 })).optional()?.ok_or(CloudError::NotFound)
    }

    pub fn update_appearance(&self, account_id: &str, target_id: &str, update: &crate::models::AppearanceUpdate) -> Result<AppearanceState, CloudError> {
        if update.request_id.is_empty() || update.request_id.len() > 128 { return Err(CloudError::invalid_metadata("invalid request_id")); }
        if let Some(scale) = update.scale { if !scale.is_finite() || !(0.01..=100.0).contains(&scale) { return Err(CloudError::invalid_metadata("invalid scale")); } }
        let mut conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        let request_hash = hex::encode(sha2::Sha256::digest(serde_json::to_vec(&(target_id, update)).map_err(|_| CloudError::invalid_metadata("invalid appearance payload"))?));
        if let Some((existing_hash, response_json)) = tx.query_row("SELECT request_hash, response_json FROM idempotency WHERE account_id = ?1 AND request_id = ?2", params![account_id, update.request_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).optional()? {
            if existing_hash != request_hash { return Err(CloudError::IdempotencyConflict); }
            return serde_json::from_str(&response_json).map_err(|_| CloudError::Internal(anyhow::anyhow!("stored idempotency response is invalid")));
        }
        let current: AppearanceState = tx.query_row("SELECT a.target_id, a.revision, a.asset_id, a.asset_revision, a.raw_sha256, a.texture_id, a.scale, a.disabled FROM appearances a JOIN targets t ON t.target_id = a.target_id WHERE a.target_id = ?1 AND t.owner_account_id = ?2", params![target_id, account_id], |row| Ok(AppearanceState { target_id: row.get(0)?, revision: row.get(1)?, asset_id: row.get(2)?, asset_revision: row.get(3)?, raw_sha256: row.get(4)?, texture_id: row.get(5)?, scale: row.get(6)?, disabled: row.get::<_, i64>(7)? != 0 })).optional()?.ok_or(CloudError::NotFound)?;
        if current.revision != update.expected_revision { return Err(CloudError::RevisionConflict); }
        let next_revision = current.revision + 1;
        tx.execute("UPDATE appearances SET revision = ?1, asset_id = ?2, asset_revision = ?3, raw_sha256 = ?4, texture_id = ?5, scale = ?6, disabled = ?7 WHERE target_id = ?8", params![next_revision, update.asset_id, update.asset_revision, update.raw_sha256, update.texture_id, update.scale, i64::from(update.disabled), target_id])?;
        let state = AppearanceState { target_id: target_id.to_owned(), revision: next_revision, asset_id: update.asset_id.clone(), asset_revision: update.asset_revision, raw_sha256: update.raw_sha256.clone(), texture_id: update.texture_id.clone(), scale: update.scale, disabled: update.disabled };
        let response_json = serde_json::to_string(&state).map_err(|_| CloudError::Internal(anyhow::anyhow!("failed to encode idempotency response")))?;
        tx.execute("INSERT INTO idempotency(account_id, request_id, request_hash, response_json) VALUES (?1, ?2, ?3, ?4)", params![account_id, update.request_id, request_hash, response_json])?;
        tx.commit()?;
        Ok(state)
    }

    pub fn list_assets(&self, account_id: &str) -> Result<Vec<AssetSummary>, CloudError> {
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let mut stmt = conn.prepare("SELECT r.asset_id, r.revision, r.name, r.format, r.raw_sha256, r.byte_length FROM asset_revisions r JOIN assets a ON a.asset_id = r.asset_id WHERE a.owner_account_id = ?1 ORDER BY r.asset_id, r.revision")?;
        let rows = stmt.query_map([account_id], |row| Ok(AssetSummary { asset_id: row.get(0)?, revision: row.get(1)?, name: row.get(2)?, format: row.get(3)?, raw_sha256: row.get(4)?, byte_length: row.get(5)? }))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn register_asset(&self, account_id: &str, asset_id: &str, name: &str, format: &str, sha256: &str, byte_length: u64, object_path: &Path) -> Result<AssetSummary, CloudError> {
        validate_slug(asset_id, "asset_id")?;
        if name.is_empty() || name.len() > 16 * 1024 || format.is_empty() || format.len() > 64 { return Err(CloudError::invalid_metadata("invalid asset metadata")); }
        let mut conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        tx.execute("INSERT INTO assets(asset_id, owner_account_id, current_revision) VALUES (?1, ?2, 1) ON CONFLICT(asset_id) DO UPDATE SET current_revision = current_revision + 1", params![asset_id, account_id])?;
        let revision: u64 = tx.query_row("SELECT current_revision FROM assets WHERE asset_id = ?1", [asset_id], |row| row.get(0))?;
        tx.execute("INSERT INTO asset_revisions(asset_id, revision, name, format, raw_sha256, byte_length, object_path, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))", params![asset_id, revision, name, format, sha256, byte_length as i64, object_path.to_string_lossy().to_string()])?;
        tx.commit()?;
        Ok(AssetSummary { asset_id: asset_id.to_owned(), revision, name: name.to_owned(), format: format.to_owned(), raw_sha256: sha256.to_owned(), byte_length })
    }

    pub fn asset_content(&self, asset_id: &str, revision: u64) -> Result<AssetContent, CloudError> {
        let conn = self.connection.lock().map_err(|_| CloudError::configuration("database lock poisoned"))?;
        conn.query_row("SELECT object_path, byte_length, raw_sha256, name, format FROM asset_revisions WHERE asset_id = ?1 AND revision = ?2", params![asset_id, revision], |row| Ok(AssetContent { path: PathBuf::from(row.get::<_, String>(0)?), length: row.get::<_, i64>(1)? as u64, raw_sha256: row.get(2)?, name: row.get(3)?, format: row.get(4)? })).optional()?.ok_or(CloudError::NotFound)
    }

    pub fn object_path_for_sha(&self, sha256: &str) -> PathBuf { self.object_dir.join(&sha256[0..2.min(sha256.len())]).join(sha256) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn sqlite_wal_and_cas_revision_are_atomic() {
        let dir = tempdir().unwrap();
        let config = CloudConfig { instance_id: "test".into(), origin: "https://localhost".into(), bind_addr: "127.0.0.1:0".parse().unwrap(), database_path: dir.path().join("test.db"), object_dir: dir.path().join("objects"), access_token: Some("secret".into()), bootstrap_account_id: "account_local".into(), max_asset_bytes: 128 * 1024 * 1024, max_message_bytes: 64 * 1024 };
        let store = CloudStore::open(&config).unwrap();
        let scope = store.create_scope("account_local", &CreateScope { scope_id: "scope".into(), name: "Scope".into(), world_epoch: "epoch-1".into() }).unwrap();
        assert_eq!(scope.scope_id, "scope");
        let target = CreateTarget { scope_id: "scope".into(), target_id: Some("target".into()), kind: crate::models::TargetKind::Player, display_name: "Player".into() };
        store.create_target("account_local", &target).unwrap();
        let first = store.get_appearance("account_local", "target").unwrap();
        assert_eq!(first.revision, 0);
        let update = crate::models::AppearanceUpdate { request_id: "req".into(), expected_revision: 0, asset_id: None, asset_revision: None, raw_sha256: None, texture_id: Some("texture".into()), scale: Some(1.0), disabled: false };
        let second = store.update_appearance("account_local", "target", &update).unwrap();
        assert_eq!(second.revision, 1);
        assert_eq!(store.update_appearance("account_local", "target", &update).unwrap().revision, 1);
        let mut conflicting_request = update.clone();
        conflicting_request.texture_id = Some("different".into());
        assert!(matches!(store.update_appearance("account_local", "target", &conflicting_request), Err(CloudError::IdempotencyConflict)));
    }
}
