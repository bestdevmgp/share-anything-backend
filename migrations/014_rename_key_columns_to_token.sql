ALTER TABLE personal_tokens
    RENAME COLUMN key_hash TO token_hash,
    RENAME COLUMN key_prefix TO token_prefix;
