-- Per-identity daily upload usage, by KST calendar day.
-- identity = "user:{id}" (signed-in) or "ip:{addr}" (guest).
CREATE TABLE IF NOT EXISTS daily_upload_usage (
  identity     VARCHAR(255) NOT NULL,
  usage_date   DATE         NOT NULL,
  bytes_used   BIGINT       NOT NULL DEFAULT 0,
  PRIMARY KEY (identity, usage_date)
);
