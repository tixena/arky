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
    /// `error`, from a backend handed `handed`, naming `written`: in place of
    /// `handed` where the message starts with it, else in front of an
    /// `InvalidStore`. Each value in the message is written back as `${NAME}`.
    pub(super) fn as_written(&self, written: &str, handed: &str, error: StoreError) -> StoreError {
        let start = format!("`{handed}`");
        let named = |message: &str, otherwise: &str| {
            message.strip_prefix(&start).map_or_else(
                || format!("{otherwise}{}", self.hidden(message)),
                |rest| format!("`{written}`{}", self.hidden(rest)),
            )
        };
        match error {
            StoreError::InvalidStore(message) => {
                StoreError::InvalidStore(named(&message, &format!("`{written}`: ")))
            }
            StoreError::NotConfigured(message) => StoreError::NotConfigured(named(&message, "")),
            StoreError::Backend(_)
            | StoreError::Conflict(_)
            | StoreError::InvalidPath(_)
            | StoreError::NotFound(_)
            | StoreError::Unsupported(_) => error,
        }
    }

    /// `message` with each value, as it is or percent-encoded, written back
    /// as `${NAME}`.
    fn hidden(&self, message: &str) -> String {
        self.values
            .iter()
            .filter(|(_, value)| !value.is_empty())
            .fold(message.to_owned(), |shown, (name, value)| {
                let variable = format!("${{{name}}}");
                shown
                    .replace(&encoded(value), &variable)
                    .replace(value.as_str(), &variable)
            })
    }

    /// `error` with each value of this expansion, as it is or percent-encoded,
    /// written back as `${NAME}` in an `InvalidStore` or `NotConfigured`
    /// message; any other error unchanged.
    #[cfg(feature = "mongodb")]
    pub(super) fn hide(&self, error: StoreError) -> StoreError {
        match error {
            StoreError::InvalidStore(message) => StoreError::InvalidStore(self.hidden(&message)),
            StoreError::NotConfigured(message) => StoreError::NotConfigured(self.hidden(&message)),
            StoreError::Backend(_)
            | StoreError::Conflict(_)
            | StoreError::InvalidPath(_)
            | StoreError::NotFound(_)
            | StoreError::Unsupported(_) => error,
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
