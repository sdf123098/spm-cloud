-- Revisions and distinct asset IDs may reuse the same content-addressed R2 object.
-- Preserve every revision field and the composite primary key while removing global SHA uniqueness.
CREATE TABLE asset_revisions_v6 (
  asset_id TEXT NOT NULL,
  revision INTEGER NOT NULL,
  name TEXT NOT NULL,
  format TEXT NOT NULL,
  raw_sha256 TEXT NOT NULL,
  byte_length INTEGER NOT NULL,
  object_key TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (asset_id, revision)
);

INSERT INTO asset_revisions_v6(asset_id, revision, name, format, raw_sha256, byte_length, object_key, created_at)
SELECT asset_id, revision, name, format, raw_sha256, byte_length, object_key, created_at FROM asset_revisions;

DROP TABLE asset_revisions;
ALTER TABLE asset_revisions_v6 RENAME TO asset_revisions;
CREATE INDEX idx_asset_revisions_sha ON asset_revisions(raw_sha256);
