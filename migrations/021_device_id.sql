ALTER TABLE trusted_devices
    ADD COLUMN device_id CHAR(36) NOT NULL DEFAULT '' AFTER user_id,
    ADD COLUMN location VARCHAR(255) NULL AFTER device_label;

UPDATE trusted_devices SET device_id = user_agent_hash WHERE device_id = '';

ALTER TABLE trusted_devices
    DROP INDEX uniq_device,
    ADD UNIQUE KEY uniq_device (user_id, device_id);

ALTER TABLE sessions
    ADD COLUMN device_id CHAR(36) NOT NULL DEFAULT '' AFTER user_id;

UPDATE sessions SET device_id = user_agent_hash WHERE device_id = '';

ALTER TABLE sessions
    ADD INDEX idx_device (user_id, device_id);
