CREATE TABLE IF NOT EXISTS entity_model_shares (
  target_id TEXT PRIMARY KEY,
  shared_by_account_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  asset_revision INTEGER NOT NULL,
  raw_sha256 TEXT NOT NULL
);
