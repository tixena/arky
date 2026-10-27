//! Any serde type as a frontmatter [`Value`], and a `Value` as the JSON value
//! serde reads any type from.
//!
//! [`from_value`] reads a frontmatter, or any value in it, into a type of your own, and
//! [`to_value`] builds a value from one. A `&Value` converts to a `serde_json::Value` with
//! `From`, as in `serde_json::Value::from(&value)`: a date, a time and a date-time become the text chrono reads, and a float that is not
//! finite becomes `null`.
//!
//! ```rust
//! use arky::parser::ast::{RawDocument, Value};
//! use arky::parser::parse;
//! use arky::value::{from_value, to_value};
//! use chrono::NaiveDate;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, PartialEq, Deserialize, Serialize)]
//! struct Post {
//!     title: String,
//!     tags: Vec<String>,
//!     published: NaiveDate,
//! }
//!
//! let source = "---\ntitle: Bread\ntags: [baking, recipes]\npublished: 2026-10-17\n---\n";
//! let RawDocument::Valid(document) = parse(source).document else {
//!     panic!("every frontmatter entry reads");
//! };
//! let frontmatter = Value::Mapping(document.frontmatter.expect("the post has a frontmatter"));
//! let post: Post = from_value(&frontmatter)?;
//! assert_eq!(post.tags, ["baking", "recipes"]);
//! assert_eq!(post.published, NaiveDate::from_ymd_opt(2026, 10, 17).expect("a real date"));
//!
//! let Value::Mapping(written) = to_value(&post)? else {
//!     panic!("a struct is written as a mapping");
//! };
//! assert_eq!(written.get("published"), Some(&Value::String("2026-10-17".to_owned())));
//! # Ok::<(), arky::value::ValueError>(())
//! ```

#[cfg(test)]
mod tests;

use core::fmt::Display;

use serde::de::DeserializeOwned;
use serde::ser::{
    self, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, Serializer,
};

use crate::parser::ast::{Mapping, Value};

/// Why a type could not be written as a `Value`, or read from one.
///
/// It holds a message, which its `Display` gives.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
#[non_exhaustive]
pub struct ValueError {
    message: String,
}

/// Builds a mapping, from a map or a struct.
struct MappingWriter {
    key: Option<String>,
    mapping: Mapping,
}

/// Builds a sequence, from a sequence, a tuple or a tuple struct.
struct SequenceWriter {
    items: Vec<Value>,
}

/// Builds the `Value` a type writes.
struct ValueWriter;

/// Builds the mapping of one key a struct variant writes.
struct VariantMappingWriter {
    mapping: Mapping,
    variant: &'static str,
}

/// Builds the mapping of one key a tuple variant writes.
struct VariantSequenceWriter {
    items: Vec<Value>,
    variant: &'static str,
}

impl From<&Value> for serde_json::Value {
    /// The JSON value of `value`: a date, a time and a date-time as the text
    /// chrono reads, and a float that is not finite as `null`, which is how
    /// `serde_json` holds one.
    fn from(value: &Value) -> Self {
        match value {
            Value::Boolean(flag) => Self::Bool(*flag),
            Value::Date(date) => Self::String(date.format("%Y-%m-%d").to_string()),
            Value::DateTime(at) => Self::String(at.format("%Y-%m-%dT%H:%M:%S%.f").to_string()),
            Value::Float(number) => {
                serde_json::Number::from_f64(*number).map_or(Self::Null, Self::Number)
            }
            Value::Integer(number) => Self::Number((*number).into()),
            Value::Mapping(mapping) => Self::Object(
                mapping
                    .iter()
                    .map(|(key, held)| (key.clone(), Self::from(held)))
                    .collect(),
            ),
            Value::Null(()) => Self::Null,
            Value::OffsetDateTime(at) => Self::String(at.to_rfc3339()),
            Value::Sequence(items) => Self::Array(items.iter().map(Self::from).collect()),
            Value::String(text) => Self::String(text.clone()),
            Value::Time(time) => Self::String(time.format("%H:%M:%S%.f").to_string()),
        }
    }
}

impl SerializeMap for MappingWriter {
    type Error = ValueError;
    type Ok = Value;

    fn end(self) -> Result<Value, ValueError> {
        Ok(Value::Mapping(self.mapping))
    }

    fn serialize_entry<K, V>(&mut self, key: &K, value: &V) -> Result<(), ValueError>
    where
        K: ?Sized + Serialize,
        V: ?Sized + Serialize,
    {
        self.mapping
            .insert(key_text(to_value(key)?)?, to_value(value)?);
        Ok(())
    }

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), ValueError>
    where
        T: ?Sized + Serialize,
    {
        self.key = Some(key_text(to_value(key)?)?);
        Ok(())
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), ValueError>
    where
        T: ?Sized + Serialize,
    {
        let key = self.key.take().ok_or_else(|| {
            <ValueError as ser::Error>::custom("a map value written before its key")
        })?;
        self.mapping.insert(key, to_value(value)?);
        Ok(())
    }
}

impl SerializeStruct for MappingWriter {
    type Error = ValueError;
    type Ok = Value;

    fn end(self) -> Result<Value, ValueError> {
        Ok(Value::Mapping(self.mapping))
    }

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), ValueError>
    where
        T: ?Sized + Serialize,
    {
        self.mapping.insert(key.to_owned(), to_value(value)?);
        Ok(())
    }

    fn skip_field(&mut self, _key: &'static str) -> Result<(), ValueError> {
        Ok(())
    }
}

impl SerializeSeq for SequenceWriter {
    type Error = ValueError;
    type Ok = Value;

    fn end(self) -> Result<Value, ValueError> {
        Ok(Value::Sequence(self.items))
    }

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), ValueError>
    where
        T: ?Sized + Serialize,
    {
        self.items.push(to_value(value)?);
        Ok(())
    }
}

impl SerializeTuple for SequenceWriter {
    type Error = ValueError;
    type Ok = Value;

    fn end(self) -> Result<Value, ValueError> {
        Ok(Value::Sequence(self.items))
    }

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), ValueError>
    where
        T: ?Sized + Serialize,
    {
        self.items.push(to_value(value)?);
        Ok(())
    }
}

impl SerializeTupleStruct for SequenceWriter {
    type Error = ValueError;
    type Ok = Value;

    fn end(self) -> Result<Value, ValueError> {
        Ok(Value::Sequence(self.items))
    }

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), ValueError>
    where
        T: ?Sized + Serialize,
    {
        self.items.push(to_value(value)?);
        Ok(())
    }
}

impl ser::Error for ValueError {
    fn custom<T>(msg: T) -> Self
    where
        T: Display,
    {
        Self {
            message: msg.to_string(),
        }
    }
}

impl Serializer for ValueWriter {
    type Error = ValueError;
    type Ok = Value;
    type SerializeMap = MappingWriter;
    type SerializeSeq = SequenceWriter;
    type SerializeStruct = MappingWriter;
    type SerializeStructVariant = VariantMappingWriter;
    type SerializeTuple = SequenceWriter;
    type SerializeTupleStruct = SequenceWriter;
    type SerializeTupleVariant = VariantSequenceWriter;

    fn collect_map<K, V, I>(self, iter: I) -> Result<Value, ValueError>
    where
        K: Serialize,
        V: Serialize,
        I: IntoIterator<Item = (K, V)>,
    {
        let mut mapping = Mapping::new();
        for (key, value) in iter {
            mapping.insert(key_text(to_value(&key)?)?, to_value(&value)?);
        }
        Ok(Value::Mapping(mapping))
    }

    fn collect_seq<I>(self, iter: I) -> Result<Value, ValueError>
    where
        I: IntoIterator,
        I::Item: Serialize,
    {
        iter.into_iter()
            .map(|item| to_value(&item))
            .collect::<Result<_, _>>()
            .map(Value::Sequence)
    }

    fn collect_str<T>(self, value: &T) -> Result<Value, ValueError>
    where
        T: ?Sized + Display,
    {
        Ok(Value::String(value.to_string()))
    }

    fn is_human_readable(&self) -> bool {
        true
    }

    fn serialize_bool(self, v: bool) -> Result<Value, ValueError> {
        Ok(Value::Boolean(v))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Value, ValueError> {
        Ok(Value::Sequence(
            v.iter()
                .map(|byte| Value::Integer(i64::from(*byte)))
                .collect(),
        ))
    }

    fn serialize_char(self, v: char) -> Result<Value, ValueError> {
        Ok(Value::String(v.to_string()))
    }

    fn serialize_f32(self, v: f32) -> Result<Value, ValueError> {
        Ok(Value::Float(f64::from(v)))
    }

    fn serialize_f64(self, v: f64) -> Result<Value, ValueError> {
        Ok(Value::Float(v))
    }

    fn serialize_i128(self, v: i128) -> Result<Value, ValueError> {
        Ok(i64::try_from(v).map_or_else(|_| Value::String(v.to_string()), Value::Integer))
    }

    fn serialize_i16(self, v: i16) -> Result<Value, ValueError> {
        Ok(Value::Integer(i64::from(v)))
    }

    fn serialize_i32(self, v: i32) -> Result<Value, ValueError> {
        Ok(Value::Integer(i64::from(v)))
    }

    fn serialize_i64(self, v: i64) -> Result<Value, ValueError> {
        Ok(Value::Integer(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Value, ValueError> {
        Ok(Value::Integer(i64::from(v)))
    }

    fn serialize_map(self, len: Option<usize>) -> Result<MappingWriter, ValueError> {
        Ok(MappingWriter {
            key: None,
            mapping: Mapping::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value, ValueError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, ValueError>
    where
        T: ?Sized + Serialize,
    {
        Ok(keyed(variant, to_value(value)?))
    }

    fn serialize_none(self) -> Result<Value, ValueError> {
        Ok(Value::Null(()))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SequenceWriter, ValueError> {
        Ok(SequenceWriter {
            items: Vec::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_some<T>(self, value: &T) -> Result<Value, ValueError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_str(self, v: &str) -> Result<Value, ValueError> {
        Ok(Value::String(v.to_owned()))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<MappingWriter, ValueError> {
        Ok(MappingWriter {
            key: None,
            mapping: Mapping::with_capacity(len),
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<VariantMappingWriter, ValueError> {
        Ok(VariantMappingWriter {
            mapping: Mapping::with_capacity(len),
            variant,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<SequenceWriter, ValueError> {
        Ok(SequenceWriter {
            items: Vec::with_capacity(len),
        })
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SequenceWriter, ValueError> {
        Ok(SequenceWriter {
            items: Vec::with_capacity(len),
        })
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<VariantSequenceWriter, ValueError> {
        Ok(VariantSequenceWriter {
            items: Vec::with_capacity(len),
            variant,
        })
    }

    fn serialize_u128(self, v: u128) -> Result<Value, ValueError> {
        Ok(i64::try_from(v).map_or_else(|_| Value::String(v.to_string()), Value::Integer))
    }

    fn serialize_u16(self, v: u16) -> Result<Value, ValueError> {
        Ok(Value::Integer(i64::from(v)))
    }

    fn serialize_u32(self, v: u32) -> Result<Value, ValueError> {
        Ok(Value::Integer(i64::from(v)))
    }

    fn serialize_u64(self, v: u64) -> Result<Value, ValueError> {
        Ok(i64::try_from(v).map_or_else(|_| Value::String(v.to_string()), Value::Integer))
    }

    fn serialize_u8(self, v: u8) -> Result<Value, ValueError> {
        Ok(Value::Integer(i64::from(v)))
    }

    fn serialize_unit(self) -> Result<Value, ValueError> {
        Ok(Value::Null(()))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, ValueError> {
        Ok(Value::Null(()))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Value, ValueError> {
        Ok(Value::String(variant.to_owned()))
    }
}

impl SerializeStructVariant for VariantMappingWriter {
    type Error = ValueError;
    type Ok = Value;

    fn end(self) -> Result<Value, ValueError> {
        Ok(keyed(self.variant, Value::Mapping(self.mapping)))
    }

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), ValueError>
    where
        T: ?Sized + Serialize,
    {
        self.mapping.insert(key.to_owned(), to_value(value)?);
        Ok(())
    }

    fn skip_field(&mut self, _key: &'static str) -> Result<(), ValueError> {
        Ok(())
    }
}

impl SerializeTupleVariant for VariantSequenceWriter {
    type Error = ValueError;
    type Ok = Value;

    fn end(self) -> Result<Value, ValueError> {
        Ok(keyed(self.variant, Value::Sequence(self.items)))
    }

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), ValueError>
    where
        T: ?Sized + Serialize,
    {
        self.items.push(to_value(value)?);
        Ok(())
    }
}

/// Reads a `T` from `value` through its JSON value.
///
/// A date, a time and a date-time reach `T` as the text chrono reads, so a chrono field of `T`
/// reads them.
///
/// # Errors
///
/// Returns the error `T`'s `Deserialize` returns for what `value` holds.
///
/// # Examples
///
/// ```rust
/// use arky::parser::ast::Value;
/// use arky::value::from_value;
/// use chrono::NaiveDate;
///
/// let day = NaiveDate::from_ymd_opt(2026, 10, 17).expect("a real date");
/// assert_eq!(from_value::<NaiveDate>(&Value::Date(day))?, day);
/// assert_eq!(from_value::<u32>(&Value::Integer(4))?, 4);
/// assert!(from_value::<u32>(&Value::String("four".to_owned())).is_err());
/// # Ok::<(), arky::value::ValueError>(())
/// ```
pub fn from_value<T>(value: &Value) -> Result<T, ValueError>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value.into()).map_err(|error| ValueError {
        message: error.to_string(),
    })
}

/// Builds the `Value` that `value` writes through serde.
///
/// An integer outside `i64` is the `String` of its digits, and a map key is
/// its text. A chrono type writes itself as text, so it becomes a `Value::String`, not a
/// `Value::Date`. A `None` and a unit become `Value::Null`.
///
/// # Errors
///
/// Returns the error `T`'s `Serialize` returns, or one naming a map key that
/// is not text, a number or a boolean.
///
/// # Examples
///
/// ```rust
/// use arky::parser::ast::Value;
/// use arky::value::to_value;
///
/// let tags = to_value(&["baking", "recipes"])?;
/// let texts = vec![Value::String("baking".to_owned()), Value::String("recipes".to_owned())];
/// assert_eq!(tags, Value::Sequence(texts));
/// assert_eq!(to_value(&u64::MAX)?, Value::String("18446744073709551615".to_owned()));
/// # Ok::<(), arky::value::ValueError>(())
/// ```
pub fn to_value<T>(value: &T) -> Result<Value, ValueError>
where
    T: ?Sized + Serialize,
{
    value.serialize(ValueWriter)
}

/// The text a map key is written under.
fn key_text(key: Value) -> Result<String, ValueError> {
    match key {
        Value::Boolean(flag) => Ok(flag.to_string()),
        Value::Float(number) => Ok(number.to_string()),
        Value::Integer(number) => Ok(number.to_string()),
        Value::String(text) => Ok(text),
        Value::Date(_)
        | Value::DateTime(_)
        | Value::Mapping(_)
        | Value::Null(())
        | Value::OffsetDateTime(_)
        | Value::Sequence(_)
        | Value::Time(_) => Err(<ValueError as ser::Error>::custom(
            "a map key is text, a number or a boolean",
        )),
    }
}

/// The mapping of one key a variant that holds something writes.
fn keyed(variant: &str, value: Value) -> Value {
    let mut mapping = Mapping::with_capacity(1);
    mapping.insert(variant.to_owned(), value);
    Value::Mapping(mapping)
}
