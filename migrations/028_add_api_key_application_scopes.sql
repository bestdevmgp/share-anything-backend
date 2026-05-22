ALTER TABLE api_key_applications
    ADD COLUMN scopes VARCHAR(64) NOT NULL DEFAULT 'read,upload,delete' AFTER purpose;
