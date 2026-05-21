ALTER TABLE personal_tokens
    ADD COLUMN scopes VARCHAR(64) NOT NULL DEFAULT 'read,upload,delete' AFTER name;

UPDATE personal_tokens
SET scopes = 'read,upload,delete'
WHERE scopes IS NULL OR scopes = '';
