CREATE TABLE IF NOT EXISTS claim_codes (
  code_hash TEXT PRIMARY KEY,
  scope_id TEXT NOT NULL,
  world_epoch TEXT NOT NULL,
  target_id TEXT NOT NULL,
  entity_uuid TEXT NOT NULL,
  issued_by TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL DEFAULT 5,
  consumed INTEGER NOT NULL DEFAULT 0,
  revoked INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_scoped_identity_bindings_scope ON scoped_identity_bindings(scope_id);
CREATE INDEX IF NOT EXISTS idx_claim_codes_scope ON claim_codes(scope_id);
