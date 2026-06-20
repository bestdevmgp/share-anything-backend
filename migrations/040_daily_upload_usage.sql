-- Daily standard-upload quota (5GB/day, Korea Standard Time).
-- Tracks bytes uploaded per identity per KST calendar day:
--   identity = "user:{user_id}" for signed-in web/CLI uploads,
--              "ip:{client_ip}" for anonymous (guest) uploads.
-- The day boundary is KST midnight (computed app-side as UTC+9 date).
-- NOT tracked here: P2P / secure transfers (nothing stored on the server)
-- and OpenAPI (API-key) uploads (which have their own per-key quota).
-- Online-safe additive table; fully backward compatible.
CREATE TABLE IF NOT EXISTS daily_upload_usage (
  identity     VARCHAR(255) NOT NULL,
  usage_date   DATE         NOT NULL,
  bytes_used   BIGINT       NOT NULL DEFAULT 0,
  PRIMARY KEY (identity, usage_date)
);
