ALTER TABLE identities ADD COLUMN canonical_name TEXT;
CREATE TABLE player_appearances (
  identity_id TEXT PRIMARY KEY REFERENCES identities(identity_id),
  entity_uuid TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 0,
  asset_id TEXT,
  asset_revision INTEGER,
  raw_sha256 TEXT,
  texture_id TEXT,
  updated_at INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX player_appearances_entity ON player_appearances(entity_uuid, updated_at);
