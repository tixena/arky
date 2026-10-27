//! `Value::Date`: a BSON `Date` at midnight UTC.

use core::borrow::Borrow;

use chrono::{NaiveDate, NaiveTime};
use serde::de::Error as _;
use serde::{Deserializer, Serializer};

use super::{read_date, write_date};

pub fn serialize<N, S>(date: N, serializer: S) -> Result<S::Ok, S::Error>
where
    N: Borrow<NaiveDate>,
    S: Serializer,
{
    write_date(date.borrow().and_time(NaiveTime::MIN).and_utc(), serializer)
}

/// Reads a date whose instant is midnight UTC.
pub fn deserialize<'de, D>(deserializer: D) -> Result<NaiveDate, D::Error>
where
    D: Deserializer<'de>,
{
    let at = read_date(deserializer)?;
    if at.time() == NaiveTime::MIN {
        Ok(at.date_naive())
    } else {
        Err(D::Error::custom(format!("{at} is not at midnight UTC")))
    }
}
