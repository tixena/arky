//! `Value::Mapping`: any mapping but a document of `$` keys alone, which is how
//! the `bson` reader hands over a BSON value of a type arky does not write.

use serde::Deserializer;
use serde::de::Error as _;

use super::super::{Mapping, grown};

/// Reads a mapping, and refuses one whose every key starts with `$`.
pub fn deserialize<'de, D>(deserializer: D) -> Result<Mapping, D::Error>
where
    D: Deserializer<'de>,
{
    let mapping: Mapping = grown(deserializer)?;
    if !mapping.is_empty() && mapping.keys().all(|key| key.starts_with('$')) {
        return Err(D::Error::custom(
            "a document of `$` keys alone is a BSON value arky does not write",
        ));
    }
    Ok(mapping)
}
