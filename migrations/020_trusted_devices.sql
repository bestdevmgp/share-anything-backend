DROP TABLE blocked_devices;

CREATE TABLE trusted_devices (
    id CHAR(36) NOT NULL PRIMARY KEY,
    user_id VARCHAR(36) NOT NULL,
    user_agent_hash CHAR(64) NOT NULL,
    user_agent TEXT,
    ip_address VARCHAR(64) NOT NULL,
    device_label VARCHAR(255),
    trusted_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uniq_device (user_id, user_agent_hash, ip_address),
    INDEX idx_user (user_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
