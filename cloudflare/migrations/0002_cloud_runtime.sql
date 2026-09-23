CREATE TABLE IF NOT EXISTS accounts (
  account_id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS account_credentials (
  account_id TEXT PRIMARY KEY,
  password_hash TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
  session_id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL,
  access_hash TEXT NOT NULL UNIQUE,
  refresh_hash TEXT NOT NULL UNIQUE,
  access_expires_at INTEGER NOT NULL,
  refresh_expires_at INTEGER NOT NULL,
  revoked INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS scopes (
  scope_id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  name TEXT NOT NULL,
  world_epoch TEXT NOT NULL,
  offline_policy TEXT NOT NULL DEFAULT 'STRICT_APPROVAL',
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS scope_acl (
  scope_id TEXT NOT NULL,
  account_id TEXT NOT NULL,
  role TEXT NOT NULL,
  PRIMARY KEY (scope_id, account_id)
);

CREATE TABLE IF NOT EXISTS targets (
  target_id TEXT PRIMARY KEY,
  scope_id TEXT NOT NULL,
  target_kind TEXT NOT NULL,
  display_name TEXT NOT NULL,
  owner_account_id TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS target_acl (
  target_id TEXT NOT NULL,
  account_id TEXT NOT NULL,
  role TEXT NOT NULL,
  PRIMARY KEY (target_id, account_id)
);

CREATE TABLE IF NOT EXISTS entity_bindings (
  binding_id TEXT PRIMARY KEY,
  scope_id TEXT NOT NULL,
  world_epoch TEXT NOT NULL,
  entity_uuid TEXT NOT NULL,
  entity_kind TEXT NOT NULL,
  target_id TEXT NOT NULL,
  observation_state TEXT NOT NULL DEFAULT 'REGISTERED',
  last_seen_at TEXT,
  revision INTEGER NOT NULL DEFAULT 0,
  UNIQUE(scope_id, world_epoch, entity_uuid)
);

CREATE TABLE IF NOT EXISTS appearances (
  target_id TEXT PRIMARY KEY,
  revision INTEGER NOT NULL DEFAULT 0,
  asset_id TEXT,
  asset_revision INTEGER,
  raw_sha256 TEXT,
  texture_id TEXT,
  scale REAL,
  disabled INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS animation_states (
  target_id TEXT PRIMARY KEY,
  revision INTEGER NOT NULL DEFAULT 0,
  owner_account_id TEXT NOT NULL,
  lease_id TEXT NOT NULL,
  expires_at_unix_ms INTEGER NOT NULL,
  channel TEXT NOT NULL,
  action TEXT NOT NULL,
  animation_key TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS identities (
  identity_id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL,
  identity_kind TEXT NOT NULL,
  provider_id TEXT,
  scope_id TEXT,
  profile_uuid TEXT NOT NULL,
  display_name TEXT NOT NULL,
  verified INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS scoped_identity_bindings (
  binding_id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL,
  identity_id TEXT NOT NULL,
  target_id TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  world_epoch TEXT NOT NULL,
  entity_uuid TEXT NOT NULL,
  verification_method TEXT NOT NULL,
  status TEXT NOT NULL,
  approved_by TEXT,
  revision INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS catalog_events (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  tenant_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  revision INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS outbox_events (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id TEXT NOT NULL UNIQUE,
  tenant_id TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  target_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_scope_acl_account ON scope_acl(account_id);
CREATE INDEX IF NOT EXISTS idx_target_acl_account ON target_acl(account_id);
CREATE INDEX IF NOT EXISTS idx_targets_scope ON targets(scope_id);
CREATE INDEX IF NOT EXISTS idx_bindings_scope ON entity_bindings(scope_id);
CREATE INDEX IF NOT EXISTS idx_identities_account ON identities(account_id);
