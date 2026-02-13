-- Add 'apple' and 'kakao' to oauth_provider ENUM
ALTER TABLE users MODIFY COLUMN oauth_provider ENUM('google', 'naver', 'apple', 'kakao') NOT NULL;
