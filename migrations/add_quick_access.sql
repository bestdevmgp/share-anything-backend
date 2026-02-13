-- Add is_quick_access column to file_shares table
ALTER TABLE file_shares ADD COLUMN is_quick_access BOOLEAN NOT NULL DEFAULT FALSE;

-- Add is_quick_access column to upload_sessions table
ALTER TABLE upload_sessions ADD COLUMN is_quick_access BOOLEAN NOT NULL DEFAULT FALSE;

-- Index for quick access queries by user
CREATE INDEX idx_file_shares_quick_access_user ON file_shares (user_id, is_quick_access, expires_at);