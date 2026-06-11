ALTER TABLE file_shares
  ADD COLUMN device_id VARCHAR(64) NULL AFTER user_id;
