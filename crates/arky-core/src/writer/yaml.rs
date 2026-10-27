//! Frontmatter written in YAML: mappings in block form, lists of plain
//! values in flow form, and text in double quotes only when the reader would
//! read it plain as something else or it has the form of a time of day.

use core::fmt::{self, Write as _};

use crate::parser::ast::{Mapping, Value};
use crate::parser::located::ScalarValue;
use crate::parser::scalar;
use crate::writer::{
    Scalar, Shape, non_finite, push_date_time, push_finite, push_offset_date_time, shape,
};

/// The characters that make a plain scalar starting with them something
/// else: the YAML indicators.
const INDICATORS: [char; 19] = [
    '!', '"', '#', '%', '&', '\'', '*', ',', '-', ':', '>', '?', '@', '[', ']', '`', '{', '|', '}',
];

/// The characters that end a plain scalar inside a flow collection.
const FLOW_INDICATORS: [char; 5] = [',', '[', ']', '{', '}'];

/// `mapping` between `---` lines.
pub fn frontmatter(mapping: &Mapping) -> String {
    let mut content = String::new();
    block_mapping(&mut content, mapping, 0, false);
    delimited(&content)
}

/// `content` between `---` lines.
pub fn delimited(content: &str) -> String {
    format!("---\n{content}---\n")
}

/// The pairs of `mapping`, each key at column `indent`. With `on_entry`, the
/// first key goes on the line a list entry's `-` started.
fn block_mapping(out: &mut String, mapping: &Mapping, indent: usize, on_entry: bool) {
    for (index, (key, value)) in mapping.iter().enumerate() {
        if index > 0 || !on_entry {
            push_indent(out, indent);
        }
        push_text(out, key, false);
        out.push(':');
        after_indicator(out, value, indent + 2, false);
    }
}

/// The items of `sequence`, each `-` at column `indent`. With `on_entry`,
/// the first `-` goes on the line a list entry's `-` started.
fn block_sequence(out: &mut String, sequence: &[Value], indent: usize, on_entry: bool) {
    for (index, item) in sequence.iter().enumerate() {
        if index > 0 || !on_entry {
            push_indent(out, indent);
        }
        out.push('-');
        after_indicator(out, item, indent + 2, true);
    }
}

/// `value` after a key's `:` or, `on_entry`, after a list entry's `-`: a
/// mapping, or a list that holds one, in block form at column `indent`, and
/// anything else on the same line.
fn after_indicator(out: &mut String, value: &Value, indent: usize, on_entry: bool) {
    let opening = if on_entry { ' ' } else { '\n' };
    match value {
        Value::Mapping(mapping) if !mapping.is_empty() => {
            out.push(opening);
            block_mapping(out, mapping, indent, on_entry);
        }
        Value::Sequence(sequence) if holds_mapping(sequence) => {
            out.push(opening);
            block_sequence(out, sequence, indent, on_entry);
        }
        Value::Boolean(_)
        | Value::Date(_)
        | Value::DateTime(_)
        | Value::Float(_)
        | Value::Integer(_)
        | Value::Mapping(_)
        | Value::Null(())
        | Value::OffsetDateTime(_)
        | Value::Sequence(_)
        | Value::String(_)
        | Value::Time(_) => {
            out.push(' ');
            flow(out, value, false);
            out.push('\n');
        }
    }
}

/// Whether `sequence` holds a mapping, in itself or in a list inside it.
fn holds_mapping(sequence: &[Value]) -> bool {
    sequence.iter().any(|item| {
        matches!(item, Value::Mapping(_))
            || matches!(item, Value::Sequence(inner) if holds_mapping(inner))
    })
}

/// `value` on one line, in flow form, inside a flow collection when
/// `in_flow`.
fn flow(out: &mut String, value: &Value, in_flow: bool) {
    match shape(value) {
        Shape::Mapping(mapping) => {
            out.push('{');
            for (index, (key, item)) in mapping.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                push_text(out, key, true);
                out.push_str(": ");
                flow(out, item, true);
            }
            out.push('}');
        }
        Shape::Scalar(scalar) => push_scalar(out, scalar, in_flow),
        Shape::Sequence(sequence) => {
            out.push('[');
            for (index, item) in sequence.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                flow(out, item, true);
            }
            out.push(']');
        }
    }
}

/// `scalar` on one line, inside a flow collection when `in_flow`: a date or
/// a date-time after the tag `!!timestamp`, a time of day and a float that is
/// not a finite number as quoted text, `null` for a null.
pub fn push_scalar(out: &mut String, scalar: Scalar<'_>, in_flow: bool) {
    match scalar {
        Scalar::Boolean(boolean) => out.push_str(if boolean { "true" } else { "false" }),
        Scalar::Date(date) => {
            let _: fmt::Result = write!(out, "!!timestamp {date}");
        }
        Scalar::DateTime(date_time) => {
            out.push_str("!!timestamp ");
            push_date_time(out, &date_time);
        }
        Scalar::Float(float) => match non_finite(float, [".nan", ".inf", "-.inf"]) {
            Some(text) => push_text(out, text, in_flow),
            None => push_finite(out, float),
        },
        Scalar::Integer(integer) => {
            let _: fmt::Result = write!(out, "{integer}");
        }
        Scalar::Null => out.push_str("null"),
        Scalar::OffsetDateTime(date_time) => {
            out.push_str("!!timestamp ");
            push_offset_date_time(out, &date_time);
        }
        Scalar::String(text) => push_text(out, text, in_flow),
        Scalar::Time(time) => {
            let _: fmt::Result = write!(out, "\"{time}\"");
        }
    }
}

/// `text` plain, or in double quotes with its line breaks, quotes,
/// backslashes and control characters escaped.
pub fn push_text(out: &mut String, text: &str, in_flow: bool) {
    if !needs_quotes(text, in_flow) {
        out.push_str(text);
        return;
    }
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control.is_control() => {
                let _: fmt::Result = write!(out, "\\x{:02X}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Whether `text` written plain would read as something else: another type,
/// a key, a comment, an indicator, a flow collection's end, or text without
/// the spaces around it or its control characters. Text in the form of a time
/// of day is quoted as a time of day is, so a second write keeps it. Text of
/// an integer too large for 64 bits, or of a float too large for a float, is
/// quoted too: arky reads one as text.
fn needs_quotes(text: &str, in_flow: bool) -> bool {
    text.starts_with(INDICATORS)
        || text.starts_with(' ')
        || text.ends_with([' ', ':'])
        || text.contains(": ")
        || text.contains(" #")
        || text.contains(char::is_control)
        || (in_flow && text.contains(FLOW_INDICATORS))
        || scalar::is_time(text)
        || scalar::is_oversized_integer(text)
        || scalar::is_oversized_float(text)
        || scalar::is_non_finite(text)
        || !matches!(scalar::yaml(text), ScalarValue::String(_))
}

pub fn push_indent(out: &mut String, indent: usize) {
    out.push_str(&" ".repeat(indent));
}
