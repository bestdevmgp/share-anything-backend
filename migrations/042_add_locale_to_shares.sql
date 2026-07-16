-- Uploader UI locale for Open Graph link previews.
-- The link-preview crawler (KakaoTalk, Slack, X, ...) fetches OG metadata, not
-- the viewer, so the viewer's language cannot be detected. Instead we persist
-- the uploader's UI language at upload time and render the preview in it, with
-- English as the fallback. NULL = unknown (older shares / CLI / API uploads) →
-- the OG handler falls back to the request's Accept-Language, then English.
-- Online-safe additive columns; fully backward compatible.
ALTER TABLE file_shares
  ADD COLUMN locale VARCHAR(16) NULL;

ALTER TABLE upload_sessions
  ADD COLUMN locale VARCHAR(16) NULL;
