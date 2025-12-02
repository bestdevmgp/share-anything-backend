use chrono::{DateTime, Utc, FixedOffset, TimeZone};
use chrono_tz::Asia::Seoul;
use serde::Serializer;

pub fn now_kst() -> DateTime<Utc> {
    let utc_now = Utc::now();
    let kst_now = utc_now.with_timezone(&Seoul);
    kst_now.naive_local().and_utc()
}

pub fn serialize_as_kst<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let kst = FixedOffset::east_opt(9 * 3600).unwrap();
    let naive = dt.naive_local();
    let kst_time = kst.from_local_datetime(&naive).single().unwrap();
    serializer.serialize_str(&kst_time.to_rfc3339())
}
