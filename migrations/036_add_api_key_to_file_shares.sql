ALTER TABLE file_shares
  ADD COLUMN created_via_api_key_id VARCHAR(64) NULL AFTER user_id;

CREATE INDEX idx_file_shares_api_key_active
  ON file_shares (created_via_api_key_id, expires_at);
