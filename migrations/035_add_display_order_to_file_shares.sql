-- Stable per-share file ordering. Without this column, multiple files inserted
-- in the same second receive identical created_at values and the visible order
-- can drift between sender and receiver.
ALTER TABLE file_shares
  ADD COLUMN display_order INT NOT NULL DEFAULT 0;
