-- Add is_one_time flag to support one-time download
ALTER TABLE file_shares
ADD COLUMN is_one_time BOOLEAN NOT NULL DEFAULT FALSE AFTER password_hash;
