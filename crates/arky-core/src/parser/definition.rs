//! Link reference definitions, such as `[guide]: <https://example.com> "Guide"`,
//! and the labels, destinations and titles links share with them.

use core::cell::Cell;
use std::collections::HashMap;

use unicase::UniCase;

use crate::parser::entity::resolve;

/// The deepest parentheses may nest in a destination. `CommonMark` lets a
/// reader limit it, so a run of `(` costs no more than this to refuse.
const PARENTHESES: usize = 32;

/// The link definitions of a file: each target by its folded label.
pub type Definitions = HashMap<String, Target>;

/// A definition read from the start of a text.
pub struct Definition {
    /// The text between the brackets, as written.
    pub label: String,
    /// How many lines of the text the definition takes.
    pub lines: usize,
    pub target: Target,
}

/// Where a link points: its destination, without the angle brackets around
/// it, and its title, each with escapes and character references resolved.
#[derive(Clone)]
pub struct Target {
    pub destination: String,
    pub title: Option<String>,
}

/// The definition a text starts with, if any, as far as the text goes.
pub struct Reading {
    pub definition: Option<Definition>,
    /// Whether a line added to the text could change `definition`.
    pub open: bool,
}

/// The bytes a definition is read from. Reading past their end marks the
/// reading open.
struct Bytes<'text> {
    bytes: &'text [u8],
    past_end: Cell<bool>,
}

impl Bytes<'_> {
    fn at(&self, at: usize) -> Option<u8> {
        let byte = self.bytes.get(at).copied();
        if byte.is_none() {
            self.past_end.set(true);
        }
        byte
    }

    /// Whether the backslash at `at` escapes the character after it, which
    /// it does for ASCII punctuation.
    fn escapes(&self, at: usize) -> bool {
        self.at(at + 1)
            .is_some_and(|byte| byte.is_ascii_punctuation())
    }
}

/// The definition that `text`, lines joined by `\n`, starts with: a label,
/// `:`, a destination and an optional title, with nothing after them on
/// their line.
pub fn read(text: &str) -> Reading {
    let bytes = Bytes {
        bytes: text.as_bytes(),
        past_end: Cell::new(false),
    };
    let definition = definition(text, &bytes);
    Reading {
        definition,
        open: bytes.past_end.get(),
    }
}

/// A link label as `CommonMark` matches labels: Unicode full case folding,
/// then each run of spaces, tabs and line endings one space, and none at
/// either end.
pub fn fold_label(label: &str) -> String {
    UniCase::new(label)
        .to_folded_case()
        .split_ascii_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The target of the inline link `(destination "title")` whose `(` is at
/// `open` in `text`, and where it ends, past its `)`. The destination and
/// the title may each be left out. Spaces and tabs around them may hold one
/// line ending.
pub fn inline(text: &str, open: usize) -> Option<(Target, usize)> {
    let bytes = Bytes {
        bytes: text.as_bytes(),
        past_end: Cell::new(false),
    };
    if bytes.at(open) != Some(b'(') {
        return None;
    }
    let start = gap(&bytes, open + 1);
    let (destination, after) = if bytes.at(start) == Some(b')') {
        (String::new(), start)
    } else {
        destination(text, &bytes, start)?
    };
    let title_at = gap(&bytes, after);
    let titled = (title_at > after)
        .then(|| title_end(&bytes, title_at))
        .flatten();
    let (title, close) = titled.map_or((None, title_at), |title_close| {
        (
            Some(resolve(&text[title_at + 1..title_close - 1])),
            gap(&bytes, title_close),
        )
    });
    (bytes.at(close) == Some(b')')).then(|| (Target { destination, title }, close + 1))
}

/// The `]` that closes the link label whose `[` is at `open` in `text`, if
/// the brackets make one.
pub fn label_at(text: &str, open: usize) -> Option<usize> {
    let bytes = Bytes {
        bytes: text.as_bytes(),
        past_end: Cell::new(false),
    };
    label_end(text, &bytes, open)
}

fn definition(text: &str, bytes: &Bytes<'_>) -> Option<Definition> {
    let close = label_end(text, bytes, 0)?;
    if bytes.at(close + 1) != Some(b':') {
        return None;
    }
    let (destination, after) = destination(text, bytes, gap(bytes, close + 2))?;
    let title_at = gap(bytes, after);
    let titled = (title_at > after)
        .then(|| title_end(bytes, title_at))
        .flatten()
        .and_then(|title_close| {
            let end = line_end(bytes, title_close)?;
            Some((end, resolve(&text[title_at + 1..title_close - 1])))
        });
    let (end, title) = match titled {
        Some((end, title)) => (end, Some(title)),
        None => (line_end(bytes, after)?, None),
    };
    Some(Definition {
        label: text[1..close].to_owned(),
        lines: text[..end].matches('\n').count() + 1,
        target: Target { destination, title },
    })
}

/// The `]` that closes the label whose `[` is at `open`: the first one not
/// escaped, with no unescaped `[` before it, around at most 999 characters
/// that are not all whitespace.
fn label_end(text: &str, bytes: &Bytes<'_>, open: usize) -> Option<usize> {
    if bytes.at(open) != Some(b'[') {
        return None;
    }
    let mut at = open + 1;
    loop {
        match bytes.at(at)? {
            b'\\' if bytes.escapes(at) => at += 2,
            b'[' => return None,
            b']' => break,
            _ => at += 1,
        }
    }
    let inner = &text[open + 1..at];
    (inner.chars().count() <= 999 && !inner.trim_ascii().is_empty()).then_some(at)
}

/// The destination at `at`, resolved, and where it ends: text between `<`
/// and `>` on one line, or a nonempty run with no space or control
/// character whose parentheses pair up, at most `PARENTHESES` deep.
fn destination(text: &str, bytes: &Bytes<'_>, at: usize) -> Option<(String, usize)> {
    if bytes.at(at) == Some(b'<') {
        let mut close = at + 1;
        loop {
            match bytes.at(close)? {
                b'\\' if bytes.escapes(close) => close += 2,
                b'>' => break,
                b'<' | b'\n' => return None,
                _ => close += 1,
            }
        }
        return Some((resolve(&text[at + 1..close]), close + 1));
    }
    let mut end = at;
    let mut depth = 0_usize;
    while let Some(byte) = bytes.at(end) {
        match byte {
            b'\\' if bytes.escapes(end) => end += 2,
            b'(' if depth == PARENTHESES => return None,
            b'(' => {
                depth += 1;
                end += 1;
            }
            b')' if depth > 0 => {
                depth -= 1;
                end += 1;
            }
            b')' => break,
            _ if byte <= b' ' || byte == 0x7f => break,
            _ => end += 1,
        }
    }
    (end > at && depth == 0).then(|| (resolve(&text[at..end]), end))
}

/// Where the title at `at` ends, past its closing mark. A title is text
/// between `"`, `'` or parentheses, with no unescaped closing mark inside,
/// nor an unescaped `(` inside parentheses.
fn title_end(bytes: &Bytes<'_>, at: usize) -> Option<usize> {
    let close = match bytes.at(at)? {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return None,
    };
    let mut end = at + 1;
    loop {
        match bytes.at(end)? {
            b'\\' if bytes.escapes(end) => end += 2,
            byte if byte == close => return Some(end + 1),
            b'(' if close == b')' => return None,
            _ => end += 1,
        }
    }
}

/// Past the spaces and tabs at `at`, with at most one line ending among
/// them.
fn gap(bytes: &Bytes<'_>, at: usize) -> usize {
    let spaces = spaces_end(bytes, at);
    if bytes.at(spaces) == Some(b'\n') {
        spaces_end(bytes, spaces + 1)
    } else {
        spaces
    }
}

/// The end of the line at `at`, when only spaces and tabs follow `at` on it.
fn line_end(bytes: &Bytes<'_>, at: usize) -> Option<usize> {
    let end = spaces_end(bytes, at);
    matches!(bytes.at(end), None | Some(b'\n')).then_some(end)
}

/// Past the spaces and tabs at `at`.
fn spaces_end(bytes: &Bytes<'_>, at: usize) -> usize {
    let mut end = at;
    while matches!(bytes.at(end), Some(b' ' | b'\t')) {
        end += 1;
    }
    end
}
