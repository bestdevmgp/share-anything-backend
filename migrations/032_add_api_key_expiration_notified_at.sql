ALTER TABLE api_keys
    ADD COLUMN expiration_notified_at TIMESTAMP NULL AFTER revoked_at;
