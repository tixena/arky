//! `Value::Float`: the `f64`, or the text `nan`, `inf` or `-inf` when it is not
//! finite; read back only when it is finite.

use core::borrow::Borrow;

use serde::de::Error as _;
use serde::{Deserialize as _, Deserializer, Serializer};

use crate::writer::non_finite;

pub fn serialize<F, S>(float: F, serializer: S) -> Result<S::Ok, S::Error>
where
    F: Borrow<f64>,
    S: Serializer,
{
    let number = *float.borrow();
    match non_finite(number, ["nan", "inf", "-inf"]) {
        Some(text) => serializer.serialize_str(text),
        None => serializer.serialize_f64(number),
    }
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let float = f64::deserialize(deserializer)?;
    if float.is_finite() {
        Ok(float)
    } else {
        Err(D::Error::custom("a float that is not finite"))
    }
}
