ALTER TABLE users
  ADD COLUMN notify_download_alert BOOLEAN NOT NULL DEFAULT TRUE AFTER notify_download;
