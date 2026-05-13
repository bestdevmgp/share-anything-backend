ALTER TABLE personal_tokens
    ADD COLUMN last_platform VARCHAR(64) NULL AFTER last_used_at;
