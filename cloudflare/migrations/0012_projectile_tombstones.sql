CREATE TABLE IF NOT EXISTS projectile_tombstones (
  scope_id TEXT NOT NULL, world_epoch TEXT NOT NULL, dimension_id TEXT NOT NULL,
  entity_uuid TEXT NOT NULL, publisher_account_id TEXT NOT NULL, event_id TEXT NOT NULL,
  PRIMARY KEY(scope_id,world_epoch,dimension_id,entity_uuid),
  UNIQUE(scope_id,world_epoch,publisher_account_id,event_id)
);
