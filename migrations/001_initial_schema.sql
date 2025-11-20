-- Create users table
CREATE TABLE IF NOT EXISTS users (
    id CHAR(36) PRIMARY KEY,
    oauth_provider ENUM('google', 'naver') NOT NULL,
    oauth_id VARCHAR(255) NOT NULL,
    email VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    profile_image VARCHAR(500),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY unique_oauth (oauth_provider, oauth_id),
    INDEX idx_email (email)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Create file_shares table
CREATE TABLE IF NOT EXISTS file_shares (
    id CHAR(36) PRIMARY KEY,
    user_id CHAR(36),
    share_code CHAR(6) NOT NULL UNIQUE,
    file_name VARCHAR(500) NOT NULL,
    file_size BIGINT NOT NULL,
    file_type VARCHAR(255) NOT NULL,
    storage_key VARCHAR(500) NOT NULL,
    description TEXT,
    password_hash VARCHAR(255),
    expires_at DATETIME NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL,
    INDEX idx_share_code (share_code),
    INDEX idx_user_id (user_id),
    INDEX idx_expires_at (expires_at),
    INDEX idx_created_at (created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Create download_logs table
CREATE TABLE IF NOT EXISTS download_logs (
    id CHAR(36) PRIMARY KEY,
    file_share_id CHAR(36) NOT NULL,
    downloader_user_id CHAR(36),
    ip_address VARCHAR(45) NOT NULL,
    user_agent TEXT,
    device_platform VARCHAR(255),
    downloaded_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (file_share_id) REFERENCES file_shares(id) ON DELETE CASCADE,
    FOREIGN KEY (downloader_user_id) REFERENCES users(id) ON DELETE SET NULL,
    INDEX idx_file_share_id (file_share_id),
    INDEX idx_downloaded_at (downloaded_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
