ALTER TABLE users ADD COLUMN status ENUM('active', 'deactivated', 'deleted') NOT NULL DEFAULT 'active';
ALTER TABLE users ADD INDEX idx_user_status (status);
