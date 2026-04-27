CREATE TABLE sessions (
    jti CHAR(36) NOT NULL PRIMARY KEY,
    user_id VARCHAR(36) NOT NULL,
    device_label VARCHAR(255),
    user_agent TEXT,
    user_agent_hash CHAR(64) NOT NULL,
    ip_address VARCHAR(64) NOT NULL,
    location VARCHAR(255),
    last_seen_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP NOT NULL,
    INDEX idx_user (user_id),
    INDEX idx_expires (expires_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE blocked_devices (
    id CHAR(36) NOT NULL PRIMARY KEY,
    user_id VARCHAR(36) NOT NULL,
    user_agent_hash CHAR(64) NOT NULL,
    user_agent TEXT,
    ip_address VARCHAR(64) NOT NULL,
    device_label VARCHAR(255),
    blocked_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY unique_device (user_id, user_agent_hash, ip_address),
    INDEX idx_user (user_id),
    INDEX idx_lookup (user_id, user_agent_hash, ip_address)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
