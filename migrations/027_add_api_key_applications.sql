CREATE TABLE api_key_applications (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    user_id VARCHAR(64) NOT NULL,
    service_name VARCHAR(255) NOT NULL,
    service_url VARCHAR(512) NOT NULL,
    purpose TEXT NOT NULL,
    status ENUM('pending', 'approved', 'rejected') NOT NULL DEFAULT 'pending',
    reject_reason TEXT NULL,
    api_key_id VARCHAR(64) NULL,
    applicant_ip VARCHAR(45) NULL,
    applicant_platform VARCHAR(128) NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    reviewed_at TIMESTAMP NULL,
    INDEX idx_user (user_id),
    INDEX idx_status (status)
);

ALTER TABLE personal_tokens
    ADD COLUMN kind ENUM('pat', 'api_key') NOT NULL DEFAULT 'pat' AFTER name,
    ADD COLUMN application_id BIGINT NULL AFTER kind;
