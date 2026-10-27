//! `Value::Time`: `{ "t": "time", "v": "07:32:00" }`.

use core::borrow::Borrow;

use chrono::NaiveTime;
use indexmap::IndexMap;
use serde::de::Error as _;
use serde::{Deserialize as _, Deserializer, Serialize, Serializer};

#[derive(Serialize)]
struct TimeOfDay {
    t: &'static str,
    v: String,
}

/// Writes the time to the millisecond, with `.250` only when it has milliseconds.
pub fn serialize<N, S>(time: N, serializer: S) -> Result<S::Ok, S::Error>
where
    N: Borrow<NaiveTime>,
    S: Serializer,
{
    let text = time.borrow().format("%H:%M:%S%.3f").to_string();
    let written = TimeOfDay {
        t: "time",
        v: text.strip_suffix(".000").unwrap_or(&text).to_owned(),
    };
    written.serialize(serializer)
}

/// Reads a document of exactly the keys `t`, equal to `time`, and `v`, a time.
pub fn deserialize<'de, D>(deserializer: D) -> Result<NaiveTime, D::Error>
where
    D: Deserializer<'de>,
{
    let document = IndexMap::<String, String>::deserialize(deserializer)?;
    let time = match (document.len(), document.get("t"), document.get("v")) {
        (2, Some(tag), Some(text)) if tag == "time" => {
            NaiveTime::parse_from_str(text, "%H:%M:%S%.f").ok()
        }
        _ => None,
    };
    time.ok_or_else(|| D::Error::custom("not a document of `t`, `time`, and `v`, a time"))
}
