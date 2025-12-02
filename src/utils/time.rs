use chrono::{DateTime, Utc, FixedOffset};
use serde::Serializer;

pub fn serialize_as_kst<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let kst = FixedOffset::east_opt(9 * 3600).unwrap();
    let kst_time = dt.with_timezone(&kst);
    serializer.serialize_str(&kst_time.to_rfc3339())
}
