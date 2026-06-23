CREATE TABLE IF NOT EXISTS empty_folders (
    share_code    CHAR(6)       NOT NULL,
    relative_path VARCHAR(1024) NOT NULL,
    created_at    TIMESTAMP     NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (share_code, relative_path(190))
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
