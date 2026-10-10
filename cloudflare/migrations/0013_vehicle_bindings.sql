CREATE TABLE IF NOT EXISTS vehicle_bindings (
  scope_id TEXT NOT NULL, world_epoch TEXT NOT NULL, dimension_id TEXT NOT NULL,
  entity_uuid TEXT NOT NULL, target_id TEXT NOT NULL, publisher_account_id TEXT NOT NULL,
  binding_json TEXT NOT NULL, revision INTEGER NOT NULL,
  PRIMARY KEY(scope_id,world_epoch,dimension_id,entity_uuid)
);
