-- Upload sessions table for presigned upload flow
CREATE TABLE IF NOT EXISTS upload_sessions (
    id CHAR(36) PRIMARY KEY,
    share_code CHAR(6) NOT NULL,
    user_id CHAR(36) NULL,
    description TEXT NULL,
    password_hash VARCHAR(255) NULL,
    is_one_time BOOLEAN NOT NULL DEFAULT FALSE,
    expires_at DATETIME NOT NULL,
    completed BOOLEAN NOT NULL DEFAULT FALSE,
    created_at DATETIME NOT NULL,
    INDEX idx_upload_sessions_share_code (share_code),
    INDEX idx_upload_sessions_created_at (created_at)
);