-- Folder upload (phase 1): preserve the per-file POSIX relative path within an
-- uploaded folder, e.g. "src/index.ts". NULL = root-level file (existing flat
-- behavior). file_name keeps holding the leaf name only; relative_path is stored
-- separately so existing display/sanitization that relies on file_name is unaffected.
-- Online-safe additive column; fully backward compatible.
ALTER TABLE file_shares
  ADD COLUMN relative_path VARCHAR(1024) NULL;
