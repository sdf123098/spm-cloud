CREATE TABLE IF NOT EXISTS player_display_states (
  identity_id TEXT PRIMARY KEY REFERENCES identities(identity_id) ON DELETE CASCADE,
  display_json TEXT
);
