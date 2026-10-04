CREATE TABLE IF NOT EXISTS identity_providers (
  provider_id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  session_path TEXT NOT NULL DEFAULT '/sessionserver/session/minecraft/hasJoined',
  enabled INTEGER NOT NULL DEFAULT 1
);

INSERT OR IGNORE INTO identity_providers(provider_id, display_name, base_url, session_path, enabled)
VALUES
  ('official', 'Minecraft official', 'https://sessionserver.mojang.com', '/session/minecraft/hasJoined', 1),
  ('littleskin', 'LittleSkin', 'https://littleskin.cn/api/yggdrasil', '/sessionserver/session/minecraft/hasJoined', 1),
  ('elyby', 'Ely.by', 'https://authserver.ely.by', '/session/hasJoined', 1),
  ('drasl_unmojang', 'Drasl (unmojang.org)', 'https://drasl.unmojang.org', '/session/minecraft/hasJoined', 1);

CREATE TABLE IF NOT EXISTS identity_challenges (
  challenge_hash TEXT PRIMARY KEY,
  purpose TEXT NOT NULL CHECK (purpose IN ('link', 'login')),
  account_id TEXT,
  provider_id TEXT NOT NULL,
  username TEXT NOT NULL,
  profile_uuid TEXT NOT NULL,
  server_id TEXT NOT NULL,
  requester_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  consumed INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_identity_challenges_rate
  ON identity_challenges(requester_hash, created_at);

CREATE UNIQUE INDEX IF NOT EXISTS idx_verified_game_identity
  ON identities(identity_kind, COALESCE(provider_id, ''), profile_uuid)
  WHERE verified = 1 AND identity_kind IN ('official', 'yggdrasil');
