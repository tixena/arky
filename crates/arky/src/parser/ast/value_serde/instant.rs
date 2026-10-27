//! `Value::OffsetDateTime`: a BSON `Date` of the instant, in UTC.

use chrono::{DateTime, FixedOffset};
use serde::{Deserializer, Serializer};

use super::{read_date, write_date};

pub fn serialize<S>(at: &DateTime<FixedOffset>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    write_date(at.to_utc(), serializer)
}

/// Reads any date, at offset `+00:00`.
pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<FixedOffset>, D::Error>
where
    D: Deserializer<'de>,
{
    read_date(deserializer).map(|at| at.fixed_offset())
}
