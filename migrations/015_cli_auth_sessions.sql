CREATE TABLE IF NOT EXISTS cli_auth_sessions (
    id CHAR(36) PRIMARY KEY,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    user_id CHAR(36),
    personal_token_id CHAR(36),
    personal_token_value VARCHAR(255),
    expires_at DATETIME NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME,
    INDEX idx_cli_auth_sessions_expires (expires_at)
);
