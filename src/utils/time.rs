use chrono::{DateTime, Utc};
use chrono_tz::Asia::Seoul;

/// Get current time in KST (Korea Standard Time, UTC+9)
/// Returns DateTime<Utc> but with KST time values for DB storage
pub fn now_kst() -> DateTime<Utc> {
    // Get current UTC time
    let utc_now = Utc::now();

    // Convert to KST
    let kst_now = utc_now.with_timezone(&Seoul);

    // Convert back to DateTime<Utc> with KST values
    // This allows storing KST time in DB while keeping the type compatible
    kst_now.naive_local().and_utc()
}
