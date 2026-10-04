CREATE TABLE player_motion (
  identity_id TEXT PRIMARY KEY REFERENCES identities(identity_id),
  motion_json TEXT
);
