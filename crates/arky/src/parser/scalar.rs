//! Typed scalars from their text: numbers, dates and times.

use core::str::FromStr;

use chrono::{FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};

use crate::parser::located::ScalarValue;

const TOML_RADIXES: &[(&str, u32)] = &[("0x", 16), ("0o", 8), ("0b", 2)];
const YAML_RADIXES: &[(&str, u32)] = &[("0x", 16), ("0o", 8)];

/// The YAML floats that are not finite numbers, which arky reads as text.
const YAML_NON_FINITE: [&str; 12] = [
    ".inf", ".Inf", ".INF", "+.inf", "+.Inf", "+.INF", "-.inf", "-.Inf", "-.INF", ".nan", ".NaN",
    ".NAN",
];

/// The value of an unquoted TOML token, or `None` when it is not one. An
/// integer that 64 bits cannot hold, a float that a float cannot hold, and
/// `inf` and `nan` with an optional sign are their text as written.
pub fn toml(text: &str) -> Option<ScalarValue> {
    match text {
        "true" => Some(ScalarValue::Boolean(true)),
        "false" => Some(ScalarValue::Boolean(false)),
        "inf" | "+inf" | "-inf" | "nan" | "+nan" | "-nan" => {
            Some(ScalarValue::String(text.to_owned()))
        }
        _ => {
            let number = is_toml_number(text).then(|| text.replace('_', ""));
            date_time(text)
                .or_else(|| date(text).map(ScalarValue::Date))
                .or_else(|| time(text).map(ScalarValue::Time))
                .or_else(|| integer(number.as_deref()?, TOML_RADIXES).map(ScalarValue::Integer))
                .or_else(|| {
                    is_oversized_toml_integer(number.as_deref()?)
                        .then(|| ScalarValue::String(text.to_owned()))
                })
                .or_else(|| float(number.as_deref()?).map(|value| finite_or_text(value, text)))
        }
    }
}

/// The value of a plain YAML scalar: whatever is not a null, a boolean, a
/// number or a date is a string, and so are digits too large for a float and
/// the floats that are not finite numbers, such as `.inf`.
pub fn yaml(text: &str) -> ScalarValue {
    match text {
        "" | "~" | "null" | "Null" | "NULL" => ScalarValue::Null,
        "true" | "True" | "TRUE" => ScalarValue::Boolean(true),
        "false" | "False" | "FALSE" => ScalarValue::Boolean(false),
        _ if is_non_finite(text) => ScalarValue::String(text.to_owned()),
        _ => integer(text, YAML_RADIXES)
            .map(ScalarValue::Integer)
            .or_else(|| yaml_float(text).map(|value| finite_or_text(value, text)))
            .or_else(|| date_time(text))
            .or_else(|| date(text).map(ScalarValue::Date))
            .unwrap_or_else(|| ScalarValue::String(text.to_owned())),
    }
}

/// Whether `text` is written as a YAML integer, decimal with an optional sign
/// or `0o` or `0x` and digits in that base, that 64 bits cannot hold.
pub fn is_oversized_integer(text: &str) -> bool {
    is_oversized(text, YAML_RADIXES)
}

/// Whether `text` is written as a YAML float that a float cannot hold, such
/// as `1e400`.
pub fn is_oversized_float(text: &str) -> bool {
    yaml_float(text).is_some_and(f64::is_infinite)
}

/// Whether `text` is a YAML float that is not a finite number, such as `.inf`.
pub fn is_non_finite(text: &str) -> bool {
    YAML_NON_FINITE.contains(&text)
}

/// [`is_oversized_integer`] for a TOML number with its underscores dropped,
/// which may also be `0b` and binary digits.
fn is_oversized_toml_integer(number: &str) -> bool {
    is_oversized(number, TOML_RADIXES)
}

/// Whether `text` is decimal digits with an optional sign, or a prefix of
/// `radixes` and digits in its base, that 64 bits cannot hold.
fn is_oversized(text: &str, radixes: &[(&str, u32)]) -> bool {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (digits, radix) = radixes
        .iter()
        .find_map(|&(prefix, radix)| Some((text.strip_prefix(prefix)?, radix)))
        .unwrap_or((unsigned, 10));
    !digits.is_empty()
        && digits.chars().all(|digit| digit.is_digit(radix))
        && integer(text, radixes).is_none()
}

/// Decimal digits with an optional sign, of any size, read as a float, or as
/// their text when a float cannot hold them.
pub fn decimal_float(text: &str) -> Option<ScalarValue> {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if !is_digits(unsigned) {
        return None;
    }
    text.parse().ok().map(|value| finite_or_text(value, text))
}

pub fn is_date(text: &str) -> bool {
    date(text).is_some()
}

/// A date such as `2026-10-10`, when that day exists in the calendar.
fn date(text: &str) -> Option<NaiveDate> {
    let [_, _, _, _, b'-', _, _, b'-', _, _] = text.as_bytes() else {
        return None;
    };
    if !text.is_ascii() {
        return None;
    }
    NaiveDate::from_ymd_opt(
        digits(&text[0..4])?,
        digits(&text[5..7])?,
        digits(&text[8..10])?,
    )
}

pub fn is_time(text: &str) -> bool {
    time(text).is_some()
}

/// A time of day such as `07:32:00`, with an optional fraction. Second 60 is
/// a leap second, which chrono holds as second 59 plus one more second of
/// nanoseconds.
fn time(text: &str) -> Option<NaiveTime> {
    let (clock, fraction) = text
        .split_once('.')
        .map_or((text, None), |(clock, fraction)| (clock, Some(fraction)));
    let [_, _, b':', _, _, b':', _, _] = clock.as_bytes() else {
        return None;
    };
    if !clock.is_ascii() {
        return None;
    }
    let fraction_nanoseconds = fraction.map_or(Some(0), nanoseconds)?;
    let (second, nanosecond) = match digits(&clock[6..8])? {
        60 => (59, fraction_nanoseconds + 1_000_000_000),
        written => (written, fraction_nanoseconds),
    };
    NaiveTime::from_hms_nano_opt(
        digits(&clock[0..2])?,
        digits(&clock[3..5])?,
        second,
        nanosecond,
    )
}

fn nanoseconds(fraction: &str) -> Option<u32> {
    if !is_digits(fraction) {
        return None;
    }
    let mut padded = fraction[..fraction.len().min(9)].to_owned();
    while padded.len() < 9 {
        padded.push('0');
    }
    padded.parse().ok()
}

/// A date, then `T`, `t` or a space, then a time: an `OffsetDateTime` when an
/// offset follows, a `DateTime` when none does.
fn date_time(text: &str) -> Option<ScalarValue> {
    if !text.is_ascii() || text.len() < 19 || !matches!(text.as_bytes()[10], b'T' | b't' | b' ') {
        return None;
    }
    let (clock, minutes) = split_offset(&text[11..])?;
    let local = NaiveDateTime::new(date(&text[..10])?, time(clock)?);
    let Some(east) = minutes else {
        return Some(ScalarValue::DateTime(local));
    };
    let offset = FixedOffset::east_opt(i32::from(east) * 60)?;
    local
        .and_local_timezone(offset)
        .single()
        .map(ScalarValue::OffsetDateTime)
}

/// Splits `07:32:00-04:00` into the time and its offset in minutes.
fn split_offset(rest: &str) -> Option<(&str, Option<i16>)> {
    if let Some(clock) = rest.strip_suffix(['Z', 'z']) {
        return Some((clock, Some(0)));
    }
    rest.rfind(['+', '-']).map_or(Some((rest, None)), |at| {
        Some((&rest[..at], Some(offset(&rest[at..])?)))
    })
}

fn offset(text: &str) -> Option<i16> {
    let [sign, _, _, b':', _, _] = text.as_bytes() else {
        return None;
    };
    let hours: i16 = digits(&text[1..3])?;
    let minutes: i16 = digits(&text[4..6])?;
    let total = hours * 60 + minutes;
    Some(if *sign == b'-' { -total } else { total })
}

/// Whether `text` is shaped as TOML writes a number: each underscore stands
/// between two digits of one part, and the whole part of a decimal integer or
/// a float is `0` or starts with a digit from 1 to 9.
fn is_toml_number(text: &str) -> bool {
    if let Some(digits) = TOML_RADIXES
        .iter()
        .find_map(|&(prefix, _)| text.strip_prefix(prefix))
    {
        return underscores_between(digits, u8::is_ascii_hexdigit);
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let whole = unsigned
        .find(['.', 'e', 'E'])
        .map_or(unsigned, |at| &unsigned[..at]);
    (whole == "0" || !whole.starts_with('0')) && underscores_between(text, u8::is_ascii_digit)
}

/// Whether each underscore of `text` stands between two bytes `is_digit`
/// accepts.
fn underscores_between(text: &str, is_digit: fn(&u8) -> bool) -> bool {
    let runs = text.split('_');
    runs.clone().zip(runs.skip(1)).all(|(before, after)| {
        before.as_bytes().last().is_some_and(is_digit)
            && after.as_bytes().first().is_some_and(is_digit)
    })
}

fn integer(text: &str, radixes: &[(&str, u32)]) -> Option<i64> {
    for &(prefix, radix) in radixes {
        if let Some(rest) = text.strip_prefix(prefix) {
            if !rest.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
                return None;
            }
            return i64::from_str_radix(rest, radix).ok();
        }
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if !is_digits(unsigned) {
        return None;
    }
    text.parse().ok()
}

/// A float with digits on each side of its dot, as TOML writes it.
fn float(text: &str) -> Option<f64> {
    let (whole, fraction) = float_parts(text)?;
    if !is_digits(whole) || !fraction.is_none_or(is_digits) {
        return None;
    }
    text.parse().ok()
}

/// A float by the YAML core schema: the digits before its dot or the ones
/// after it may be missing, but not both.
fn yaml_float(text: &str) -> Option<f64> {
    let (whole, fraction) = float_parts(text)?;
    let digits_around_dot = if whole.is_empty() {
        fraction.is_some_and(is_digits)
    } else {
        is_digits(whole) && fraction.is_none_or(|digits| digits.is_empty() || is_digits(digits))
    };
    if !digits_around_dot {
        return None;
    }
    text.parse().ok()
}

/// The text before a float's dot and the text after it, once its sign and a
/// well-formed exponent are dropped; `None` when it has neither a dot nor an
/// exponent.
fn float_parts(text: &str) -> Option<(&str, Option<&str>)> {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (mantissa, exponent) = unsigned
        .split_once(['e', 'E'])
        .map_or((unsigned, None), |(mantissa, exponent)| {
            (mantissa, Some(exponent))
        });
    let exponent_digits = exponent.map(|part| part.strip_prefix(['+', '-']).unwrap_or(part));
    if !exponent_digits.is_none_or(is_digits) {
        return None;
    }
    let (whole, fraction) = mantissa
        .split_once('.')
        .map_or((mantissa, None), |(whole, fraction)| {
            (whole, Some(fraction))
        });
    if fraction.is_none() && exponent.is_none() {
        return None;
    }
    Some((whole, fraction))
}

/// A `Float`, or the `String` of `text` when its digits are too large for a
/// float to hold.
fn finite_or_text(float: f64, text: &str) -> ScalarValue {
    if float.is_finite() {
        ScalarValue::Float(float)
    } else {
        ScalarValue::String(text.to_owned())
    }
}

fn digits<T>(text: &str) -> Option<T>
where
    T: FromStr,
{
    if !is_digits(text) {
        return None;
    }
    text.parse().ok()
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}
