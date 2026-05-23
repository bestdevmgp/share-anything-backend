CREATE TABLE IF NOT EXISTS api_key_reveals (
    token CHAR(64) PRIMARY KEY,
    api_key_id CHAR(36) NOT NULL,
    user_id CHAR(36) NOT NULL,
    plaintext_key VARCHAR(64) DEFAULT NULL,
    expires_at DATETIME NOT NULL,
    revealed_at DATETIME DEFAULT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    INDEX idx_user_id (user_id),
    INDEX idx_expires_at (expires_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
