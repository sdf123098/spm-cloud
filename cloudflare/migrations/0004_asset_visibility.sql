ALTER TABLE assets ADD COLUMN visibility TEXT NOT NULL DEFAULT 'PRIVATE';

CREATE INDEX IF NOT EXISTS idx_assets_visibility ON assets(visibility, asset_id);
