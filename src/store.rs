use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use argon2::{
    Argon2, PasswordHash, PasswordVerifier,
    password_hash::{PasswordHasher, SaltString},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    config::{CloudConfig, validate_slug},
    error::CloudError,
    identity::GameIdentity,
    models::{
        AccountSummary, AclEntry, AclUpdate, AppearanceState, AssetAclEntry, AssetAclUpdate,
        AssetSummary, AuditEntry, ClaimCodeRequest, ClaimCodeResponse, CreateIdentity,
        CreateIdentityChallenge, CreateScope, CreateTarget, EntityBindingSummary,
        IdentityChallengeResponse, IdentityProviderUpdate, IdentitySummary, LoginRequest,
        ObserveEntityBinding, OfflineBindingApproval, OfflineBindingRequest, RedeemClaimCode,
        RegisterEntityBinding, RevokeClaimCode, ScopeAclEntry, ScopeAclUpdate, ScopeSummary,
        ScopedIdentityBindingSummary, SessionResponse, TargetKind, TargetSummary,
    },
};

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

#[derive(Clone, Debug)]
pub struct ProviderChallenge {
    pub provider_id: String,
    pub username: String,
    pub expected_profile_uuid: Option<String>,
    pub server_id: String,
    pub expires_at: i64,
}

type ClaimCodeRecord = (String, String, String, String, i64, i64, i64, i64);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppearanceMutation {
    pub appearance: AppearanceState,
    pub event_id: String,
    pub scope_id: String,
}

impl CloudStore {
    pub fn open(config: &CloudConfig) -> Result<Self, CloudError> {
        if let Some(parent) = config.database_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir_all(&config.object_dir)?;
        let connection = Connection::open(&config.database_path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Self::migrate(&connection)?;
        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
            object_dir: Arc::new(config.object_dir.clone()),
        };
        store.seed_bootstrap(
            &config.bootstrap_account_id,
            config.bootstrap_password_hash.as_deref(),
        )?;
        Ok(store)
    }

    fn migrate(connection: &Connection) -> Result<(), CloudError> {
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS accounts (
                 account_id TEXT PRIMARY KEY,
                 created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS account_credentials (
                 account_id TEXT PRIMARY KEY REFERENCES accounts(account_id),
                 password_hash TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions (
                 session_id TEXT PRIMARY KEY,
                 account_id TEXT NOT NULL REFERENCES accounts(account_id),
                 access_hash TEXT NOT NULL UNIQUE,
                 refresh_hash TEXT NOT NULL UNIQUE,
                 access_expires_at INTEGER NOT NULL,
                 refresh_expires_at INTEGER NOT NULL,
                 revoked INTEGER NOT NULL DEFAULT 0
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
             CREATE TABLE IF NOT EXISTS identity_challenges (
                 challenge_hash TEXT PRIMARY KEY,
                 account_id TEXT NOT NULL REFERENCES accounts(account_id),
                 provider_id TEXT NOT NULL REFERENCES identity_providers(provider_id),
                 username TEXT NOT NULL,
                 expected_profile_uuid TEXT,
                 server_id TEXT NOT NULL,
                 expires_at INTEGER NOT NULL,
                 consumed INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS scopes (
                 scope_id TEXT PRIMARY KEY,
                 tenant_id TEXT NOT NULL,
                 name TEXT NOT NULL,
                 world_epoch TEXT NOT NULL,
                 offline_policy TEXT NOT NULL DEFAULT 'STRICT_APPROVAL',
                 created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS scope_acl (
                 scope_id TEXT NOT NULL REFERENCES scopes(scope_id),
                 account_id TEXT NOT NULL REFERENCES accounts(account_id),
                 role TEXT NOT NULL,
                 PRIMARY KEY(scope_id, account_id)
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
             CREATE TABLE IF NOT EXISTS scoped_identity_bindings (
                 binding_id TEXT PRIMARY KEY,
                 account_id TEXT NOT NULL REFERENCES accounts(account_id),
                 identity_id TEXT NOT NULL REFERENCES identities(identity_id),
                 target_id TEXT NOT NULL REFERENCES targets(target_id),
                 scope_id TEXT NOT NULL REFERENCES scopes(scope_id),
                 world_epoch TEXT NOT NULL,
                 entity_uuid TEXT NOT NULL,
                 verification_method TEXT NOT NULL,
                 status TEXT NOT NULL,
                 approved_by TEXT,
                 revision INTEGER NOT NULL DEFAULT 0,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS claim_codes (
                 code_hash TEXT PRIMARY KEY,
                 scope_id TEXT NOT NULL REFERENCES scopes(scope_id),
                 world_epoch TEXT NOT NULL,
                 target_id TEXT NOT NULL REFERENCES targets(target_id),
                 entity_uuid TEXT NOT NULL,
                 issued_by TEXT NOT NULL REFERENCES accounts(account_id),
                 expires_at INTEGER NOT NULL,
                 attempts INTEGER NOT NULL DEFAULT 0,
                 max_attempts INTEGER NOT NULL DEFAULT 5,
                 consumed INTEGER NOT NULL DEFAULT 0,
                 revoked INTEGER NOT NULL DEFAULT 0,
                 created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS entity_bindings (
                 binding_id TEXT PRIMARY KEY,
                 scope_id TEXT NOT NULL REFERENCES scopes(scope_id),
                 world_epoch TEXT NOT NULL,
                 entity_uuid TEXT NOT NULL,
                 entity_kind TEXT NOT NULL,
                 target_id TEXT NOT NULL REFERENCES targets(target_id),
                 observation_state TEXT NOT NULL DEFAULT 'REGISTERED',
                 last_seen_at TEXT,
                 revision INTEGER NOT NULL DEFAULT 0,
                 UNIQUE(scope_id, world_epoch, entity_uuid)
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
             CREATE TABLE IF NOT EXISTS asset_acl (
                 asset_id TEXT NOT NULL REFERENCES assets(asset_id),
                 account_id TEXT NOT NULL REFERENCES accounts(account_id),
                 permission TEXT NOT NULL,
                 PRIMARY KEY(asset_id, account_id)
             );
             CREATE TABLE IF NOT EXISTS idempotency (
                 account_id TEXT NOT NULL,
                 request_id TEXT NOT NULL,
                 request_hash TEXT NOT NULL,
                 response_json TEXT NOT NULL,
                 PRIMARY KEY(account_id, request_id)
             );
             CREATE TABLE IF NOT EXISTS catalog_events (
                 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                 tenant_id TEXT NOT NULL,
                 asset_id TEXT NOT NULL,
                 revision INTEGER NOT NULL,
                 created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS outbox_events (
                 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                 event_id TEXT NOT NULL UNIQUE,
                 tenant_id TEXT NOT NULL,
                 scope_id TEXT NOT NULL,
                 target_id TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 payload_json TEXT NOT NULL,
                 created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS audit_events (
                 event_id TEXT PRIMARY KEY,
                 actor_account_id TEXT NOT NULL REFERENCES accounts(account_id),
                 action TEXT NOT NULL,
                 scope_id TEXT,
                 target_id TEXT,
                 subject_id TEXT,
                 details_json TEXT NOT NULL,
                 created_at TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_targets_scope ON targets(scope_id);
             CREATE INDEX IF NOT EXISTS idx_entity_bindings_scope ON entity_bindings(scope_id);
             CREATE INDEX IF NOT EXISTS idx_scoped_identity_bindings_scope ON scoped_identity_bindings(scope_id, world_epoch);
             CREATE UNIQUE INDEX IF NOT EXISTS idx_scoped_identity_active ON scoped_identity_bindings(scope_id, world_epoch, entity_uuid) WHERE status = 'APPROVED';
             CREATE INDEX IF NOT EXISTS idx_asset_revisions_sha ON asset_revisions(raw_sha256);
             INSERT OR IGNORE INTO scope_acl(scope_id, account_id, role)
                 SELECT scope_id, tenant_id, 'manage' FROM scopes;
             INSERT OR IGNORE INTO asset_acl(asset_id, account_id, permission)
                 SELECT asset_id, owner_account_id, 'manage' FROM assets;
             COMMIT;"
        )?;
        let has_offline_policy: Option<String> = connection
            .query_row(
                "SELECT name FROM pragma_table_info('scopes') WHERE name = 'offline_policy'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if has_offline_policy.is_none() {
            connection.execute("ALTER TABLE scopes ADD COLUMN offline_policy TEXT NOT NULL DEFAULT 'STRICT_APPROVAL'", [])?;
        }
        Ok(())
    }

    fn seed_bootstrap(
        &self,
        account_id: &str,
        password_hash: Option<&str>,
    ) -> Result<(), CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        conn.execute("INSERT OR IGNORE INTO identity_providers(provider_id, display_name, base_url, enabled) VALUES ('official', 'Minecraft official', 'https://sessionserver.mojang.com', 1)", [])?;
        conn.execute(
            "INSERT OR IGNORE INTO accounts(account_id, created_at) VALUES (?1, datetime('now'))",
            [account_id],
        )?;
        if let Some(password_hash) = password_hash {
            PasswordHash::new(password_hash).map_err(|_| {
                CloudError::configuration("invalid bootstrap Argon2id password hash")
            })?;
            conn.execute("INSERT INTO account_credentials(account_id, password_hash) VALUES (?1, ?2) ON CONFLICT(account_id) DO UPDATE SET password_hash = excluded.password_hash", params![account_id, password_hash])?;
        }
        Ok(())
    }

    pub fn authenticate_access_token(&self, token: &str) -> Result<String, CloudError> {
        let access_hash = hash_token(token);
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let session: Option<(String, i64, i64)> = conn.query_row("SELECT account_id, access_expires_at, revoked FROM sessions WHERE access_hash = ?1", [&access_hash], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
        let Some((account_id, expires_at, revoked)) = session else {
            return Err(CloudError::Unauthenticated);
        };
        if revoked != 0 || expires_at <= now_seconds() {
            return Err(CloudError::SessionExpired);
        }
        Ok(account_id)
    }

    pub fn issue_session(&self, login: &LoginRequest) -> Result<SessionResponse, CloudError> {
        validate_slug(&login.account_id, "account_id")?;
        if login.password.is_empty() || login.password.len() > 1024 {
            return Err(CloudError::Unauthenticated);
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let password_hash: Option<String> = conn
            .query_row(
                "SELECT password_hash FROM account_credentials WHERE account_id = ?1",
                [&login.account_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(password_hash) = password_hash else {
            return Err(CloudError::Unauthenticated);
        };
        let parsed = PasswordHash::new(&password_hash).map_err(|_| {
            CloudError::Internal(anyhow::anyhow!("stored password hash is invalid"))
        })?;
        Argon2::default()
            .verify_password(login.password.as_bytes(), &parsed)
            .map_err(|_| CloudError::Unauthenticated)?;
        issue_session_in_transaction(&mut conn, &login.account_id)
    }

    pub fn refresh_session(&self, refresh_token: &str) -> Result<SessionResponse, CloudError> {
        if refresh_token.is_empty() || refresh_token.len() > 512 {
            return Err(CloudError::RefreshReused);
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let refresh_hash = hash_token(refresh_token);
        let session: Option<(String, i64, i64)> = conn.query_row("SELECT account_id, refresh_expires_at, revoked FROM sessions WHERE refresh_hash = ?1", [&refresh_hash], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
        let Some((account_id, expires_at, revoked)) = session else {
            return Err(CloudError::RefreshReused);
        };
        if revoked != 0 {
            return Err(CloudError::RefreshReused);
        }
        if expires_at <= now_seconds() {
            return Err(CloudError::SessionExpired);
        }
        conn.execute(
            "UPDATE sessions SET revoked = 1 WHERE refresh_hash = ?1",
            [&refresh_hash],
        )?;
        issue_session_in_transaction(&mut conn, &account_id)
    }

    pub fn revoke_access_token(&self, access_token: &str) -> Result<(), CloudError> {
        let access_hash = hash_token(access_token);
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        conn.execute(
            "UPDATE sessions SET revoked = 1 WHERE access_hash = ?1",
            [&access_hash],
        )?;
        Ok(())
    }

    pub fn list_providers(&self) -> Result<Vec<serde_json::Value>, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let mut stmt = conn.prepare("SELECT provider_id, display_name, base_url, enabled FROM identity_providers ORDER BY provider_id")?;
        let rows = stmt.query_map([], |row| Ok(serde_json::json!({"provider_id": row.get::<_, String>(0)?, "display_name": row.get::<_, String>(1)?, "base_url": row.get::<_, String>(2)?, "enabled": row.get::<_, i64>(3)? != 0})))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn configure_provider(
        &self,
        input: &IdentityProviderUpdate,
    ) -> Result<serde_json::Value, CloudError> {
        validate_slug(&input.provider_id, "provider_id")?;
        if input.provider_id == "official" {
            return Err(CloudError::InvalidMetadata(
                "official provider is immutable".to_owned(),
            ));
        }
        if input.display_name.trim().is_empty() || input.display_name.len() > 256 {
            return Err(CloudError::invalid_metadata(
                "invalid provider display name",
            ));
        }
        validate_provider_base_url(&input.base_url)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        conn.execute("INSERT INTO identity_providers(provider_id, display_name, base_url, enabled) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(provider_id) DO UPDATE SET display_name = excluded.display_name, base_url = excluded.base_url, enabled = excluded.enabled", params![input.provider_id, input.display_name, input.base_url, i64::from(input.enabled)])?;
        Ok(
            serde_json::json!({"provider_id": input.provider_id, "display_name": input.display_name, "base_url": input.base_url, "enabled": input.enabled}),
        )
    }

    pub fn create_identity_challenge(
        &self,
        account_id: &str,
        input: &CreateIdentityChallenge,
    ) -> Result<IdentityChallengeResponse, CloudError> {
        validate_slug(&input.provider_id, "provider_id")?;
        if input.username.trim().is_empty() || input.username.len() > 256 {
            return Err(CloudError::invalid_metadata("invalid provider username"));
        }
        let expected_profile_uuid = input
            .profile_uuid
            .as_deref()
            .map(normalize_profile_uuid)
            .transpose()?;
        let challenge_id = format!("challenge_{}", uuid::Uuid::new_v4().simple());
        let server_id = uuid::Uuid::new_v4().simple().to_string();
        let expires_at = now_seconds() + 120;
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let enabled: Option<i64> = conn
            .query_row(
                "SELECT enabled FROM identity_providers WHERE provider_id = ?1",
                [&input.provider_id],
                |row| row.get(0),
            )
            .optional()?;
        if enabled != Some(1) {
            return Err(CloudError::IdentityProviderUntrusted);
        }
        conn.execute("INSERT INTO identity_challenges(challenge_hash, account_id, provider_id, username, expected_profile_uuid, server_id, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![hash_token(&challenge_id), account_id, input.provider_id, input.username, expected_profile_uuid, server_id, expires_at])?;
        Ok(IdentityChallengeResponse {
            challenge_id,
            provider_id: input.provider_id.clone(),
            server_id,
            expires_in_seconds: 120,
        })
    }

    pub fn provider_challenge(
        &self,
        account_id: &str,
        challenge_id: &str,
    ) -> Result<ProviderChallenge, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let challenge: Option<(String, String, Option<String>, String, i64, i64)> = conn.query_row("SELECT c.provider_id, c.username, c.expected_profile_uuid, c.server_id, c.expires_at, c.consumed FROM identity_challenges c WHERE c.challenge_hash = ?1 AND c.account_id = ?2", params![hash_token(challenge_id), account_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))).optional()?;
        let Some((provider_id, username, expected_profile_uuid, server_id, expires_at, consumed)) =
            challenge
        else {
            return Err(CloudError::NotFound);
        };
        if consumed != 0 {
            return Err(CloudError::IdentityChallengeReplayed);
        }
        if expires_at <= now_seconds() {
            return Err(CloudError::IdentityChallengeExpired);
        }
        let enabled: Option<i64> = conn
            .query_row(
                "SELECT enabled FROM identity_providers WHERE provider_id = ?1",
                [&provider_id],
                |row| row.get(0),
            )
            .optional()?;
        if enabled != Some(1) {
            return Err(CloudError::IdentityProviderUntrusted);
        }
        Ok(ProviderChallenge {
            provider_id,
            username,
            expected_profile_uuid,
            server_id,
            expires_at,
        })
    }

    pub fn provider_base_url(&self, provider_id: &str) -> Result<String, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let (base_url, enabled): (String, i64) = conn.query_row(
            "SELECT base_url, enabled FROM identity_providers WHERE provider_id = ?1",
            [provider_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if enabled == 0 {
            return Err(CloudError::IdentityProviderUntrusted);
        }
        validate_provider_base_url(&base_url)?;
        Ok(base_url)
    }

    pub fn complete_identity_challenge(
        &self,
        account_id: &str,
        challenge_id: &str,
        profile_uuid: &str,
        display_name: &str,
    ) -> Result<IdentitySummary, CloudError> {
        let profile_uuid = normalize_profile_uuid(profile_uuid)?;
        if display_name.trim().is_empty() || display_name.len() > 16 * 1024 {
            return Err(CloudError::invalid_metadata(
                "invalid verified profile name",
            ));
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        let challenge: (String, Option<String>, i64, i64) = tx.query_row("SELECT provider_id, expected_profile_uuid, expires_at, consumed FROM identity_challenges WHERE challenge_hash = ?1 AND account_id = ?2", params![hash_token(challenge_id), account_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).optional()?.ok_or(CloudError::NotFound)?;
        if challenge.3 != 0 {
            return Err(CloudError::IdentityChallengeReplayed);
        }
        if challenge.2 <= now_seconds() {
            return Err(CloudError::IdentityChallengeExpired);
        }
        if challenge
            .1
            .as_deref()
            .is_some_and(|expected| expected != profile_uuid)
        {
            return Err(CloudError::IdentityProfileMismatch);
        }
        let (kind, provider_id) = if challenge.0 == "official" {
            ("official", None)
        } else {
            ("yggdrasil", Some(challenge.0.clone()))
        };
        let identity_id = format!("identity_{}", uuid::Uuid::new_v4().simple());
        tx.execute(
            "UPDATE identity_challenges SET consumed = 1 WHERE challenge_hash = ?1",
            [hash_token(challenge_id)],
        )?;
        tx.execute("INSERT INTO identities(identity_id, account_id, identity_kind, provider_id, scope_id, profile_uuid, display_name, verified) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, 1)", params![identity_id, account_id, kind, provider_id, profile_uuid, display_name])?;
        tx.commit()?;
        let identity = if kind == "official" {
            format!("official:{profile_uuid}")
        } else {
            format!("yggdrasil:{}:{profile_uuid}", challenge.0)
        };
        Ok(IdentitySummary {
            identity_id,
            account_id: account_id.to_owned(),
            identity,
            display_name: display_name.to_owned(),
            verification_status: "VERIFIED".to_owned(),
        })
    }

    pub fn create_account(
        &self,
        account_id: &str,
        password: &str,
    ) -> Result<AccountSummary, CloudError> {
        validate_slug(account_id, "account_id")?;
        if password.len() < 8 || password.len() > 1024 {
            return Err(CloudError::invalid_metadata(
                "password must be between 8 and 1024 bytes",
            ));
        }
        let salt = SaltString::encode_b64(&rand::random::<[u8; 16]>()).map_err(|_| {
            CloudError::Internal(anyhow::anyhow!("failed to generate password salt"))
        })?;
        let password_hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| CloudError::Internal(anyhow::anyhow!("failed to hash account password")))?
            .to_string();
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        conn.execute("INSERT INTO accounts(account_id, created_at) VALUES (?1, datetime('now')) ON CONFLICT(account_id) DO NOTHING", [account_id])?;
        let inserted = conn.execute("INSERT INTO account_credentials(account_id, password_hash) VALUES (?1, ?2) ON CONFLICT(account_id) DO NOTHING", params![account_id, password_hash])?;
        if inserted == 0 {
            return Err(CloudError::AccessDenied);
        }
        Ok(AccountSummary {
            account_id: account_id.to_owned(),
        })
    }

    pub fn create_identity(
        &self,
        account_id: &str,
        input: &CreateIdentity,
    ) -> Result<IdentitySummary, CloudError> {
        let identity: GameIdentity = input.identity.parse()?;
        if input.display_name.trim().is_empty() || input.display_name.len() > 16 * 1024 {
            return Err(CloudError::invalid_metadata(
                "invalid identity display name",
            ));
        }
        let identity_id = format!("identity_{}", uuid::Uuid::new_v4().simple());
        let (kind, provider_id, scope_id, profile_uuid) = match &identity {
            GameIdentity::Official { profile_uuid } => {
                ("official", None, None, profile_uuid.to_string())
            }
            GameIdentity::Yggdrasil {
                provider_id,
                profile_uuid,
            } => (
                "yggdrasil",
                Some(provider_id.as_str()),
                None,
                profile_uuid.to_string(),
            ),
            GameIdentity::Offline {
                scope_id,
                profile_uuid,
            } => (
                "offline",
                None,
                Some(scope_id.as_str()),
                profile_uuid.to_string(),
            ),
        };
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if let Some(provider_id) = provider_id {
            let trusted: Option<i64> = conn
                .query_row(
                    "SELECT enabled FROM identity_providers WHERE provider_id = ?1",
                    [provider_id],
                    |row| row.get(0),
                )
                .optional()?;
            if trusted != Some(1) {
                return Err(CloudError::AccessDenied);
            }
        }
        conn.execute("INSERT INTO accounts(account_id, created_at) VALUES (?1, datetime('now')) ON CONFLICT(account_id) DO NOTHING", [account_id])?;
        conn.execute("INSERT INTO identities(identity_id, account_id, identity_kind, provider_id, scope_id, profile_uuid, display_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![identity_id, account_id, kind, provider_id, scope_id, profile_uuid, input.display_name])?;
        Ok(IdentitySummary {
            identity_id,
            account_id: account_id.to_owned(),
            identity: identity.wire_string(),
            display_name: input.display_name.clone(),
            verification_status: "PENDING_VERIFICATION".to_owned(),
        })
    }

    pub fn list_identities(&self, account_id: &str) -> Result<Vec<IdentitySummary>, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let mut stmt = conn.prepare("SELECT identity_id, account_id, identity_kind, provider_id, scope_id, profile_uuid, display_name, verified FROM identities WHERE account_id = ?1 ORDER BY identity_id")?;
        let rows = stmt.query_map([account_id], |row| {
            let kind: String = row.get(2)?;
            let provider: Option<String> = row.get(3)?;
            let scope: Option<String> = row.get(4)?;
            let uuid: String = row.get(5)?;
            let identity = match kind.as_str() {
                "official" => format!("official:{uuid}"),
                "yggdrasil" => format!(
                    "yggdrasil:{}:{uuid}",
                    provider.ok_or_else(|| rusqlite::Error::InvalidQuery)?
                ),
                "offline" => format!(
                    "offline:{}:{uuid}",
                    scope.ok_or_else(|| rusqlite::Error::InvalidQuery)?
                ),
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            let verified: i64 = row.get(7)?;
            Ok(IdentitySummary {
                identity_id: row.get(0)?,
                account_id: row.get(1)?,
                identity,
                display_name: row.get(6)?,
                verification_status: if verified == 1 {
                    "VERIFIED".to_owned()
                } else {
                    "PENDING_VERIFICATION".to_owned()
                },
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn create_offline_binding(
        &self,
        account_id: &str,
        input: &OfflineBindingRequest,
    ) -> Result<serde_json::Value, CloudError> {
        validate_slug(&input.scope_id, "scope_id")?;
        validate_slug(&input.world_epoch, "world_epoch")?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let identity: Option<(String, Option<String>, String)> = conn.query_row("SELECT identity_kind, scope_id, profile_uuid FROM identities WHERE identity_id = ?1 AND account_id = ?2", params![input.identity_id, account_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
        let Some((kind, identity_scope, profile_uuid)) = identity else {
            return Err(CloudError::NotFound);
        };
        if kind != "offline" || identity_scope.as_deref() != Some(input.scope_id.as_str()) {
            return Err(CloudError::AccessDenied);
        }
        let scope_policy: Option<(String, String)> = conn.query_row("SELECT scope_id, offline_policy FROM scopes WHERE scope_id = ?1 AND world_epoch = ?2", params![input.scope_id, input.world_epoch], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let Some((_scope_id, offline_policy)) = scope_policy else {
            return Err(CloudError::NotFound);
        };
        if offline_policy != "STRICT_APPROVAL" {
            return Err(CloudError::AccessDenied);
        }
        let target_scope: Option<String> = conn
            .query_row(
                "SELECT scope_id FROM targets WHERE target_id = ?1 AND target_kind = 'PLAYER'",
                [&input.target_id],
                |row| row.get(0),
            )
            .optional()?;
        if target_scope.as_deref() != Some(input.scope_id.as_str()) {
            return Err(CloudError::NotFound);
        }
        if conn.query_row("SELECT 1 FROM scoped_identity_bindings WHERE scope_id = ?1 AND world_epoch = ?2 AND entity_uuid = ?3 AND status = 'APPROVED'", params![input.scope_id, input.world_epoch, profile_uuid], |_| Ok(())).optional()?.is_some() {
            return Err(CloudError::RevisionConflict);
        }
        let binding_id = format!("offline_binding_{}", uuid::Uuid::new_v4().simple());
        conn.execute("INSERT INTO scoped_identity_bindings(binding_id, account_id, identity_id, target_id, scope_id, world_epoch, entity_uuid, verification_method, status, revision, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'STRICT_APPROVAL', 'PENDING_APPROVAL', 0, datetime('now'), datetime('now'))", params![binding_id, account_id, input.identity_id, input.target_id, input.scope_id, input.world_epoch, profile_uuid])?;
        write_audit(
            &conn,
            account_id,
            "offline_binding.requested",
            Some(&input.scope_id),
            Some(&input.target_id),
            Some(&binding_id),
            serde_json::json!({"identity_id": input.identity_id, "entity_uuid": profile_uuid}),
        )?;
        Ok(
            serde_json::json!({"binding_id": binding_id, "identity_id": input.identity_id, "target_id": input.target_id, "scope_id": input.scope_id, "world_epoch": input.world_epoch, "entity_uuid": profile_uuid, "verification_method": "STRICT_APPROVAL", "status": "PENDING_APPROVAL", "revision": 0}),
        )
    }

    pub fn create_claim_code(
        &self,
        account_id: &str,
        target_id: &str,
        input: &ClaimCodeRequest,
    ) -> Result<ClaimCodeResponse, CloudError> {
        if uuid::Uuid::parse_str(&input.entity_uuid).is_err() {
            return Err(CloudError::invalid_metadata("entity_uuid must be a UUID"));
        }
        let expires_in_seconds = input.expires_in_seconds.unwrap_or(600).clamp(60, 86_400);
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let target: Option<(String, String)> = conn
            .query_row(
                "SELECT scope_id, target_kind FROM targets WHERE target_id = ?1",
                [target_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((scope_id, target_kind)) = target else {
            return Err(CloudError::NotFound);
        };
        if target_kind != "PLAYER" {
            return Err(CloudError::invalid_metadata(
                "claim codes require PLAYER targets",
            ));
        }
        if scope_role(&conn, &scope_id, account_id)?.as_deref() != Some("manage") {
            return Err(CloudError::AccessDenied);
        }
        let (stored_epoch, offline_policy): (String, String) = conn.query_row(
            "SELECT world_epoch, offline_policy FROM scopes WHERE scope_id = ?1",
            [&scope_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if stored_epoch != input.world_epoch {
            return Err(CloudError::RevisionConflict);
        }
        if !matches!(offline_policy.as_str(), "CLAIM_CODE" | "FIRST_CLAIM") {
            return Err(CloudError::AccessDenied);
        }
        if offline_policy == "FIRST_CLAIM" && conn.query_row("SELECT 1 FROM scoped_identity_bindings WHERE scope_id = ?1 AND world_epoch = ?2 AND entity_uuid = ?3 AND status = 'APPROVED'", params![scope_id, input.world_epoch, input.entity_uuid], |_| Ok(())).optional()?.is_some() { return Err(CloudError::RevisionConflict); }
        let code = format!("spm_claim_{}", uuid::Uuid::new_v4().simple());
        conn.execute("INSERT INTO claim_codes(code_hash, scope_id, world_epoch, target_id, entity_uuid, issued_by, expires_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))", params![hash_token(&code), scope_id, input.world_epoch, target_id, input.entity_uuid, account_id, now_seconds() + expires_in_seconds as i64])?;
        write_audit(
            &conn,
            account_id,
            "claim_code.issued",
            Some(&scope_id),
            Some(target_id),
            None,
            serde_json::json!({"world_epoch": input.world_epoch, "entity_uuid": input.entity_uuid, "expires_in_seconds": expires_in_seconds}),
        )?;
        Ok(ClaimCodeResponse {
            code,
            scope_id,
            world_epoch: input.world_epoch.clone(),
            target_id: target_id.to_owned(),
            entity_uuid: input.entity_uuid.clone(),
            expires_in_seconds,
        })
    }

    pub fn redeem_claim_code(
        &self,
        account_id: &str,
        input: &RedeemClaimCode,
    ) -> Result<ScopedIdentityBindingSummary, CloudError> {
        if input.code.is_empty() || input.code.len() > 256 {
            return Err(CloudError::invalid_metadata("invalid claim code"));
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let code_hash = hash_token(&input.code);
        let claim: Option<ClaimCodeRecord> = conn.query_row("SELECT scope_id, world_epoch, target_id, entity_uuid, expires_at, attempts, max_attempts, consumed FROM claim_codes WHERE code_hash = ?1 AND revoked = 0", [&code_hash], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?))).optional()?;
        let Some((
            scope_id,
            world_epoch,
            target_id,
            entity_uuid,
            expires_at,
            attempts,
            max_attempts,
            consumed,
        )) = claim
        else {
            return Err(CloudError::NotFound);
        };
        if consumed != 0 || expires_at <= now_seconds() || attempts >= max_attempts {
            return Err(CloudError::AccessDenied);
        }
        let offline_policy: String = conn.query_row(
            "SELECT offline_policy FROM scopes WHERE scope_id = ?1",
            [&scope_id],
            |row| row.get(0),
        )?;
        if !matches!(offline_policy.as_str(), "CLAIM_CODE" | "FIRST_CLAIM") {
            return Err(CloudError::AccessDenied);
        }
        conn.execute(
            "UPDATE claim_codes SET attempts = attempts + 1 WHERE code_hash = ?1",
            [&code_hash],
        )?;
        let identity: Option<(String, Option<String>, String)> = conn.query_row("SELECT identity_kind, scope_id, profile_uuid FROM identities WHERE identity_id = ?1 AND account_id = ?2", params![input.identity_id, account_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
        let Some((kind, identity_scope, profile_uuid)) = identity else {
            return Err(CloudError::NotFound);
        };
        if kind != "offline"
            || identity_scope.as_deref() != Some(scope_id.as_str())
            || profile_uuid != entity_uuid
        {
            return Err(CloudError::AccessDenied);
        }
        let tx = conn.transaction()?;
        if tx.query_row("SELECT 1 FROM scoped_identity_bindings WHERE scope_id = ?1 AND world_epoch = ?2 AND entity_uuid = ?3 AND status = 'APPROVED'", params![scope_id, world_epoch, entity_uuid], |_| Ok(())).optional()?.is_some() { return Err(CloudError::RevisionConflict); }
        let binding_id = format!("offline_binding_{}", uuid::Uuid::new_v4().simple());
        tx.execute("INSERT INTO scoped_identity_bindings(binding_id, account_id, identity_id, target_id, scope_id, world_epoch, entity_uuid, verification_method, status, revision, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'CLAIM_CODE', 'APPROVED', 1, datetime('now'), datetime('now'))", params![binding_id, account_id, input.identity_id, target_id, scope_id, world_epoch, entity_uuid])?;
        tx.execute(
            "UPDATE claim_codes SET consumed = 1 WHERE code_hash = ?1",
            [&code_hash],
        )?;
        let summary = tx.query_row("SELECT binding_id, account_id, identity_id, target_id, scope_id, world_epoch, entity_uuid, verification_method, status, approved_by, revision FROM scoped_identity_bindings WHERE binding_id = ?1", [&binding_id], |row| Ok(ScopedIdentityBindingSummary { binding_id: row.get(0)?, account_id: row.get(1)?, identity_id: row.get(2)?, target_id: row.get(3)?, scope_id: row.get(4)?, world_epoch: row.get(5)?, entity_uuid: row.get(6)?, verification_method: row.get(7)?, status: row.get(8)?, approved_by: row.get(9)?, revision: row.get::<_, i64>(10)? as u64 }))?;
        tx.commit()?;
        write_audit(
            &conn,
            account_id,
            "claim_code.redeemed",
            Some(&scope_id),
            Some(&target_id),
            Some(&binding_id),
            serde_json::json!({"identity_id": input.identity_id, "entity_uuid": entity_uuid}),
        )?;
        Ok(summary)
    }

    pub fn revoke_claim_code(
        &self,
        account_id: &str,
        input: &RevokeClaimCode,
    ) -> Result<(), CloudError> {
        if input.code.is_empty() || input.code.len() > 256 {
            return Err(CloudError::invalid_metadata("invalid claim code"));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let code_hash = hash_token(&input.code);
        let scope_id: Option<String> = conn
            .query_row(
                "SELECT scope_id FROM claim_codes WHERE code_hash = ?1",
                [&code_hash],
                |row| row.get(0),
            )
            .optional()?;
        let Some(scope_id) = scope_id else {
            return Err(CloudError::NotFound);
        };
        if scope_role(&conn, &scope_id, account_id)?.as_deref() != Some("manage") {
            return Err(CloudError::AccessDenied);
        }
        conn.execute(
            "UPDATE claim_codes SET revoked = 1 WHERE code_hash = ?1 AND consumed = 0",
            [&code_hash],
        )?;
        write_audit(
            &conn,
            account_id,
            "claim_code.revoked",
            Some(&scope_id),
            None,
            None,
            serde_json::json!({"code_hash": code_hash}),
        )?;
        Ok(())
    }

    pub fn approve_offline_binding(
        &self,
        account_id: &str,
        binding_id: &str,
        input: &OfflineBindingApproval,
    ) -> Result<ScopedIdentityBindingSummary, CloudError> {
        if !matches!(input.status.as_str(), "APPROVED" | "REJECTED") {
            return Err(CloudError::invalid_metadata(
                "invalid offline binding status",
            ));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let binding: Option<(String, String, String, String, String, String)> = conn.query_row("SELECT account_id, identity_id, target_id, scope_id, world_epoch, entity_uuid FROM scoped_identity_bindings WHERE binding_id = ?1", [binding_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))).optional()?;
        let Some((_owner, _identity_id, _target_id, scope_id, _world_epoch, _entity_uuid)) =
            binding
        else {
            return Err(CloudError::NotFound);
        };
        if scope_role(&conn, &scope_id, account_id)?.as_deref() != Some("manage") {
            return Err(CloudError::AccessDenied);
        }
        let policy: String = conn.query_row(
            "SELECT offline_policy FROM scopes WHERE scope_id = ?1",
            [&scope_id],
            |row| row.get(0),
        )?;
        if policy != "STRICT_APPROVAL" {
            return Err(CloudError::AccessDenied);
        }
        let revision: i64 = conn.query_row(
            "SELECT revision FROM scoped_identity_bindings WHERE binding_id = ?1",
            [binding_id],
            |row| row.get(0),
        )?;
        if revision as u64 != input.expected_revision {
            return Err(CloudError::RevisionConflict);
        }
        if input.status == "APPROVED" && conn.query_row("SELECT 1 FROM scoped_identity_bindings WHERE scope_id = ?1 AND world_epoch = (SELECT world_epoch FROM scoped_identity_bindings WHERE binding_id = ?2) AND entity_uuid = (SELECT entity_uuid FROM scoped_identity_bindings WHERE binding_id = ?2) AND status = 'APPROVED' AND binding_id <> ?2", params![scope_id, binding_id], |_| Ok(())).optional()?.is_some() { return Err(CloudError::RevisionConflict); }
        conn.execute("UPDATE scoped_identity_bindings SET status = ?1, approved_by = ?2, revision = revision + 1, updated_at = datetime('now') WHERE binding_id = ?3", params![input.status, account_id, binding_id])?;
        write_audit(
            &conn,
            account_id,
            "offline_binding.status_changed",
            Some(&scope_id),
            None,
            Some(binding_id),
            serde_json::json!({"status": input.status, "expected_revision": input.expected_revision}),
        )?;
        conn.query_row("SELECT binding_id, account_id, identity_id, target_id, scope_id, world_epoch, entity_uuid, verification_method, status, approved_by, revision FROM scoped_identity_bindings WHERE binding_id = ?1", [binding_id], |row| Ok(ScopedIdentityBindingSummary { binding_id: row.get(0)?, account_id: row.get(1)?, identity_id: row.get(2)?, target_id: row.get(3)?, scope_id: row.get(4)?, world_epoch: row.get(5)?, entity_uuid: row.get(6)?, verification_method: row.get(7)?, status: row.get(8)?, approved_by: row.get(9)?, revision: row.get::<_, i64>(10)? as u64 })).map_err(CloudError::from)
    }

    pub fn create_scope(
        &self,
        account_id: &str,
        input: &CreateScope,
    ) -> Result<ScopeSummary, CloudError> {
        validate_slug(&input.scope_id, "scope_id")?;
        if input.name.trim().is_empty() || input.name.len() > 16 * 1024 {
            return Err(CloudError::invalid_metadata("invalid scope name"));
        }
        validate_slug(&input.world_epoch, "world_epoch")?;
        let offline_policy = input.offline_policy.as_deref().unwrap_or("STRICT_APPROVAL");
        if !matches!(
            offline_policy,
            "STRICT_APPROVAL" | "CLAIM_CODE" | "FIRST_CLAIM" | "DISABLED"
        ) {
            return Err(CloudError::invalid_metadata("invalid offline_policy"));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        conn.execute("INSERT INTO accounts(account_id, created_at) VALUES (?1, datetime('now')) ON CONFLICT(account_id) DO NOTHING", [account_id])?;
        conn.execute("INSERT INTO scopes(scope_id, tenant_id, name, world_epoch, offline_policy, created_at) VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))", params![input.scope_id, account_id, input.name, input.world_epoch, offline_policy])?;
        conn.execute(
            "INSERT INTO scope_acl(scope_id, account_id, role) VALUES (?1, ?2, 'manage')",
            params![input.scope_id, account_id],
        )?;
        Ok(ScopeSummary {
            scope_id: input.scope_id.clone(),
            tenant_id: account_id.to_owned(),
            name: input.name.clone(),
            world_epoch: input.world_epoch.clone(),
            offline_policy: offline_policy.to_owned(),
        })
    }

    pub fn list_scopes(&self, account_id: &str) -> Result<Vec<ScopeSummary>, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let mut stmt = conn.prepare("SELECT s.scope_id, s.tenant_id, s.name, s.world_epoch, s.offline_policy FROM scopes s JOIN scope_acl acl ON acl.scope_id = s.scope_id WHERE acl.account_id = ?1 ORDER BY s.scope_id")?;
        let rows = stmt.query_map([account_id], |row| {
            Ok(ScopeSummary {
                scope_id: row.get(0)?,
                tenant_id: row.get(1)?,
                name: row.get(2)?,
                world_epoch: row.get(3)?,
                offline_policy: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn list_offline_bindings(
        &self,
        account_id: &str,
        scope_id: &str,
    ) -> Result<Vec<ScopedIdentityBindingSummary>, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if scope_role(&conn, scope_id, account_id)?.is_none() {
            return Err(CloudError::AccessDenied);
        }
        let mut stmt = conn.prepare("SELECT binding_id, account_id, identity_id, target_id, scope_id, world_epoch, entity_uuid, verification_method, status, approved_by, revision FROM scoped_identity_bindings WHERE scope_id = ?1 ORDER BY binding_id")?;
        let rows = stmt.query_map([scope_id], |row| {
            Ok(ScopedIdentityBindingSummary {
                binding_id: row.get(0)?,
                account_id: row.get(1)?,
                identity_id: row.get(2)?,
                target_id: row.get(3)?,
                scope_id: row.get(4)?,
                world_epoch: row.get(5)?,
                entity_uuid: row.get(6)?,
                verification_method: row.get(7)?,
                status: row.get(8)?,
                approved_by: row.get(9)?,
                revision: row.get::<_, i64>(10)? as u64,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn list_audit(
        &self,
        account_id: &str,
        scope_id: &str,
        limit: u64,
    ) -> Result<Vec<AuditEntry>, CloudError> {
        let limit = limit.clamp(1, 500) as i64;
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if scope_role(&conn, scope_id, account_id)?.as_deref() != Some("manage") {
            return Err(CloudError::AccessDenied);
        }
        let mut stmt = conn.prepare("SELECT event_id, actor_account_id, action, scope_id, target_id, subject_id, details_json, created_at FROM audit_events WHERE scope_id = ?1 ORDER BY created_at DESC, event_id DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![scope_id, limit], |row| {
            let details: String = row.get(6)?;
            Ok(AuditEntry {
                event_id: row.get(0)?,
                actor_account_id: row.get(1)?,
                action: row.get(2)?,
                scope_id: row.get(3)?,
                target_id: row.get(4)?,
                subject_id: row.get(5)?,
                details: serde_json::from_str(&details).unwrap_or(serde_json::Value::Null),
                created_at: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn list_scope_acl(
        &self,
        account_id: &str,
        scope_id: &str,
    ) -> Result<Vec<ScopeAclEntry>, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let role = scope_role(&conn, scope_id, account_id)?;
        if role.as_deref() != Some("manage") {
            return Err(CloudError::AccessDenied);
        }
        let mut stmt = conn.prepare(
            "SELECT account_id, role FROM scope_acl WHERE scope_id = ?1 ORDER BY account_id",
        )?;
        let rows = stmt.query_map([scope_id], |row| {
            Ok(ScopeAclEntry {
                account_id: row.get(0)?,
                role: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn set_scope_acl(
        &self,
        account_id: &str,
        scope_id: &str,
        update: &ScopeAclUpdate,
    ) -> Result<ScopeAclEntry, CloudError> {
        validate_slug(&update.account_id, "account_id")?;
        if !matches!(update.role.as_str(), "manage" | "edit" | "viewer") {
            return Err(CloudError::invalid_metadata("invalid scope ACL role"));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if scope_role(&conn, scope_id, account_id)?.as_deref() != Some("manage") {
            return Err(CloudError::AccessDenied);
        }
        conn.execute("INSERT INTO accounts(account_id, created_at) VALUES (?1, datetime('now')) ON CONFLICT(account_id) DO NOTHING", [&update.account_id])?;
        conn.execute("INSERT INTO scope_acl(scope_id, account_id, role) VALUES (?1, ?2, ?3) ON CONFLICT(scope_id, account_id) DO UPDATE SET role = excluded.role", params![scope_id, update.account_id, update.role])?;
        Ok(ScopeAclEntry {
            account_id: update.account_id.clone(),
            role: update.role.clone(),
        })
    }

    pub fn create_target(
        &self,
        account_id: &str,
        input: &CreateTarget,
    ) -> Result<serde_json::Value, CloudError> {
        let id = input.id();
        validate_slug(&id, "target_id")?;
        if input.display_name.trim().is_empty() || input.display_name.len() > 16 * 1024 {
            return Err(CloudError::invalid_metadata("invalid target display name"));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if scope_role(&conn, &input.scope_id, account_id)?.as_deref() != Some("manage") {
            return Err(CloudError::AccessDenied);
        }
        let kind = serde_json::to_string(&input.kind)
            .unwrap()
            .trim_matches('"')
            .to_owned();
        conn.execute("INSERT INTO targets(target_id, scope_id, target_kind, display_name, owner_account_id) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, input.scope_id, kind, input.display_name, account_id])?;
        conn.execute(
            "INSERT INTO target_acl(target_id, account_id, role) VALUES (?1, ?2, 'manage')",
            params![id, account_id],
        )?;
        conn.execute("INSERT INTO appearances(target_id) VALUES (?1)", [&id])?;
        Ok(
            serde_json::json!({"target_id": id, "scope_id": input.scope_id, "kind": input.kind.clone(), "display_name": input.display_name, "revision": 0}),
        )
    }

    pub fn list_targets(
        &self,
        account_id: &str,
        scope_id: &str,
    ) -> Result<Vec<TargetSummary>, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if scope_role(&conn, scope_id, account_id)?.is_none() {
            return Err(CloudError::AccessDenied);
        }
        let mut stmt = conn.prepare("SELECT t.target_id, t.scope_id, t.target_kind, t.display_name, t.revision FROM targets t WHERE t.scope_id = ?1 ORDER BY t.target_id")?;
        let rows = stmt.query_map([scope_id], |row| {
            let kind = match row.get::<_, String>(2)?.as_str() {
                "PLAYER" => TargetKind::Player,
                "DUMMY" => TargetKind::Dummy,
                "MAID" => TargetKind::Maid,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(TargetSummary {
                target_id: row.get(0)?,
                scope_id: row.get(1)?,
                kind,
                display_name: row.get(3)?,
                revision: row.get::<_, i64>(4)? as u64,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn list_bindings(
        &self,
        account_id: &str,
        scope_id: &str,
    ) -> Result<Vec<EntityBindingSummary>, CloudError> {
        validate_slug(scope_id, "scope_id")?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if scope_role(&conn, scope_id, account_id)?.is_none() {
            return Err(CloudError::AccessDenied);
        }
        let mut stmt = conn.prepare("SELECT binding_id, scope_id, world_epoch, entity_uuid, entity_kind, target_id, observation_state, last_seen_at, revision FROM entity_bindings WHERE scope_id = ?1 ORDER BY binding_id")?;
        let rows = stmt.query_map([scope_id], |row| {
            Ok(EntityBindingSummary {
                binding_id: row.get(0)?,
                scope_id: row.get(1)?,
                world_epoch: row.get(2)?,
                entity_uuid: row.get(3)?,
                entity_kind: row.get(4)?,
                target_id: row.get(5)?,
                observation_state: row.get(6)?,
                last_seen_at: row.get(7)?,
                revision: row.get::<_, i64>(8)? as u64,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn register_binding(
        &self,
        account_id: &str,
        scope_id: &str,
        input: &RegisterEntityBinding,
    ) -> Result<EntityBindingSummary, CloudError> {
        validate_slug(scope_id, "scope_id")?;
        if input.world_epoch.trim().is_empty() || input.world_epoch.len() > 256 {
            return Err(CloudError::invalid_metadata("invalid world_epoch"));
        }
        if uuid::Uuid::parse_str(&input.entity_uuid).is_err() {
            return Err(CloudError::invalid_metadata("entity_uuid must be a UUID"));
        }
        if input.entity_kind.trim().is_empty() || input.entity_kind.len() > 64 {
            return Err(CloudError::invalid_metadata("invalid entity_kind"));
        }
        validate_slug(&input.target_id, "target_id")?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if !matches!(
            scope_role(&conn, scope_id, account_id)?.as_deref(),
            Some("manage") | Some("edit")
        ) {
            return Err(CloudError::AccessDenied);
        }
        let stored_epoch: Option<String> = conn
            .query_row(
                "SELECT world_epoch FROM scopes WHERE scope_id = ?1",
                [scope_id],
                |row| row.get(0),
            )
            .optional()?;
        if stored_epoch.as_deref() != Some(input.world_epoch.as_str()) {
            return Err(CloudError::RevisionConflict);
        }
        let target_scope: Option<String> = conn
            .query_row(
                "SELECT scope_id FROM targets WHERE target_id = ?1",
                [&input.target_id],
                |row| row.get(0),
            )
            .optional()?;
        if target_scope.as_deref() != Some(scope_id) {
            return Err(CloudError::NotFound);
        }
        let binding_id: Option<String> = conn.query_row("SELECT binding_id FROM entity_bindings WHERE scope_id = ?1 AND world_epoch = ?2 AND entity_uuid = ?3", params![scope_id, input.world_epoch, input.entity_uuid], |row| row.get(0)).optional()?;
        let binding_id =
            binding_id.unwrap_or_else(|| format!("binding_{}", uuid::Uuid::new_v4().simple()));
        conn.execute(
            "INSERT INTO entity_bindings(binding_id, scope_id, world_epoch, entity_uuid, entity_kind, target_id, observation_state, revision) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'REGISTERED', 0) ON CONFLICT(scope_id, world_epoch, entity_uuid) DO UPDATE SET target_id = excluded.target_id, entity_kind = excluded.entity_kind, observation_state = 'REGISTERED', revision = entity_bindings.revision + 1",
            params![binding_id, scope_id, input.world_epoch, input.entity_uuid, input.entity_kind, input.target_id]
        )?;
        conn.query_row("SELECT binding_id, scope_id, world_epoch, entity_uuid, entity_kind, target_id, observation_state, last_seen_at, revision FROM entity_bindings WHERE binding_id = ?1", [&binding_id], |row| Ok(EntityBindingSummary {
            binding_id: row.get(0)?, scope_id: row.get(1)?, world_epoch: row.get(2)?, entity_uuid: row.get(3)?, entity_kind: row.get(4)?, target_id: row.get(5)?, observation_state: row.get(6)?, last_seen_at: row.get(7)?, revision: row.get::<_, i64>(8)? as u64,
        })).map_err(CloudError::from)
    }

    pub fn observe_binding(
        &self,
        account_id: &str,
        binding_id: &str,
        input: &ObserveEntityBinding,
    ) -> Result<EntityBindingSummary, CloudError> {
        if input.world_epoch.trim().is_empty() || input.world_epoch.len() > 256 {
            return Err(CloudError::invalid_metadata("invalid world_epoch"));
        }
        if !matches!(
            input.observation_state.as_str(),
            "ACTIVE" | "STALE" | "OFFLINE"
        ) {
            return Err(CloudError::invalid_metadata("invalid observation_state"));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let binding: Option<(String, String)> = conn
            .query_row(
                "SELECT scope_id, world_epoch FROM entity_bindings WHERE binding_id = ?1",
                [binding_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((scope_id, world_epoch)) = binding else {
            return Err(CloudError::NotFound);
        };
        if world_epoch != input.world_epoch {
            return Err(CloudError::RevisionConflict);
        }
        if scope_role(&conn, &scope_id, account_id)?.is_none() {
            return Err(CloudError::AccessDenied);
        }
        conn.execute("UPDATE entity_bindings SET observation_state = ?1, last_seen_at = datetime('now'), revision = revision + 1 WHERE binding_id = ?2", params![input.observation_state, binding_id])?;
        conn.query_row("SELECT binding_id, scope_id, world_epoch, entity_uuid, entity_kind, target_id, observation_state, last_seen_at, revision FROM entity_bindings WHERE binding_id = ?1", [binding_id], |row| Ok(EntityBindingSummary {
            binding_id: row.get(0)?, scope_id: row.get(1)?, world_epoch: row.get(2)?, entity_uuid: row.get(3)?, entity_kind: row.get(4)?, target_id: row.get(5)?, observation_state: row.get(6)?, last_seen_at: row.get(7)?, revision: row.get::<_, i64>(8)? as u64,
        })).map_err(CloudError::from)
    }

    pub fn join_scope(
        &self,
        account_id: &str,
        scope_id: &str,
        world_epoch: &str,
    ) -> Result<Vec<TargetSummary>, CloudError> {
        validate_slug(scope_id, "scope_id")?;
        validate_slug(world_epoch, "world_epoch")?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if scope_role(&conn, scope_id, account_id)?.is_none() {
            return Err(CloudError::AccessDenied);
        }
        let stored_epoch: Option<String> = conn
            .query_row(
                "SELECT world_epoch FROM scopes WHERE scope_id = ?1",
                [scope_id],
                |row| row.get(0),
            )
            .optional()?;
        if stored_epoch.as_deref() != Some(world_epoch) {
            return Err(CloudError::AccessDenied);
        }
        let mut stmt = conn.prepare("SELECT target_id, scope_id, target_kind, display_name, revision FROM targets WHERE scope_id = ?1 ORDER BY target_id")?;
        let rows = stmt.query_map([scope_id], |row| {
            let kind = match row.get::<_, String>(2)?.as_str() {
                "PLAYER" => TargetKind::Player,
                "DUMMY" => TargetKind::Dummy,
                "MAID" => TargetKind::Maid,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(TargetSummary {
                target_id: row.get(0)?,
                scope_id: row.get(1)?,
                kind,
                display_name: row.get(3)?,
                revision: row.get::<_, i64>(4)? as u64,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn list_acl(&self, account_id: &str, target_id: &str) -> Result<Vec<AclEntry>, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if target_role(&conn, target_id, account_id)?.is_none() {
            return Err(CloudError::AccessDenied);
        }
        let mut stmt = conn.prepare(
            "SELECT account_id, role FROM target_acl WHERE target_id = ?1 ORDER BY account_id",
        )?;
        let rows = stmt.query_map([target_id], |row| {
            Ok(AclEntry {
                account_id: row.get(0)?,
                role: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn set_acl(
        &self,
        account_id: &str,
        target_id: &str,
        update: &AclUpdate,
    ) -> Result<AclEntry, CloudError> {
        validate_slug(&update.account_id, "account_id")?;
        if !matches!(update.role.as_str(), "manage" | "edit" | "viewer") {
            return Err(CloudError::invalid_metadata("invalid target ACL role"));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if target_role(&conn, target_id, account_id)?.as_deref() != Some("manage") {
            return Err(CloudError::AccessDenied);
        }
        conn.execute("INSERT INTO accounts(account_id, created_at) VALUES (?1, datetime('now')) ON CONFLICT(account_id) DO NOTHING", [&update.account_id])?;
        conn.execute("INSERT INTO target_acl(target_id, account_id, role) VALUES (?1, ?2, ?3) ON CONFLICT(target_id, account_id) DO UPDATE SET role = excluded.role", params![target_id, update.account_id, update.role])?;
        Ok(AclEntry {
            account_id: update.account_id.clone(),
            role: update.role.clone(),
        })
    }

    pub fn get_appearance(
        &self,
        account_id: &str,
        target_id: &str,
    ) -> Result<AppearanceState, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if target_role(&conn, target_id, account_id)?.is_none() {
            return Err(CloudError::AccessDenied);
        }
        conn.query_row("SELECT a.target_id, a.revision, a.asset_id, a.asset_revision, a.raw_sha256, a.texture_id, a.scale, a.disabled FROM appearances a WHERE a.target_id = ?1", [target_id], |row| Ok(AppearanceState { target_id: row.get(0)?, revision: row.get::<_, i64>(1)? as u64, asset_id: row.get(2)?, asset_revision: row.get::<_, Option<i64>>(3)?.map(|value| value as u64), raw_sha256: row.get(4)?, texture_id: row.get(5)?, scale: row.get(6)?, disabled: row.get::<_, i64>(7)? != 0 })).optional()?.ok_or(CloudError::NotFound)
    }

    pub fn update_appearance(
        &self,
        account_id: &str,
        target_id: &str,
        update: &crate::models::AppearanceUpdate,
    ) -> Result<AppearanceMutation, CloudError> {
        if update.request_id.is_empty() || update.request_id.len() > 128 {
            return Err(CloudError::invalid_metadata("invalid request_id"));
        }
        if let Some(scale) = update.scale {
            if !scale.is_finite() || !(0.01..=100.0).contains(&scale) {
                return Err(CloudError::invalid_metadata("invalid scale"));
            }
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        let request_hash = hex::encode(sha2::Sha256::digest(
            serde_json::to_vec(&(target_id, update))
                .map_err(|_| CloudError::invalid_metadata("invalid appearance payload"))?,
        ));
        if let Some((existing_hash, response_json)) = tx.query_row("SELECT request_hash, response_json FROM idempotency WHERE account_id = ?1 AND request_id = ?2", params![account_id, update.request_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).optional()? {
            if existing_hash != request_hash { return Err(CloudError::IdempotencyConflict); }
            return serde_json::from_str(&response_json).map_err(|_| CloudError::Internal(anyhow::anyhow!("stored idempotency response is invalid")));
        }
        if target_role(&tx, target_id, account_id)?.as_deref() != Some("manage")
            && target_role(&tx, target_id, account_id)?.as_deref() != Some("edit")
        {
            return Err(CloudError::AccessDenied);
        }
        if let Some(asset_id) = update.asset_id.as_deref() {
            if asset_permission(&tx, asset_id, account_id)?
                .is_none_or(|permission| !matches!(permission.as_str(), "manage" | "use"))
            {
                return Err(CloudError::AccessDenied);
            }
        }
        let current: AppearanceState = tx.query_row("SELECT a.target_id, a.revision, a.asset_id, a.asset_revision, a.raw_sha256, a.texture_id, a.scale, a.disabled FROM appearances a WHERE a.target_id = ?1", [target_id], |row| Ok(AppearanceState { target_id: row.get(0)?, revision: row.get::<_, i64>(1)? as u64, asset_id: row.get(2)?, asset_revision: row.get::<_, Option<i64>>(3)?.map(|value| value as u64), raw_sha256: row.get(4)?, texture_id: row.get(5)?, scale: row.get(6)?, disabled: row.get::<_, i64>(7)? != 0 })).optional()?.ok_or(CloudError::NotFound)?;
        if current.revision != update.expected_revision {
            return Err(CloudError::RevisionConflict);
        }
        let next_revision = current.revision + 1;
        tx.execute("UPDATE appearances SET revision = ?1, asset_id = ?2, asset_revision = ?3, raw_sha256 = ?4, texture_id = ?5, scale = ?6, disabled = ?7 WHERE target_id = ?8", params![next_revision as i64, update.asset_id, update.asset_revision.map(|value| value as i64), update.raw_sha256, update.texture_id, update.scale, i64::from(update.disabled), target_id])?;
        let state = AppearanceState {
            target_id: target_id.to_owned(),
            revision: next_revision,
            asset_id: update.asset_id.clone(),
            asset_revision: update.asset_revision,
            raw_sha256: update.raw_sha256.clone(),
            texture_id: update.texture_id.clone(),
            scale: update.scale,
            disabled: update.disabled,
        };
        let scope_id: String = tx.query_row(
            "SELECT scope_id FROM targets WHERE target_id = ?1",
            [target_id],
            |row| row.get(0),
        )?;
        let event_id = format!("event_{}", uuid::Uuid::new_v4().simple());
        let appearance_json = serde_json::to_string(&state).map_err(|_| {
            CloudError::Internal(anyhow::anyhow!("failed to encode appearance event"))
        })?;
        let mutation = AppearanceMutation {
            appearance: state,
            event_id,
            scope_id,
        };
        let response_json = serde_json::to_string(&mutation).map_err(|_| {
            CloudError::Internal(anyhow::anyhow!("failed to encode idempotency response"))
        })?;
        tx.execute("INSERT INTO idempotency(account_id, request_id, request_hash, response_json) VALUES (?1, ?2, ?3, ?4)", params![account_id, update.request_id, request_hash, response_json])?;
        tx.execute("INSERT INTO outbox_events(event_id, tenant_id, scope_id, target_id, kind, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, 'APPEARANCE_UPDATED', ?5, datetime('now'))", params![mutation.event_id, account_id, mutation.scope_id, target_id, appearance_json])?;
        tx.commit()?;
        Ok(mutation)
    }

    pub fn scope_id_for_target(
        &self,
        account_id: &str,
        target_id: &str,
    ) -> Result<Option<String>, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if target_role(&conn, target_id, account_id)?.is_none() {
            return Ok(None);
        }
        conn.query_row(
            "SELECT scope_id FROM targets WHERE target_id = ?1",
            [target_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(CloudError::from)
    }

    pub fn outbox_recovery(
        &self,
        account_id: &str,
        scope_id: &str,
        after: u64,
        requested_limit: usize,
    ) -> Result<serde_json::Value, CloudError> {
        let limit = requested_limit.clamp(1, 256);
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if scope_role(&conn, scope_id, account_id)?.is_none() {
            return Err(CloudError::AccessDenied);
        }
        let mut stmt = conn.prepare("SELECT sequence, event_id, target_id, kind, payload_json FROM outbox_events WHERE scope_id = ?1 AND sequence > ?2 ORDER BY sequence LIMIT ?3")?;
        let rows = stmt.query_map(params![scope_id, after as i64, limit as i64], |row| {
            let payload: String = row.get(4)?;
            let payload = serde_json::from_str::<serde_json::Value>(&payload)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(serde_json::json!({
                "sequence": row.get::<_, i64>(0)?,
                "event_id": row.get::<_, String>(1)?,
                "target_id": row.get::<_, String>(2)?,
                "kind": row.get::<_, String>(3)?,
                "payload": payload
            }))
        })?;
        let entries = rows.collect::<Result<Vec<_>, _>>()?;
        let to_cursor = entries
            .last()
            .and_then(|entry| entry.get("sequence"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(after as i64)
            .max(0) as u64;
        let has_more = entries.len() == limit;
        Ok(serde_json::json!({
            "scope_id": scope_id,
            "from_cursor": after,
            "to_cursor": to_cursor,
            "entries": entries,
            "has_more": has_more
        }))
    }

    pub fn list_assets(&self, account_id: &str) -> Result<Vec<AssetSummary>, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let mut stmt = conn.prepare("SELECT r.asset_id, r.revision, r.name, r.format, r.raw_sha256, r.byte_length FROM asset_revisions r JOIN asset_acl acl ON acl.asset_id = r.asset_id WHERE acl.account_id = ?1 AND acl.permission IN ('manage', 'use', 'discover') ORDER BY r.asset_id, r.revision")?;
        let rows = stmt.query_map([account_id], |row| {
            Ok(AssetSummary {
                asset_id: row.get(0)?,
                revision: row.get::<_, i64>(1)? as u64,
                name: row.get(2)?,
                format: row.get(3)?,
                raw_sha256: row.get(4)?,
                byte_length: row.get::<_, i64>(5)? as u64,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn register_asset(
        &self,
        account_id: &str,
        asset_id: &str,
        name: &str,
        format: &str,
        sha256: &str,
        byte_length: u64,
        object_path: &Path,
    ) -> Result<AssetSummary, CloudError> {
        self.register_asset_with_idempotency(
            account_id,
            asset_id,
            name,
            format,
            sha256,
            byte_length,
            object_path,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn register_asset_with_idempotency(
        &self,
        account_id: &str,
        asset_id: &str,
        name: &str,
        format: &str,
        sha256: &str,
        byte_length: u64,
        object_path: &Path,
        idempotency: Option<(&str, &str)>,
    ) -> Result<AssetSummary, CloudError> {
        validate_slug(asset_id, "asset_id")?;
        if name.is_empty() || name.len() > 16 * 1024 || format.is_empty() || format.len() > 64 {
            return Err(CloudError::invalid_metadata("invalid asset metadata"));
        }
        if let Some((request_id, _)) = idempotency {
            if request_id.is_empty() || request_id.len() > 128 {
                return Err(CloudError::invalid_metadata("invalid Idempotency-Key"));
            }
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        if let Some((request_id, request_hash)) = idempotency {
            if let Some((existing_hash, response_json)) = tx.query_row("SELECT request_hash, response_json FROM idempotency WHERE account_id = ?1 AND request_id = ?2", params![account_id, request_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).optional()? {
                if existing_hash != request_hash { return Err(CloudError::IdempotencyConflict); }
                return serde_json::from_str(&response_json).map_err(|_| CloudError::Internal(anyhow::anyhow!("stored asset idempotency response is invalid")));
            }
        }
        let existing_owner: Option<String> = tx
            .query_row(
                "SELECT owner_account_id FROM assets WHERE asset_id = ?1",
                [asset_id],
                |row| row.get(0),
            )
            .optional()?;
        if existing_owner.is_some_and(|owner| owner != account_id) {
            return Err(CloudError::AccessDenied);
        }
        tx.execute("INSERT INTO assets(asset_id, owner_account_id, current_revision) VALUES (?1, ?2, 1) ON CONFLICT(asset_id) DO UPDATE SET current_revision = current_revision + 1", params![asset_id, account_id])?;
        let revision: i64 = tx.query_row(
            "SELECT current_revision FROM assets WHERE asset_id = ?1",
            [asset_id],
            |row| row.get(0),
        )?;
        tx.execute("INSERT INTO asset_revisions(asset_id, revision, name, format, raw_sha256, byte_length, object_path, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))", params![asset_id, revision, name, format, sha256, byte_length as i64, object_path.to_string_lossy().to_string()])?;
        tx.execute("INSERT INTO catalog_events(tenant_id, asset_id, revision, created_at) VALUES (?1, ?2, ?3, datetime('now'))", params![account_id, asset_id, revision])?;
        tx.execute("INSERT INTO asset_acl(asset_id, account_id, permission) VALUES (?1, ?2, 'manage') ON CONFLICT(asset_id, account_id) DO UPDATE SET permission = 'manage'", params![asset_id, account_id])?;
        let summary = AssetSummary {
            asset_id: asset_id.to_owned(),
            revision: revision as u64,
            name: name.to_owned(),
            format: format.to_owned(),
            raw_sha256: sha256.to_owned(),
            byte_length,
        };
        if let Some((request_id, request_hash)) = idempotency {
            let response_json = serde_json::to_string(&summary).map_err(|_| {
                CloudError::Internal(anyhow::anyhow!(
                    "failed to encode asset idempotency response"
                ))
            })?;
            tx.execute("INSERT INTO idempotency(account_id, request_id, request_hash, response_json) VALUES (?1, ?2, ?3, ?4)", params![account_id, request_id, request_hash, response_json])?;
        }
        tx.commit()?;
        Ok(summary)
    }

    pub fn catalog_recovery(
        &self,
        account_id: &str,
        after: u64,
        requested_limit: usize,
    ) -> Result<serde_json::Value, CloudError> {
        let limit = requested_limit.clamp(1, 256);
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let mut stmt = conn.prepare("SELECT e.sequence, r.asset_id, r.revision, r.name, r.raw_sha256, r.byte_length FROM catalog_events e JOIN asset_revisions r ON r.asset_id = e.asset_id AND r.revision = e.revision WHERE e.tenant_id = ?1 AND e.sequence > ?2 ORDER BY e.sequence LIMIT ?3")?;
        let rows = stmt.query_map(params![account_id, after as i64, limit as i64], |row| {
            Ok(serde_json::json!({
                "sequence": row.get::<_, i64>(0)?,
                "asset_id": row.get::<_, String>(1)?,
                "revision": row.get::<_, i64>(2)?,
                "name": row.get::<_, String>(3)?,
                "raw_sha256": row.get::<_, String>(4)?,
                "byte_length": row.get::<_, i64>(5)?
            }))
        })?;
        let entries = rows.collect::<Result<Vec<_>, _>>()?;
        let to_cursor = entries
            .last()
            .and_then(|entry| entry.get("sequence"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(after as i64)
            .max(0) as u64;
        let has_more = entries.len() == limit;
        Ok(serde_json::json!({
            "view_epoch": 1,
            "from_cursor": {"tenant_id": account_id, "view_epoch": 1, "offset": after},
            "to_cursor": {"tenant_id": account_id, "view_epoch": 1, "offset": to_cursor},
            "entries": entries,
            "has_more": has_more
        }))
    }

    pub fn asset_content(
        &self,
        account_id: &str,
        asset_id: &str,
        revision: u64,
    ) -> Result<AssetContent, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if asset_permission(&conn, asset_id, account_id)?
            .is_none_or(|permission| !matches!(permission.as_str(), "manage" | "render_read"))
        {
            return Err(CloudError::AccessDenied);
        }
        conn.query_row("SELECT object_path, byte_length, raw_sha256, name, format FROM asset_revisions WHERE asset_id = ?1 AND revision = ?2", params![asset_id, revision as i64], |row| Ok(AssetContent { path: PathBuf::from(row.get::<_, String>(0)?), length: row.get::<_, i64>(1)? as u64, raw_sha256: row.get(2)?, name: row.get(3)?, format: row.get(4)? })).optional()?.ok_or(CloudError::NotFound)
    }

    pub fn list_asset_acl(
        &self,
        account_id: &str,
        asset_id: &str,
    ) -> Result<Vec<AssetAclEntry>, CloudError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if asset_permission(&conn, asset_id, account_id)?.as_deref() != Some("manage") {
            return Err(CloudError::AccessDenied);
        }
        let mut stmt = conn.prepare(
            "SELECT account_id, permission FROM asset_acl WHERE asset_id = ?1 ORDER BY account_id",
        )?;
        let rows = stmt.query_map([asset_id], |row| {
            Ok(AssetAclEntry {
                account_id: row.get(0)?,
                permission: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn set_asset_acl(
        &self,
        account_id: &str,
        asset_id: &str,
        update: &AssetAclUpdate,
    ) -> Result<AssetAclEntry, CloudError> {
        validate_slug(&update.account_id, "account_id")?;
        if !matches!(
            update.permission.as_str(),
            "discover" | "use" | "render_read" | "manage"
        ) {
            return Err(CloudError::invalid_metadata("invalid asset permission"));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        if asset_permission(&conn, asset_id, account_id)?.as_deref() != Some("manage") {
            return Err(CloudError::AccessDenied);
        }
        conn.execute("INSERT INTO accounts(account_id, created_at) VALUES (?1, datetime('now')) ON CONFLICT(account_id) DO NOTHING", [&update.account_id])?;
        conn.execute("INSERT INTO asset_acl(asset_id, account_id, permission) VALUES (?1, ?2, ?3) ON CONFLICT(asset_id, account_id) DO UPDATE SET permission = excluded.permission", params![asset_id, update.account_id, update.permission])?;
        write_audit(
            &conn,
            account_id,
            "asset_acl.updated",
            None,
            Some(asset_id),
            Some(&update.account_id),
            serde_json::json!({"permission": update.permission}),
        )?;
        Ok(AssetAclEntry {
            account_id: update.account_id.clone(),
            permission: update.permission.clone(),
        })
    }

    pub fn object_path_for_sha(&self, sha256: &str) -> PathBuf {
        self.object_dir
            .join(&sha256[0..2.min(sha256.len())])
            .join(sha256)
    }
}

fn write_audit(
    conn: &Connection,
    actor_account_id: &str,
    action: &str,
    scope_id: Option<&str>,
    target_id: Option<&str>,
    subject_id: Option<&str>,
    details: serde_json::Value,
) -> Result<(), CloudError> {
    conn.execute("INSERT INTO audit_events(event_id, actor_account_id, action, scope_id, target_id, subject_id, details_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))", params![format!("audit_{}", uuid::Uuid::new_v4().simple()), actor_account_id, action, scope_id, target_id, subject_id, details.to_string()])?;
    Ok(())
}

fn asset_permission(
    conn: &Connection,
    asset_id: &str,
    account_id: &str,
) -> Result<Option<String>, CloudError> {
    conn.query_row(
        "SELECT permission FROM asset_acl WHERE asset_id = ?1 AND account_id = ?2",
        params![asset_id, account_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(CloudError::from)
}

fn scope_role(
    conn: &Connection,
    scope_id: &str,
    account_id: &str,
) -> Result<Option<String>, CloudError> {
    conn.query_row(
        "SELECT role FROM scope_acl WHERE scope_id = ?1 AND account_id = ?2",
        params![scope_id, account_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(CloudError::from)
}

fn target_role(
    conn: &Connection,
    target_id: &str,
    account_id: &str,
) -> Result<Option<String>, CloudError> {
    conn.query_row(
        "SELECT role FROM target_acl WHERE target_id = ?1 AND account_id = ?2",
        params![target_id, account_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(CloudError::from)
}

fn normalize_profile_uuid(value: &str) -> Result<String, CloudError> {
    let compact = value.trim().replace('-', "").to_ascii_lowercase();
    if compact.len() != 32 || !compact.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CloudError::IdentityProfileMismatch);
    }
    let canonical = format!(
        "{}-{}-{}-{}-{}",
        &compact[0..8],
        &compact[8..12],
        &compact[12..16],
        &compact[16..20],
        &compact[20..32]
    );
    uuid::Uuid::parse_str(&canonical)
        .map(|uuid| uuid.to_string())
        .map_err(|_| CloudError::IdentityProfileMismatch)
}

fn validate_provider_base_url(value: &str) -> Result<(), CloudError> {
    let url = reqwest::Url::parse(value).map_err(|_| CloudError::IdentityProviderUntrusted)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.username() != ""
        || url.password().is_some()
        || (url.path() != "" && url.path() != "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(CloudError::IdentityProviderUntrusted);
    }
    if let Some(host) = url.host_str() {
        if let Ok(address) = host.parse::<std::net::IpAddr>() {
            let private = match address {
                std::net::IpAddr::V4(ip) => {
                    ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
                }
                std::net::IpAddr::V6(ip) => {
                    ip.is_loopback() || ip.is_unspecified() || ip.is_unique_local()
                }
            };
            if private {
                return Err(CloudError::IdentityProviderUntrusted);
            }
        }
    }
    Ok(())
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn hash_token(token: &str) -> String {
    hex::encode(sha2::Sha256::digest(token.as_bytes()))
}

fn issue_session_in_transaction(
    conn: &mut Connection,
    account_id: &str,
) -> Result<SessionResponse, CloudError> {
    const ACCESS_EXPIRES_IN: u64 = 15 * 60;
    const REFRESH_EXPIRES_IN: u64 = 30 * 24 * 60 * 60;
    let access_token = format!("spm_access_{}", uuid::Uuid::new_v4().simple());
    let refresh_token = format!("spm_refresh_{}", uuid::Uuid::new_v4().simple());
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO sessions(session_id, account_id, access_hash, refresh_hash, access_expires_at, refresh_expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![uuid::Uuid::new_v4().to_string(), account_id, hash_token(&access_token), hash_token(&refresh_token), now_seconds() + ACCESS_EXPIRES_IN as i64, now_seconds() + REFRESH_EXPIRES_IN as i64]
    )?;
    tx.commit()?;
    Ok(SessionResponse {
        access_token,
        refresh_token,
        access_expires_in_seconds: ACCESS_EXPIRES_IN,
        refresh_expires_in_seconds: REFRESH_EXPIRES_IN,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn sqlite_wal_and_cas_revision_are_atomic() {
        let dir = tempdir().unwrap();
        let config = CloudConfig {
            instance_id: "test".into(),
            origin: "https://localhost".into(),
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            database_path: dir.path().join("test.db"),
            object_dir: dir.path().join("objects"),
            access_token: Some("secret".into()),
            bootstrap_account_id: "account_local".into(),
            bootstrap_password_hash: None,
            max_asset_bytes: 128 * 1024 * 1024,
            max_message_bytes: 64 * 1024,
        };
        let store = CloudStore::open(&config).unwrap();
        assert_eq!(
            store
                .create_account("account_editor", "correct horse battery staple")
                .unwrap()
                .account_id,
            "account_editor"
        );
        assert!(
            store
                .issue_session(&LoginRequest {
                    account_id: "account_editor".into(),
                    password: "correct horse battery staple".into()
                })
                .is_ok()
        );
        let challenge = store
            .create_identity_challenge(
                "account_local",
                &crate::models::CreateIdentityChallenge {
                    provider_id: "official".into(),
                    username: "Player".into(),
                    profile_uuid: None,
                },
            )
            .unwrap();
        assert_eq!(
            store
                .provider_challenge("account_local", &challenge.challenge_id)
                .unwrap()
                .server_id,
            challenge.server_id
        );
        let verified = store
            .complete_identity_challenge(
                "account_local",
                &challenge.challenge_id,
                "123456781234123412341234567890ab",
                "Player",
            )
            .unwrap();
        assert_eq!(verified.verification_status, "VERIFIED");
        assert!(matches!(
            store.provider_challenge("account_local", &challenge.challenge_id),
            Err(CloudError::IdentityChallengeReplayed)
        ));
        assert!(matches!(
            store.configure_provider(&crate::models::IdentityProviderUpdate {
                provider_id: "private".into(),
                display_name: "Private".into(),
                base_url: "https://127.0.0.1".into(),
                enabled: true
            }),
            Err(CloudError::IdentityProviderUntrusted)
        ));
        let scope = store
            .create_scope(
                "account_local",
                &CreateScope {
                    scope_id: "scope".into(),
                    name: "Scope".into(),
                    world_epoch: "epoch-1".into(),
                    offline_policy: Some("STRICT_APPROVAL".into()),
                },
            )
            .unwrap();
        assert_eq!(scope.scope_id, "scope");
        store
            .set_scope_acl(
                "account_local",
                "scope",
                &ScopeAclUpdate {
                    account_id: "account_editor".into(),
                    role: "viewer".into(),
                },
            )
            .unwrap();
        assert_eq!(store.list_scopes("account_editor").unwrap().len(), 1);
        let target = CreateTarget {
            scope_id: "scope".into(),
            target_id: Some("target".into()),
            kind: crate::models::TargetKind::Player,
            display_name: "Player".into(),
        };
        store.create_target("account_local", &target).unwrap();
        let binding = store
            .register_binding(
                "account_local",
                "scope",
                &crate::models::RegisterEntityBinding {
                    world_epoch: "epoch-1".into(),
                    entity_uuid: "12345678-1234-1234-1234-1234567890ab".into(),
                    entity_kind: "PLAYER".into(),
                    target_id: "target".into(),
                },
            )
            .unwrap();
        assert_eq!(binding.observation_state, "REGISTERED");
        let observed = store
            .observe_binding(
                "account_local",
                &binding.binding_id,
                &crate::models::ObserveEntityBinding {
                    world_epoch: "epoch-1".into(),
                    observation_state: "ACTIVE".into(),
                },
            )
            .unwrap();
        assert_eq!(observed.observation_state, "ACTIVE");
        assert!(
            store
                .register_binding(
                    "account_local",
                    "scope",
                    &crate::models::RegisterEntityBinding {
                        world_epoch: "old-epoch".into(),
                        entity_uuid: "12345678-1234-1234-1234-1234567890ab".into(),
                        entity_kind: "PLAYER".into(),
                        target_id: "target".into()
                    }
                )
                .is_err()
        );
        let offline_identity = store
            .create_identity(
                "account_editor",
                &crate::models::CreateIdentity {
                    identity: "offline:scope:12345678-1234-1234-1234-1234567890ac".into(),
                    display_name: "Offline Player".into(),
                },
            )
            .unwrap();
        let pending = store
            .create_offline_binding(
                "account_editor",
                &crate::models::OfflineBindingRequest {
                    scope_id: "scope".into(),
                    world_epoch: "epoch-1".into(),
                    identity_id: offline_identity.identity_id.clone(),
                    target_id: "target".into(),
                },
            )
            .unwrap();
        assert_eq!(pending["status"], "PENDING_APPROVAL");
        let approved = store
            .approve_offline_binding(
                "account_local",
                pending["binding_id"].as_str().unwrap(),
                &crate::models::OfflineBindingApproval {
                    status: "APPROVED".into(),
                    expected_revision: 0,
                },
            )
            .unwrap();
        assert_eq!(approved.status, "APPROVED");
        let claim_scope = store
            .create_scope(
                "account_local",
                &CreateScope {
                    scope_id: "claim-scope".into(),
                    name: "Claim Scope".into(),
                    world_epoch: "epoch-1".into(),
                    offline_policy: Some("CLAIM_CODE".into()),
                },
            )
            .unwrap();
        assert_eq!(claim_scope.offline_policy, "CLAIM_CODE");
        store
            .create_target(
                "account_local",
                &CreateTarget {
                    scope_id: "claim-scope".into(),
                    target_id: Some("claim-target".into()),
                    kind: crate::models::TargetKind::Player,
                    display_name: "Claim Player".into(),
                },
            )
            .unwrap();
        let claim = store
            .create_claim_code(
                "account_local",
                "claim-target",
                &crate::models::ClaimCodeRequest {
                    world_epoch: "epoch-1".into(),
                    entity_uuid: "12345678-1234-1234-1234-1234567890ad".into(),
                    expires_in_seconds: Some(600),
                },
            )
            .unwrap();
        let claim_identity = store
            .create_identity(
                "account_editor",
                &crate::models::CreateIdentity {
                    identity: "offline:claim-scope:12345678-1234-1234-1234-1234567890ad".into(),
                    display_name: "Claimed Player".into(),
                },
            )
            .unwrap();
        let redeemed = store
            .redeem_claim_code(
                "account_editor",
                &crate::models::RedeemClaimCode {
                    code: claim.code,
                    identity_id: claim_identity.identity_id,
                },
            )
            .unwrap();
        assert_eq!(redeemed.verification_method, "CLAIM_CODE");
        assert!(
            store
                .list_audit("account_local", "scope", 100)
                .unwrap()
                .iter()
                .any(|entry| entry.action == "offline_binding.status_changed")
        );
        assert!(
            store
                .list_audit("account_local", "claim-scope", 100)
                .unwrap()
                .iter()
                .any(|entry| entry.action == "claim_code.redeemed")
        );
        let first = store.get_appearance("account_local", "target").unwrap();
        assert_eq!(first.revision, 0);
        let update = crate::models::AppearanceUpdate {
            request_id: "req".into(),
            expected_revision: 0,
            asset_id: None,
            asset_revision: None,
            raw_sha256: None,
            texture_id: Some("texture".into()),
            scale: Some(1.0),
            disabled: false,
        };
        let second = store
            .update_appearance("account_local", "target", &update)
            .unwrap();
        assert_eq!(second.appearance.revision, 1);
        let outbox = store
            .outbox_recovery("account_local", "scope", 0, 10)
            .unwrap();
        assert_eq!(outbox["entries"].as_array().unwrap().len(), 1);
        assert_eq!(
            store
                .update_appearance("account_local", "target", &update)
                .unwrap()
                .appearance
                .revision,
            1
        );
        let mut conflicting_request = update.clone();
        conflicting_request.texture_id = Some("different".into());
        assert!(matches!(
            store.update_appearance("account_local", "target", &conflicting_request),
            Err(CloudError::IdempotencyConflict)
        ));

        let asset = store
            .register_asset(
                "account_local",
                "asset",
                "model.ysm",
                "application/octet-stream",
                &"a".repeat(64),
                1,
                &dir.path().join("asset"),
            )
            .unwrap();
        assert_eq!(asset.revision, 1);
        assert!(matches!(
            store.asset_content("account_editor", "asset", 1),
            Err(CloudError::AccessDenied)
        ));
        store
            .set_asset_acl(
                "account_local",
                "asset",
                &crate::models::AssetAclUpdate {
                    account_id: "account_editor".into(),
                    permission: "render_read".into(),
                },
            )
            .unwrap();
        assert_eq!(
            store
                .asset_content("account_editor", "asset", 1)
                .unwrap()
                .raw_sha256,
            "a".repeat(64)
        );
        let idem = store
            .register_asset_with_idempotency(
                "account_local",
                "idem-asset",
                "model.ysm",
                "application/octet-stream",
                &"b".repeat(64),
                1,
                &dir.path().join("idem-asset"),
                Some(("upload-1", "request-hash")),
            )
            .unwrap();
        let idem_retry = store
            .register_asset_with_idempotency(
                "account_local",
                "idem-asset",
                "model.ysm",
                "application/octet-stream",
                &"b".repeat(64),
                1,
                &dir.path().join("idem-asset"),
                Some(("upload-1", "request-hash")),
            )
            .unwrap();
        assert_eq!(idem.revision, idem_retry.revision);
        assert!(matches!(
            store.register_asset_with_idempotency(
                "account_local",
                "idem-asset",
                "model.ysm",
                "application/octet-stream",
                &"b".repeat(64),
                1,
                &dir.path().join("idem-asset"),
                Some(("upload-1", "different-hash"))
            ),
            Err(CloudError::IdempotencyConflict)
        ));
        let catalog = store.catalog_recovery("account_local", 0, 10).unwrap();
        assert_eq!(catalog["entries"].as_array().unwrap().len(), 2);
    }
}
