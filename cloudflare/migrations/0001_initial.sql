CREATE TABLE IF NOT EXISTS assets (
  asset_id TEXT PRIMARY KEY,
  owner_account_id TEXT NOT NULL,
  current_revision INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS asset_revisions (
  asset_id TEXT NOT NULL,
  revision INTEGER NOT NULL,
  name TEXT NOT NULL,
  format TEXT NOT NULL,
  raw_sha256 TEXT NOT NULL,
  byte_length INTEGER NOT NULL,
  object_key TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (asset_id, revision),
  UNIQUE (raw_sha256)
);

CREATE TABLE IF NOT EXISTS asset_acl (
  asset_id TEXT NOT NULL,
  account_id TEXT NOT NULL,
  permission TEXT NOT NULL,
  PRIMARY KEY (asset_id, account_id)
);

CREATE TABLE IF NOT EXISTS idempotency (
  account_id TEXT NOT NULL,
  request_id TEXT NOT NULL,
  request_hash TEXT NOT NULL,
  response_json TEXT NOT NULL,
  PRIMARY KEY (account_id, request_id)
);

CREATE TABLE IF NOT EXISTS catalog_events (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  tenant_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  revision INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
