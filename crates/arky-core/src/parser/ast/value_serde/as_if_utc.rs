//! `Value::DateTime`: a BSON `Date` of the local date-time taken as UTC.

use chrono::NaiveDateTime;
use serde::de::Error as _;
use serde::{Deserializer, Serializer};

use super::write_date;

pub fn serialize<S>(at: &NaiveDateTime, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    write_date(at.and_utc(), serializer)
}

/// Reads nothing: a stored date reads as a `Date` or an `OffsetDateTime`.
pub fn deserialize<'de, D>(_deserializer: D) -> Result<NaiveDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    Err(D::Error::custom(
        "a stored date reads as a date or a date-time with an offset",
    ))
}
