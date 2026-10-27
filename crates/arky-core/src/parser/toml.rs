//! TOML frontmatter: `key = value` lines and `[table]` headers into a
//! mapping.

use core::ops::Range;
use core::{iter, mem};
use std::collections::HashMap;

use indexmap::IndexMap;
use indexmap::map::Entry;

use crate::parser::cursor::{Cursor, Dropped};
use crate::parser::error::ParseError;
use crate::parser::frontmatter::{Stop, append, caught, extent, finish, insert, nest, undropped};
use crate::parser::located::{Mapping, Pair, Scalar, ScalarValue, Sequence, Value};
use crate::parser::scalar;

/// One part of a key, such as `name` in `owner.name`, and where it is
/// written.
type Part = (String, Range<usize>);

/// The tables a dotted key or a header made, by key, each with those made
/// inside it. A dotted key or a header goes on into them; a key whose value
/// was written with `=` is written again.
#[derive(Default)]
struct Made(HashMap<String, Self>);

pub fn parse(source: &str, content: Range<usize>) -> Result<Mapping, ParseError> {
    read(source, content).and_then(undropped)
}

/// The mapping [`parse`] reads, with each statement, pair and table it
/// dropped. A frontmatter none of whose statements reads fails with the error
/// of the first.
pub fn read(source: &str, content: Range<usize>) -> Result<(Mapping, Vec<Dropped>), ParseError> {
    let empty_at = content.start;
    let mut cursor = Cursor::new(source, content, ());
    let mut root = IndexMap::new();
    // The path of the open table; empty is the root.
    let mut current: Vec<String> = Vec::new();
    // Tables made only because a deeper header named them; a header may still
    // add to them.
    let mut implicit: Vec<Vec<String>> = Vec::new();
    let mut made = Made::default();
    // `None` before the first statement, `Ok` once one reads, or else the
    // error of the first.
    let mut outcome = None;
    loop {
        skip_trivia(&mut cursor);
        if cursor.at_end() {
            break;
        }
        let mark = cursor.mark();
        let is_header = cursor.peek() == Some(b'[');
        let read = if is_header {
            header(&mut cursor, &mut root, &mut implicit, &made).map(|(path, end)| {
                current = path;
                make(&mut made, &current);
                end
            })
        } else {
            statement(&mut cursor, &mut root, &current, &mut implicit, &mut made)
        };
        let error = match read {
            Ok(end) => {
                stretch(&mut root, &current, end);
                outcome = Some(Ok(()));
                continue;
            }
            Err(Stop::Drop(error)) => {
                outcome = Some(Ok(()));
                error
            }
            Err(Stop::Fail(error)) => {
                let _: &mut Result<(), ParseError> =
                    outcome.get_or_insert_with(|| Err(error.clone()));
                error
            }
        };
        cursor.set_pos(mark.pos);
        if is_header {
            skip_table(&mut cursor);
        } else {
            skip_broken_pair(&mut cursor);
        }
        cursor.drop_since(mark, error);
    }
    if let Some(Err(error)) = outcome {
        return Err(error);
    }
    Ok((finish(root, empty_at), cursor.into_dropped()))
}

/// Reads the `key = value` pair at the cursor and the end of its line into
/// the open table at `current`, and returns where the pair ends.
fn statement(
    cursor: &mut Cursor<'_>,
    root: &mut IndexMap<String, Pair>,
    current: &[String],
    implicit: &mut Vec<Vec<String>>,
    made: &mut Made,
) -> Result<usize, Stop> {
    let (parents, last, value) = pair(cursor, current.len())?;
    let end = cursor.pos();
    end_of_line(cursor)?;
    let keys = put_pair(
        table_at(root, current),
        make(made, current),
        &parents,
        last,
        value,
    );
    // The key written with `=` holds a value of its own, so no table under it
    // is one only a deeper header made.
    let full = [current, keys.as_slice()].concat();
    implicit.retain(|path| !dotted_through(path, current, &keys) && !path.starts_with(&full));
    Ok(end)
}

/// Reads a `[a.b]` or `[[a.b]]` header to its line's end, opens the table it
/// names and returns its path and where it ends. A table written again follows
/// the repeated-key rule; a table made only by a deeper header takes the pairs
/// of its own header. Before the last part, the path goes on into the tables of
/// `made`; any other value is the key written again.
fn header(
    cursor: &mut Cursor<'_>,
    root: &mut IndexMap<String, Pair>,
    implicit: &mut Vec<Vec<String>>,
    made: &Made,
) -> Result<(Vec<String>, usize), Stop> {
    let start = cursor.pos();
    cursor.bump();
    let array = cursor.eat(b'[');
    cursor.skip_spaces();
    let (parents, last) = key_path(cursor)?;
    parents
        .iter()
        .chain(iter::once(&last))
        .try_fold(0, |open, (_, written)| nest(open, written.clone()))?;
    if !(cursor.eat(b']') && (!array || cursor.eat(b']'))) {
        return cursor.error("expected `]` to close the table header");
    }
    let span = start..cursor.pos();
    end_of_line(cursor)?;
    let path: Vec<String> = parents
        .into_iter()
        .chain(iter::once(last))
        .map(|(name, _)| name)
        .collect();
    let mut table = root;
    let mut tables = Some(made);
    for (depth, name) in path.iter().enumerate() {
        let prefix = &path[..=depth];
        let named = depth + 1 == path.len();
        let inside = tables.and_then(|level| level.0.get(name));
        let pair = match table.entry(name.clone()) {
            Entry::Vacant(slot) => {
                if !named {
                    implicit.push(prefix.to_vec());
                }
                let value = if named && array {
                    Value::Sequence(Sequence {
                        items: vec![mapping_at(span.end)],
                        span: span.clone(),
                    })
                } else {
                    mapping_at(span.end)
                };
                slot.insert(Pair {
                    repeats: Vec::new(),
                    span: span.clone(),
                    value,
                })
            }
            Entry::Occupied(slot) => {
                let pair = slot.into_mut();
                pair.repeats.push(span.clone());
                if named {
                    let opens = match &pair.value {
                        Value::Mapping(_) => true,
                        Value::Sequence(sequence) => {
                            matches!(sequence.items.last(), Some(Value::Mapping(_)))
                        }
                        Value::Scalar(_) => false,
                    };
                    let adds =
                        !array && opens && implicit.iter().any(|other| other.as_slice() == prefix);
                    if adds {
                        implicit.retain(|other| other.as_slice() != prefix);
                    } else {
                        implicit.retain(|other| !other.starts_with(prefix));
                        append(&mut pair.value, mapping_at(span.end));
                    }
                }
                if !named && inside.is_none() {
                    append(&mut pair.value, mapping_at(span.end));
                    implicit.push(prefix.to_vec());
                }
                pair
            }
        };
        tables = inside;
        table = &mut table_of(&mut pair.value).pairs;
    }
    Ok((path, span.end))
}

/// The open table at `path`, into the last item of an array of tables. The
/// walk stops early where the path names nothing.
fn table_at<'root>(
    root: &'root mut IndexMap<String, Pair>,
    path: &[String],
) -> &'root mut IndexMap<String, Pair> {
    path.iter()
        .fold(root, |table, name| match table.get_index_of(name) {
            Some(index) => &mut table_of(&mut table[index].value).pairs,
            None => table,
        })
}

/// The table `value` holds: itself, or the last item of a list when that
/// item is a mapping. Anything else first gets an empty mapping after it, by
/// the repeated-key rule.
fn table_of(value: &mut Value) -> &mut Mapping {
    match value {
        Value::Mapping(mapping) => mapping,
        Value::Sequence(sequence) => {
            if !matches!(sequence.items.last(), Some(Value::Mapping(_))) {
                sequence.items.push(mapping_at(sequence.span.end));
            }
            let last = sequence.items.len() - 1;
            table_of(&mut sequence.items[last])
        }
        Value::Scalar(_) => {
            let end = value.span().end;
            append(value, mapping_at(end));
            table_of(value)
        }
    }
}

/// Records each table on `path` as made, and returns the tables made inside
/// the last.
fn make<'made>(made: &'made mut Made, path: &[String]) -> &'made mut Made {
    path.iter().fold(made, |tables, name| {
        tables.0.entry(name.clone()).or_default()
    })
}

/// An empty mapping at `at`.
fn mapping_at(at: usize) -> Value {
    Value::Mapping(Mapping {
        pairs: IndexMap::new(),
        span: at..at,
    })
}

/// Extends each table on `path` to `end`: the latest place its key is
/// written, the list it sits in, and its mapping.
fn stretch(pairs: &mut IndexMap<String, Pair>, path: &[String], end: usize) {
    let Some((name, rest)) = path.split_first() else {
        return;
    };
    let Some(pair) = pairs.get_mut(name) else {
        return;
    };
    let latest = pair.repeats.last_mut().unwrap_or(&mut pair.span);
    latest.end = latest.end.max(end);
    if let Value::Sequence(sequence) = &mut pair.value {
        sequence.span.end = sequence.span.end.max(end);
    }
    let table = table_of(&mut pair.value);
    stretch(&mut table.pairs, rest, end);
    fit(table);
}

/// Sets the span of a mapping read as lines to cover its pairs.
fn fit(mapping: &mut Mapping) {
    if let Some(span) = extent(&mapping.pairs) {
        mapping.span = span;
    }
}

/// Whether `path` is a table the dotted key `keys`, written in the table at
/// `current`, walks through.
fn dotted_through(path: &[String], current: &[String], keys: &[String]) -> bool {
    path.strip_prefix(current)
        .is_some_and(|rest| !rest.is_empty() && rest.len() < keys.len() && keys.starts_with(rest))
}

/// Reads `key = value` into `pairs`, a table inside `open` values whose
/// tables a dotted key or a header made are `made`. A pair that raises a drop
/// is noted with its error, in place of the drops noted inside it.
fn entry(
    cursor: &mut Cursor<'_>,
    pairs: &mut IndexMap<String, Pair>,
    made: &mut Made,
    open: usize,
) -> Result<(), Stop> {
    let mark = cursor.mark();
    let read = pair(cursor, open);
    match caught(cursor, mark.pos, read, skip_pair)? {
        Ok((parents, last, value)) => {
            let _: Vec<String> = put_pair(pairs, made, &parents, last, value);
        }
        Err(dropped) => cursor.drop_since(mark, dropped),
    }
    Ok(())
}

/// Adds `value` under the key whose parts are `parents` and `last`, as
/// [`put`] does, and returns the name of each part.
fn put_pair(
    pairs: &mut IndexMap<String, Pair>,
    made: &mut Made,
    parents: &[Part],
    last: Part,
    value: Value,
) -> Vec<String> {
    let keys = parents
        .iter()
        .chain(iter::once(&last))
        .map(|(name, _)| name.clone())
        .collect();
    put(pairs, made, parents, last, value);
    keys
}

/// The parts before the last of a `key = value` pair inside `open` values,
/// the last, and the value.
fn pair(cursor: &mut Cursor<'_>, open: usize) -> Result<(Vec<Part>, Part, Value), Stop> {
    let (parents, last) = key_path(cursor)?;
    let depth = parents
        .iter()
        .try_fold(open, |depth, (_, written)| nest(depth, written.clone()))?;
    if !cursor.eat(b'=') {
        return cursor.error("expected `=` after the key");
    }
    cursor.skip_spaces();
    let value = value(cursor, depth)?;
    Ok((parents, last, value))
}

/// Moves the cursor from the start of a pair that is dropped to its end:
/// the first `,`, `]`, `}` or line ending outside every bracket the pair
/// opens. Strings and comments are passed over whole.
fn skip_pair(cursor: &mut Cursor<'_>) {
    let mut depth = 0_usize;
    loop {
        match cursor.peek() {
            None => return,
            Some(b',' | b']' | b'}' | b'\n') if depth == 0 => return,
            Some(b'[' | b'{') => depth += 1,
            Some(b']' | b'}') => depth -= 1,
            Some(b'"' | b'\'') => {
                skip_string(cursor);
                continue;
            }
            Some(b'#') => {
                skip_comment(cursor);
                continue;
            }
            Some(_) => {}
        }
        cursor.bump();
    }
}

/// Moves the cursor from the start of a pair that does not read, or that
/// nests past the value cap, past its value, as [`skip_value`] does from its
/// `=`, then on as [`skip_statement`] does.
fn skip_broken_pair(cursor: &mut Cursor<'_>) {
    loop {
        match cursor.peek() {
            None | Some(b'\n' | b'#') => break,
            Some(b'=') => {
                skip_value(cursor);
                break;
            }
            Some(b'"' | b'\'') => skip_string(cursor),
            Some(_) => cursor.bump(),
        }
    }
    skip_statement(cursor);
}

/// Moves the cursor from the `=` of a pair that does not read, or from the
/// start of a pair in a dropped table, to the line ending where every bracket
/// after it is closed. When one never closes, it stops at the line ending
/// before the first later line that starts a key followed by `=`. Strings and
/// comments are passed over whole.
fn skip_value(cursor: &mut Cursor<'_>) {
    let mut depth = 0_usize;
    let mut next_pair = None;
    loop {
        match cursor.peek() {
            None => break,
            Some(b'\n') if depth == 0 => return,
            Some(b'\n') => {
                let at = cursor.pos();
                cursor.bump();
                if next_pair.is_none() && starts_pair(cursor) {
                    next_pair = Some(at);
                }
                continue;
            }
            Some(b'[' | b'{') => depth += 1,
            Some(b']' | b'}') => depth = depth.saturating_sub(1),
            Some(b'"' | b'\'') => {
                skip_string(cursor);
                continue;
            }
            Some(b'#') => {
                skip_comment(cursor);
                continue;
            }
            Some(_) => {}
        }
        cursor.bump();
    }
    if depth > 0
        && let Some(at) = next_pair
    {
        cursor.set_pos(at);
    }
}

/// Moves the cursor past the string at it to where [`quoted`] would close
/// it, also past an escape that does not read. One left open runs to the end
/// of its line, or for `"""` and `'''`, to the end.
fn skip_string(cursor: &mut Cursor<'_>) {
    let Some(quote) = cursor.peek() else {
        return;
    };
    let multi = cursor.eat_str(if quote == b'"' { "\"\"\"" } else { "'''" });
    if !multi {
        cursor.bump();
    }
    loop {
        match cursor.peek() {
            None => return,
            Some(b'\n') if !multi => return,
            Some(b'\\') if quote == b'"' => {
                cursor.bump();
                if matches!(cursor.peek(), Some(b'"' | b'\\')) {
                    cursor.bump();
                }
            }
            Some(byte) if byte == quote => {
                if !multi {
                    cursor.bump();
                    return;
                }
                if quotes(cursor, quote, &mut String::new()) {
                    return;
                }
            }
            Some(_) => cursor.bump(),
        }
    }
}

/// Moves the cursor from the start of a statement that does not read past
/// it: to the next line that starts a key followed by `=`, or a `[`. Strings
/// are passed over as [`skip_string`] passes them.
fn skip_statement(cursor: &mut Cursor<'_>) {
    loop {
        match cursor.peek() {
            None => return,
            Some(b'\n') => {
                cursor.bump();
                if starts_statement(cursor) {
                    return;
                }
            }
            Some(b'"' | b'\'') => skip_string(cursor),
            Some(b'#') => skip_comment(cursor),
            Some(_) => cursor.bump(),
        }
    }
}

/// Whether the line at the cursor starts a statement: after spaces and tabs,
/// a `[`, or a key followed by `=`.
fn starts_statement(cursor: &mut Cursor<'_>) -> bool {
    cursor
        .rest()
        .trim_start_matches([' ', '\t'])
        .starts_with('[')
        || starts_pair(cursor)
}

/// Whether the line at the cursor starts, after spaces and tabs, with a key
/// followed by `=`.
fn starts_pair(cursor: &mut Cursor<'_>) -> bool {
    let start = cursor.pos();
    cursor.skip_spaces();
    let starts = key_path(cursor).is_ok_and(|_| cursor.peek() == Some(b'='));
    cursor.set_pos(start);
    starts
}

/// Moves the cursor from a header that is dropped past its table and the
/// pairs in it: to the line ending before the next header, or to the end.
/// The header's line is passed as text, as a broken one may leave a `[` open,
/// and each pair as [`skip_value`] passes it.
fn skip_table(cursor: &mut Cursor<'_>) {
    let _: &str = cursor.take_until(b"\n");
    loop {
        skip_value(cursor);
        let rest = cursor.rest();
        let header_follows = rest
            .strip_prefix('\n')
            .is_some_and(|next| next.trim_start_matches([' ', '\t']).starts_with('['));
        if rest.is_empty() || header_follows {
            return;
        }
        cursor.bump();
    }
}

/// Adds `value` under `last`, inside the tables `parents` name. A part goes
/// on into a table of `made`; any other part gets a new table, which joins a
/// value already written for its key. `last` is written with `=`, so it
/// leaves `made`.
fn put(
    pairs: &mut IndexMap<String, Pair>,
    made: &mut Made,
    parents: &[Part],
    last: Part,
    value: Value,
) {
    let end = value.span().end;
    let Some(((name, written), rest)) = parents.split_first() else {
        let (name, written) = last;
        made.0.remove(&name);
        let pair = Pair {
            repeats: Vec::new(),
            span: written.start..end,
            value,
        };
        insert(pairs, name, pair);
        return;
    };
    let span = written.start..end;
    if let (Some(pair), Some(inside)) = (pairs.get_mut(name), made.0.get_mut(name)) {
        pair.repeats.push(span);
        let table = table_of(&mut pair.value);
        put(&mut table.pairs, inside, rest, last, value);
        fit(table);
    } else {
        let mut inside = Made::default();
        let mut table = IndexMap::new();
        put(&mut table, &mut inside, rest, last, value);
        made.0.insert(name.clone(), inside);
        let pair = Pair {
            repeats: Vec::new(),
            span,
            value: Value::Mapping(finish(table, written.start)),
        };
        insert(pairs, name.clone(), pair);
    }
}

/// A key, or a dotted key such as `owner.name` with spaces allowed around
/// each `.`: the parts before the last, and the last.
fn key_path(cursor: &mut Cursor<'_>) -> Result<(Vec<Part>, Part), ParseError> {
    let mut parents = Vec::new();
    let mut last = part(cursor)?;
    loop {
        cursor.skip_spaces();
        if !cursor.eat(b'.') {
            return Ok((parents, last));
        }
        cursor.skip_spaces();
        let next = part(cursor)?;
        parents.push(mem::replace(&mut last, next));
    }
}

fn part(cursor: &mut Cursor<'_>) -> Result<Part, ParseError> {
    let start = cursor.pos();
    let name = key(cursor)?;
    Ok((name, start..cursor.pos()))
}

fn key(cursor: &mut Cursor<'_>) -> Result<String, ParseError> {
    match cursor.peek() {
        Some(b'"') => basic_string(cursor),
        Some(b'\'') => literal_string(cursor),
        Some(_) | None => {
            let start = cursor.pos();
            while cursor
                .peek()
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                cursor.bump();
            }
            if cursor.pos() == start {
                return cursor.error("expected a key");
            }
            Ok(cursor.slice(start..cursor.pos()).to_owned())
        }
    }
}

/// The value at the cursor, inside `open` values.
fn value(cursor: &mut Cursor<'_>, open: usize) -> Result<Value, Stop> {
    match cursor.peek() {
        Some(b'"' | b'\'') => quoted(cursor).map_err(Stop::Fail),
        Some(b'[') => array(cursor, open),
        Some(b'{') => inline_table(cursor, open),
        Some(_) | None => bare(cursor).map_err(Stop::Fail),
    }
}

/// The string at the cursor, written with `"`, `'`, `"""` or `'''`.
fn quoted(cursor: &mut Cursor<'_>) -> Result<Value, ParseError> {
    let start = cursor.pos();
    let text = if cursor.eat_str("\"\"\"") {
        multi_line_basic_string(cursor)?
    } else if cursor.eat_str("'''") {
        multi_line_literal_string(cursor)?
    } else if cursor.peek() == Some(b'"') {
        basic_string(cursor)?
    } else {
        literal_string(cursor)?
    };
    Ok(string(start..cursor.pos(), text))
}

const fn string(span: Range<usize>, text: String) -> Value {
    Value::Scalar(Scalar {
        span,
        value: ScalarValue::String(text),
    })
}

fn basic_string(cursor: &mut Cursor<'_>) -> Result<String, ParseError> {
    cursor.bump();
    let mut text = String::new();
    loop {
        text.push_str(cursor.take_until(b"\"\\\n"));
        if cursor.eat(b'"') {
            return Ok(text);
        }
        if !cursor.eat(b'\\') {
            return cursor.error("the string is not closed");
        }
        text.push(escape(cursor)?);
    }
}

/// The text of a `"""` string, read from after its opening quotes. A `\` at
/// the end of a line drops the line ending and the whitespace after it.
fn multi_line_basic_string(cursor: &mut Cursor<'_>) -> Result<String, ParseError> {
    skip_newline(cursor);
    let mut text = String::new();
    loop {
        text.push_str(cursor.take_until(b"\"\\"));
        if cursor.at_end() {
            return cursor.error("the string is not closed");
        }
        if cursor.peek() == Some(b'"') {
            if quotes(cursor, b'"', &mut text) {
                return Ok(text);
            }
        } else {
            cursor.bump();
            if cursor.rest_of_line().trim_matches([' ', '\t']).is_empty() {
                while matches!(cursor.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                    cursor.bump();
                }
            } else {
                text.push(escape(cursor)?);
            }
        }
    }
}

/// The text of a `'''` string, read from after its opening quotes. It has no
/// escapes.
fn multi_line_literal_string(cursor: &mut Cursor<'_>) -> Result<String, ParseError> {
    skip_newline(cursor);
    let mut text = String::new();
    loop {
        text.push_str(cursor.take_until(b"'"));
        if cursor.at_end() {
            return cursor.error("the string is not closed");
        }
        if quotes(cursor, b'\'', &mut text) {
            return Ok(text);
        }
    }
}

/// The line ending right after the opening quotes of a multi-line string,
/// which is not part of its text.
fn skip_newline(cursor: &mut Cursor<'_>) {
    let _: bool = cursor.eat_str("\r\n") || cursor.eat(b'\n');
}

/// Reads a run of `quote` inside a multi-line string and returns whether it
/// closes the string. Three quotes close it, and up to two more before them
/// belong to the text.
fn quotes(cursor: &mut Cursor<'_>, quote: u8, text: &mut String) -> bool {
    let run = cursor
        .rest()
        .bytes()
        .take_while(|&byte| byte == quote)
        .count();
    let closes = run >= 3;
    let inside = if closes { (run - 3).min(2) } else { run };
    text.extend(iter::repeat_n(char::from(quote), inside));
    cursor.set_pos(cursor.pos() + if closes { inside + 3 } else { run });
    closes
}

fn escape(cursor: &mut Cursor<'_>) -> Result<char, ParseError> {
    let Some(byte) = cursor.peek() else {
        return cursor.error("the string is not closed");
    };
    cursor.bump();
    let character = match byte {
        b'b' => Some('\u{8}'),
        b't' => Some('\t'),
        b'n' => Some('\n'),
        b'f' => Some('\u{c}'),
        b'r' => Some('\r'),
        b'"' => Some('"'),
        b'\\' => Some('\\'),
        b'u' => cursor.hex_char(4),
        b'U' => cursor.hex_char(8),
        _ => None,
    };
    character.map_or_else(|| cursor.error("unknown escape in the string"), Ok)
}

fn literal_string(cursor: &mut Cursor<'_>) -> Result<String, ParseError> {
    cursor.bump();
    let text = cursor.take_until(b"'\n").to_owned();
    if !cursor.eat(b'\'') {
        return cursor.error("the string is not closed");
    }
    Ok(text)
}

fn array(cursor: &mut Cursor<'_>, open: usize) -> Result<Value, Stop> {
    let start = cursor.pos();
    let depth = nest(open, start..start + 1)?;
    cursor.bump();
    let mut items = Vec::new();
    loop {
        skip_trivia(cursor);
        if cursor.eat(b']') {
            break;
        }
        items.push(value(cursor, depth)?);
        skip_trivia(cursor);
        if cursor.eat(b']') {
            break;
        }
        if !cursor.eat(b',') {
            return cursor.error("expected `,` or `]` in the array");
        }
    }
    Ok(Value::Sequence(Sequence {
        items,
        span: start..cursor.pos(),
    }))
}

fn inline_table(cursor: &mut Cursor<'_>, open: usize) -> Result<Value, Stop> {
    let start = cursor.pos();
    let depth = nest(open, start..start + 1)?;
    cursor.bump();
    cursor.skip_spaces();
    let mut pairs = IndexMap::new();
    let mut made = Made::default();
    if !cursor.eat(b'}') {
        loop {
            cursor.skip_spaces();
            entry(cursor, &mut pairs, &mut made, depth)?;
            cursor.skip_spaces();
            if cursor.eat(b'}') {
                break;
            }
            if !cursor.eat(b',') {
                return cursor.error("expected `,` or `}` in the inline table");
            }
        }
    }
    Ok(Value::Mapping(Mapping {
        pairs,
        span: start..cursor.pos(),
    }))
}

/// An unquoted value: a boolean, a number, a date or a time.
fn bare(cursor: &mut Cursor<'_>) -> Result<Value, ParseError> {
    let start = cursor.pos();
    let date_end = token_end(cursor, start);
    if date_end == start {
        return cursor.error("expected a value");
    }
    // A date and its time may be separated by one space instead of `T`.
    let end = if scalar::is_date(cursor.slice(start..date_end)) && time_follows(cursor, date_end) {
        token_end(cursor, date_end + 1)
    } else {
        date_end
    };
    let text = cursor.slice(start..end);
    let Some(value) = scalar::toml(text) else {
        let message = format!("`{text}` is not a value");
        return Err(ParseError::new(&message, start..end));
    };
    cursor.set_pos(end);
    Ok(Value::Scalar(Scalar {
        span: start..end,
        value,
    }))
}

fn token_end(cursor: &Cursor<'_>, from: usize) -> usize {
    let text = cursor.slice(from..cursor.end());
    from + text
        .find([',', ']', '}', '#', ' ', '\t', '\n', '\r'])
        .unwrap_or(text.len())
}

fn time_follows(cursor: &Cursor<'_>, at: usize) -> bool {
    let bytes = cursor.slice(at..cursor.end()).as_bytes();
    matches!(bytes, [b' ', hour_tens, hour_units, b':', ..] if hour_tens.is_ascii_digit() && hour_units.is_ascii_digit())
}

fn skip_trivia(cursor: &mut Cursor<'_>) {
    loop {
        match cursor.peek() {
            Some(b' ' | b'\t' | b'\n' | b'\r') => cursor.bump(),
            Some(b'#') => skip_comment(cursor),
            Some(_) | None => break,
        }
    }
}

fn skip_comment(cursor: &mut Cursor<'_>) {
    let _: &str = cursor.take_until(b"\n");
}

fn end_of_line(cursor: &mut Cursor<'_>) -> Result<(), ParseError> {
    cursor.skip_spaces();
    if cursor.peek() == Some(b'#') {
        skip_comment(cursor);
    }
    match cursor.peek() {
        None | Some(b'\n' | b'\r') => Ok(()),
        Some(_) => cursor.error("expected the end of the line after the value"),
    }
}
