//! The serde hooks of the `Value` variants serde cannot handle plain: a float
//! written as text when it is not finite, the three dates as BSON `Date`s, a
//! time of day, and a mapping that refuses a document of `$` keys alone.

pub mod as_if_utc;
pub mod finite;
pub mod instant;
pub mod midnight;
pub mod plain_mapping;
pub mod time_of_day;

use chrono::{DateTime, Utc};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The serde shape `bson`'s serializer stores as a BSON `Date`: a struct named
/// `$date` holding `{ "$numberLong": "<milliseconds>" }`.
#[derive(Serialize, Deserialize)]
#[serde(rename = "$date")]
struct BsonDate {
    #[serde(rename = "$date")]
    date: NumberLong,
}

#[derive(Serialize, Deserialize)]
struct NumberLong {
    #[serde(rename = "$numberLong")]
    millis: String,
}

/// What a reader hands over for a date: `{ "$numberLong": ".." }` from JSON,
/// an RFC 3339 text from the `bson` reader.
#[derive(Deserialize)]
#[serde(untagged)]
enum DateBody {
    Long(NumberLong),
    Text(String),
}

/// `{ "$date": <body> }`, read from a document of that one key only: serde
/// reads a struct from a list too.
#[derive(Deserialize)]
enum ReadDate {
    #[serde(rename = "$date")]
    Date(DateBody),
}

/// Writes `at` as a BSON `Date`, to the millisecond.
fn write_date<S>(at: DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let date = NumberLong {
        millis: at.timestamp_millis().to_string(),
    };
    BsonDate { date }.serialize(serializer)
}

/// Reads a BSON `Date` as the instant it holds.
fn read_date<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let ReadDate::Date(body) = ReadDate::deserialize(deserializer)?;
    match body {
        DateBody::Long(long) => long
            .millis
            .parse()
            .ok()
            .and_then(DateTime::from_timestamp_millis)
            .ok_or_else(|| D::Error::custom(format!("{} is not a date", long.millis))),
        DateBody::Text(text) => DateTime::parse_from_rfc3339(&text)
            .map(|at| at.to_utc())
            .map_err(D::Error::custom),
    }
}
