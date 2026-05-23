ALTER TABLE api_key_applications
    ADD COLUMN requested_expires_at TIMESTAMP NULL AFTER scopes;
