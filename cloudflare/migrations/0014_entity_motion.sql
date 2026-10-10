CREATE TABLE IF NOT EXISTS entity_motion_states (
    scope_id TEXT NOT NULL, world_epoch TEXT NOT NULL, dimension_id TEXT NOT NULL, entity_uuid TEXT NOT NULL,
    publisher_account_id TEXT NOT NULL, update_json TEXT, resource_json TEXT,
    event_id TEXT NOT NULL, started_at_ms INTEGER NOT NULL,
    revision INTEGER NOT NULL, expires_at_ms INTEGER NOT NULL,
    PRIMARY KEY(scope_id, world_epoch, dimension_id, entity_uuid)
);
