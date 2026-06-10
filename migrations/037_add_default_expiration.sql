-- Add default_expiration column to users table.
-- Holds the user's preferred default expiration for fast/home uploads.
-- Allowed values match the ExpirationOption enum used by the frontend:
--   'five_minutes', 'thirty_minutes', 'one_hour', 'three_hours',
--   'six_hours', 'twelve_hours', 'twenty_four_hours'.
ALTER TABLE users
  ADD COLUMN default_expiration VARCHAR(32) NOT NULL DEFAULT 'thirty_minutes'
  AFTER notify_language;
