//! The frontmatter block: finding it and reading it in its own syntax.

use core::ops::Range;
use core::{iter, mem};

use indexmap::IndexMap;
use indexmap::map::Entry;

use crate::parser::ast::FrontmatterFormat;
use crate::parser::cursor::{Cursor, Dropped, Undo};
use crate::parser::error::{Failure, ParseError};
use crate::parser::line::{Line, line_at};
use crate::parser::located::{BrokenFrontmatter, Frontmatter, Mapping, Pair, Sequence, Value};
use crate::parser::{toml, yaml};

/// How deep frontmatter values nest: deeper ones overflow a 2 MB debug stack.
pub const VALUE_CAP: usize = 400;

/// Why the TOML and YAML readers stop reading a value: what they met drops
/// the innermost pair around it, or fails the file.
pub enum Stop {
    Drop(ParseError),
    Fail(ParseError),
}

impl From<ParseError> for Stop {
    fn from(error: ParseError) -> Self {
        Self::Fail(error)
    }
}

impl From<Stop> for ParseError {
    fn from(stop: Stop) -> Self {
        match stop {
            Stop::Drop(error) | Stop::Fail(error) => error,
        }
    }
}

/// The frontmatter `source` opens with at byte `start`, if any, and the
/// offset its body starts at. A `strict` parse fails with the error of the
/// first entry the frontmatter drops, or of a frontmatter no entry of reads.
pub fn parse<F>(
    source: &str,
    start: usize,
    strict: bool,
) -> Result<(Option<Result<Frontmatter, BrokenFrontmatter>>, usize), F>
where
    F: Failure,
{
    let opening = line_at(source, start);
    let format = match source[start..opening.end].trim_end() {
        "+++" => FrontmatterFormat::Toml,
        "---" => FrontmatterFormat::Yaml,
        _ => return Ok((None, start)),
    };
    let Some(closing) = closing(source, opening.next, format) else {
        return Ok((None, start));
    };
    let content = opening.next..closing.start;
    let read = match format {
        FrontmatterFormat::Toml => toml::read(source, content.clone()),
        FrontmatterFormat::Yaml => yaml::read(source, content.clone()),
    };
    let span = start..closing.start + 3;
    let frontmatter = match read.and_then(kept) {
        Ok((mapping, drops)) => {
            if let Some(first) = drops.first().filter(|_| strict) {
                F::fail(first.error.clone())?;
            }
            let dropped = drops
                .into_iter()
                .map(|entry| ParseError {
                    span: entry.span,
                    ..entry.error
                })
                .collect();
            Ok(Frontmatter {
                dropped,
                format,
                mapping: mapping.into_model(),
                span,
            })
        }
        Err(error) => {
            if strict {
                F::fail(error.clone())?;
            }
            Err(BrokenFrontmatter {
                error,
                format,
                span,
                text: without_line_ending(&source[content]).to_owned(),
            })
        }
    };
    Ok((Some(frontmatter), closing.next))
}

/// The mapping a reader read, `dropped` empty, or else the error of the
/// first entry or key it dropped.
pub fn undropped((mapping, dropped): (Mapping, Vec<Dropped>)) -> Result<Mapping, ParseError> {
    dropped
        .into_iter()
        .next()
        .map_or(Ok(mapping), |first| Err(first.error))
}

/// The mapping a reader read and what it dropped, or the error of the first
/// entry it dropped when it kept no pair.
fn kept(
    (mapping, dropped): (Mapping, Vec<Dropped>),
) -> Result<(Mapping, Vec<Dropped>), ParseError> {
    match dropped.first() {
        Some(first) if mapping.pairs.is_empty() => Err(first.error.clone()),
        Some(_) | None => Ok((mapping, dropped)),
    }
}

/// `text` without the line ending of its last line.
fn without_line_ending(text: &str) -> &str {
    text.strip_suffix('\n')
        .map_or(text, |line| line.strip_suffix('\r').unwrap_or(line))
}

/// The depth of a value opened at `span` inside `open` values, or a drop
/// pointing at `span` when that passes `VALUE_CAP`.
pub fn nest(open: usize, span: Range<usize>) -> Result<usize, Stop> {
    if open < VALUE_CAP {
        return Ok(open + 1);
    }
    let message = format!("the value nests deeper than {VALUE_CAP} levels");
    Err(Stop::Drop(ParseError::new(&message, span)))
}

/// What `read` gives, or the error of the drop it raised, after which
/// `skip` moves the cursor from `start` past what is dropped. A failure
/// stays one.
pub fn caught<S, T, F>(
    cursor: &mut Cursor<'_, S>,
    start: usize,
    read: Result<T, Stop>,
    skip: F,
) -> Result<Result<T, ParseError>, Stop>
where
    F: FnOnce(&mut Cursor<'_, S>),
    S: Undo,
{
    match read {
        Ok(value) => Ok(Ok(value)),
        Err(Stop::Drop(error)) => {
            cursor.set_pos(start);
            skip(cursor);
            Ok(Err(error))
        }
        Err(failure @ Stop::Fail(_)) => Err(failure),
    }
}

/// Adds a pair to a mapping being read. A key written again joins the first
/// one: its value becomes a list of every value written for it.
pub fn insert(pairs: &mut IndexMap<String, Pair>, key: String, pair: Pair) {
    match pairs.entry(key) {
        Entry::Vacant(slot) => {
            slot.insert(pair);
        }
        Entry::Occupied(slot) => {
            let first = slot.into_mut();
            first.repeats.push(pair.span);
            append(&mut first.value, pair.value);
        }
    }
}

/// Makes `first` a list of its own items followed by those of `value`. The
/// list keeps the range of `first`.
pub fn append(first: &mut Value, value: Value) {
    let span = first.span().clone();
    let mut items = into_items(mem::replace(first, empty(span.start)));
    items.extend(into_items(value));
    *first = Value::Sequence(Sequence { items, span });
}

/// A list gives its items; any other value gives itself.
fn into_items(value: Value) -> Vec<Value> {
    match value {
        Value::Sequence(sequence) => sequence.items,
        Value::Mapping(_) | Value::Scalar(_) => vec![value],
    }
}

/// A list with no items, at `at`.
const fn empty(at: usize) -> Value {
    Value::Sequence(Sequence {
        items: Vec::new(),
        span: at..at,
    })
}

/// The mapping holding `pairs`. Its span runs from the first key to the end
/// of the last value written; with no pairs it is empty, at `empty_at`.
pub fn finish(pairs: IndexMap<String, Pair>, empty_at: usize) -> Mapping {
    let span = extent(&pairs).unwrap_or(empty_at..empty_at);
    Mapping { pairs, span }
}

/// From the first key of `pairs` to the end of the last value written, or
/// `None` when there are no pairs.
pub fn extent(pairs: &IndexMap<String, Pair>) -> Option<Range<usize>> {
    let end = pairs
        .values()
        .flat_map(|pair| iter::once(&pair.span).chain(&pair.repeats))
        .map(|span| span.end)
        .max();
    pairs
        .first()
        .zip(end)
        .map(|((_, first), last_end)| first.span.start..last_end)
}

pub fn closing(source: &str, from: usize, format: FrontmatterFormat) -> Option<Line> {
    let mut start = from;
    while start < source.len() {
        let line = line_at(source, start);
        let text = source[line.start..line.end].trim_end();
        let closes = match format {
            FrontmatterFormat::Toml => text == "+++",
            FrontmatterFormat::Yaml => text == "---" || text == "...",
        };
        if closes {
            return Some(line);
        }
        start = line.next;
    }
    None
}
