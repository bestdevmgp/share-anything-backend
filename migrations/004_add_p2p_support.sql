-- Add P2P transfer support
ALTER TABLE file_shares
ADD COLUMN transfer_type ENUM('server', 'p2p') NOT NULL DEFAULT 'server' AFTER file_type,
ADD COLUMN p2p_status ENUM('waiting', 'connected', 'completed', 'failed') DEFAULT NULL AFTER transfer_type,
ADD COLUMN uploader_peer_id VARCHAR(255) DEFAULT NULL AFTER p2p_status,
ADD INDEX idx_transfer_type (transfer_type),
ADD INDEX idx_p2p_status (p2p_status);
