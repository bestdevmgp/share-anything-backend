ALTER TABLE users
  ADD COLUMN notify_upload BOOLEAN NOT NULL DEFAULT TRUE AFTER profile_image,
  ADD COLUMN notify_download BOOLEAN NOT NULL DEFAULT TRUE AFTER notify_upload;
