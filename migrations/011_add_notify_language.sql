ALTER TABLE users
  ADD COLUMN notify_language VARCHAR(5) NOT NULL DEFAULT 'ko' AFTER notify_download_alert;
