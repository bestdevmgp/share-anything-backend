-- Add share_group_id to support multiple files sharing the same code
ALTER TABLE file_shares
ADD COLUMN share_group_id CHAR(36) AFTER id,
ADD INDEX idx_share_group_id (share_group_id);

-- Update share_code to be non-unique (multiple files can share the same code)
ALTER TABLE file_shares
DROP INDEX share_code;

-- Add composite index for share_code and share_group_id
ALTER TABLE file_shares
ADD INDEX idx_share_code_group (share_code, share_group_id);
