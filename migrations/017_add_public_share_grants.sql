-- Public share grants: temporary public access tokens pointing to an existing file_shares row.
-- The grant only holds metadata; it does NOT own the storage object. R2 object lifetime is
-- governed by the referenced file_shares row's expires_at (extended implicitly by
-- cleanup's NOT EXISTS check against active grants).

CREATE TABLE IF NOT EXISTS public_share_grants (
    share_code CHAR(6) NOT NULL PRIMARY KEY,
    file_share_id CHAR(36) NOT NULL,
    expires_at DATETIME NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (file_share_id) REFERENCES file_shares(id) ON DELETE CASCADE,
    INDEX idx_grants_file_share_id (file_share_id),
    INDEX idx_grants_expires_at (expires_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
