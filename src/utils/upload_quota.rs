use axum::http::HeaderMap;
use chrono::{FixedOffset, NaiveDate, Utc};

use crate::db::{repository, DbPool};
use crate::models::error::{daily_quota_exceeded, AppError};

// Sole gate for standard uploads (web + CLI); P2P and OpenAPI are exempt.
pub const DAILY_LIMIT_AUTHED: i64 = 1024 * 1024 * 1024 * 1024; // 1 TB
pub const DAILY_LIMIT_GUEST: i64 = 10 * 1024 * 1024 * 1024; // 10 GB

pub fn daily_limit_for(user_id: Option<&str>) -> i64 {
    if user_id.is_some() {
        DAILY_LIMIT_AUTHED
    } else {
        DAILY_LIMIT_GUEST
    }
}

pub const DAILY_QUOTA_MESSAGE: &str =
    "Daily transfer limit reached. It resets at midnight (KST).";

pub fn kst_today() -> NaiveDate {
    let kst = FixedOffset::east_opt(9 * 3600).expect("valid KST offset");
    Utc::now().with_timezone(&kst).date_naive()
}

pub fn next_kst_reset() -> chrono::DateTime<Utc> {
    use chrono::TimeZone;
    let kst = FixedOffset::east_opt(9 * 3600).expect("valid KST offset");
    let tomorrow_kst = Utc::now()
        .with_timezone(&kst)
        .date_naive()
        .succ_opt()
        .expect("valid next day");
    let midnight = tomorrow_kst
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight");
    kst.from_local_datetime(&midnight)
        .single()
        .expect("unambiguous fixed-offset midnight")
        .with_timezone(&Utc)
}

pub fn quota_identity(user_id: Option<&str>, headers: &HeaderMap) -> String {
    match user_id {
        Some(uid) => format!("user:{}", uid),
        None => format!("ip:{}", crate::utils::client_ip(headers)),
    }
}

pub async fn enforce_daily_quota(
    db: &DbPool,
    user_id: Option<&str>,
    headers: &HeaderMap,
    session_bytes: i64,
) -> Result<(), AppError> {
    let identity = quota_identity(user_id, headers);
    let used = repository::get_daily_upload_usage(db, &identity, kst_today()).await?;
    if used + session_bytes > daily_limit_for(user_id) {
        return Err(daily_quota_exceeded(DAILY_QUOTA_MESSAGE));
    }
    Ok(())
}

// Best-effort: a bookkeeping failure must not fail an already-finished upload.
pub async fn record_daily_usage(
    db: &DbPool,
    user_id: Option<&str>,
    headers: &HeaderMap,
    bytes: i64,
) {
    if bytes <= 0 {
        return;
    }
    let identity = quota_identity(user_id, headers);
    let _ = repository::add_daily_upload_usage(db, &identity, kst_today(), bytes).await;
}
