ALTER TABLE users
  ADD COLUMN notify_security BOOLEAN NOT NULL DEFAULT TRUE AFTER notify_download_alert;
