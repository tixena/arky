//! `${NAME}` in a store string or a plain path, replaced by the variable
//! `NAME`.

use std::env::VarError;

use os_shim::System;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::StoreError;

/// Every byte but the unreserved ones, `A-Z a-z 0-9 - . _ ~`: a value
/// encoded with it cannot change a URL's structure.
const RESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// A text with its variables replaced, and the value of each.
pub(super) struct Expanded {
    /// The text, each `${NAME}` replaced by its value.
    text: String,
    /// Each variable replaced, by name, and its value.
    values: Vec<(String, String)>,
}

impl Expanded {
    /// `error` with the store string `written` in front of the message of an
    /// `InvalidStore`, and each value in that message, as it is or
    /// percent-encoded, written back as `${NAME}`.
    pub(super) fn as_written(&self, written: &str, error: StoreError) -> StoreError {
        if let StoreError::InvalidStore(message) = error {
            let hidden = self
                .values
                .iter()
                .filter(|(_, value)| !value.is_empty())
                .fold(message, |shown, (name, value)| {
                    let variable = format!("${{{name}}}");
                    shown
                        .replace(&encoded(value), &variable)
                        .replace(value.as_str(), &variable)
                });
            StoreError::InvalidStore(format!("`{written}`: {hidden}"))
        } else {
            error
        }
    }

    /// The text, each `${NAME}` replaced by its value.
    pub(super) fn text(&self) -> &str {
        &self.text
    }
}

/// `text` with each `${NAME}` replaced by the variable `NAME` read through
/// `system`: percent-encoded in a store string, as it is in a plain path.
/// `$$` writes `$`.
///
/// An error names `text` as written and never a variable's value: a variable
/// not set or not UTF-8 is `NotConfigured`, a `${` with no closing `}` or
/// no name is `InvalidStore`.
pub(super) fn expand(
    text: &str,
    system: &dyn System,
    encode: bool,
) -> Result<Expanded, StoreError> {
    let mut expanded = String::with_capacity(text.len());
    let mut values = Vec::new();
    let mut rest = text;
    while let Some(dollar) = rest.find('$') {
        expanded.push_str(&rest[..dollar]);
        let after = &rest[dollar + 1..];
        if let Some(escaped) = after.strip_prefix('$') {
            expanded.push('$');
            rest = escaped;
        } else if let Some(braced) = after.strip_prefix('{') {
            let Some((name, tail)) = braced.split_once('}') else {
                return Err(StoreError::InvalidStore(format!(
                    "`{text}`: a ${{ with no closing }}"
                )));
            };
            if name.is_empty() {
                return Err(StoreError::InvalidStore(format!(
                    "`{text}`: a ${{}} with no name"
                )));
            }
            let value = variable(system, name, text)?;
            if encode {
                expanded.push_str(&encoded(&value));
            } else {
                expanded.push_str(&value);
            }
            values.push((name.to_owned(), value));
            rest = tail;
        } else {
            expanded.push('$');
            rest = after;
        }
    }
    expanded.push_str(rest);
    Ok(Expanded {
        text: expanded,
        values,
    })
}

/// `value` percent-encoded, every byte but the unreserved ones.
fn encoded(value: &str) -> String {
    utf8_percent_encode(value, RESERVED).to_string()
}

/// The value of the variable `name`, which the store string `text` reads; the
/// error names the variable and `text`, never the value.
fn variable(system: &dyn System, name: &str, text: &str) -> Result<String, StoreError> {
    system.env_var(name).map_err(|error| {
        let what = match error {
            VarError::NotPresent => "is not set",
            VarError::NotUnicode(_) => "is not UTF-8",
        };
        StoreError::NotConfigured(format!("{name} {what}: `{text}` reads it"))
    })
}
