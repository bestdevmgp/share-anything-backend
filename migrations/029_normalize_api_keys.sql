-- 1. Create api_keys table
CREATE TABLE api_keys (
    id VARCHAR(64) PRIMARY KEY,
    user_id VARCHAR(64) NOT NULL,
    application_id BIGINT NOT NULL,
    key_hash VARCHAR(64) NOT NULL UNIQUE,
    key_prefix VARCHAR(16) NOT NULL,
    name VARCHAR(255) NOT NULL,
    last_used_at TIMESTAMP NULL,
    last_platform VARCHAR(64) NULL,
    expires_at TIMESTAMP NULL,
    revoked_at TIMESTAMP NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_user (user_id),
    INDEX idx_hash (key_hash),
    INDEX idx_application (application_id)
);

-- 2. Create key_scopes junction table
CREATE TABLE key_scopes (
    api_key_id VARCHAR(64) NOT NULL,
    scope VARCHAR(32) NOT NULL,
    PRIMARY KEY (api_key_id, scope),
    FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE
);

-- 3. Drop sparse columns from personal_tokens
ALTER TABLE personal_tokens
    DROP COLUMN scopes,
    DROP COLUMN kind,
    DROP COLUMN application_id;
