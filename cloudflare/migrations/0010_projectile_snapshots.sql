CREATE TABLE IF NOT EXISTS projectile_snapshots (
  scope_id TEXT NOT NULL, world_epoch TEXT NOT NULL, dimension_id TEXT NOT NULL,
  entity_uuid TEXT NOT NULL, publisher_account_id TEXT NOT NULL, source_identity_id TEXT NOT NULL,
  event_id TEXT NOT NULL, snapshot_json TEXT NOT NULL,
  lease_revision INTEGER NOT NULL DEFAULT 1, received_at_ms INTEGER NOT NULL,
  expires_at_ms INTEGER NOT NULL, absolute_expires_at_ms INTEGER NOT NULL,
  withdrawn INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(scope_id,world_epoch,dimension_id,entity_uuid),
  UNIQUE(scope_id,world_epoch,publisher_account_id,event_id)
);
CREATE INDEX IF NOT EXISTS projectile_snapshots_publisher ON projectile_snapshots(publisher_account_id,expires_at_ms);
CREATE INDEX IF NOT EXISTS projectile_snapshots_expiry ON projectile_snapshots(absolute_expires_at_ms);
CREATE TABLE IF NOT EXISTS visual_publish_buckets(account_id TEXT PRIMARY KEY, tokens REAL NOT NULL, updated_at_ms INTEGER NOT NULL);
