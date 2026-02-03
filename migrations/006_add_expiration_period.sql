-- Add expiration_period column to calculate expires_at at completion time
ALTER TABLE upload_sessions ADD COLUMN expiration_period VARCHAR(50) NOT NULL DEFAULT 'five_minutes' AFTER is_one_time;