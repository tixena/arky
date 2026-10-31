//! YAML frontmatter: block and flow collections, block scalars, tags,
//! anchors and aliases, and plain scalars into a mapping.

use core::mem;
use core::ops::Range;
use std::collections::HashMap;

use indexmap::IndexMap;

use crate::parser::cursor::{self, Dropped, Mark};
use crate::parser::error::ParseError;
use crate::parser::frontmatter::{Stop, caught, finish, insert, nest, undropped};
use crate::parser::located::{Mapping, Pair, Scalar, ScalarValue, Sequence, Value};
use crate::parser::scalar;

/// How many values the anchors and aliases of one frontmatter may copy:
/// nested aliases grow a copy exponentially.
const COPY_CAP: usize = 100_000;

/// How many bytes of text the anchors and aliases of one frontmatter may
/// copy: an alias of a long string copies all of it.
const TEXT_CAP: usize = 1_000_000;

/// The core-schema tags arky reads, plus `!!timestamp`.
const TAGS: [&str; 8] = [
    "bool",
    "float",
    "int",
    "map",
    "null",
    "seq",
    "str",
    "timestamp",
];

/// The cursor of the YAML reader.
type Cursor<'src> = cursor::Cursor<'src, State>;

/// A key's text, or where a key that is a list or a mapping is written: TOML
/// cannot hold such a key, so its pair is dropped.
type Key = Result<String, Range<usize>>;

/// What a block scalar keeps of the line break after its text and of the
/// empty lines after that.
#[derive(Clone, Copy)]
enum Chomping {
    /// The line break only.
    Clip,
    /// The line break and every empty line.
    Keep,
    /// Neither.
    Strip,
}

/// Where a value is written: after a key or in a list entry whose `-` is
/// at the given column, or inside a flow collection whose lines must be
/// indented more than the given column, if any.
#[derive(Clone, Copy)]
enum Context {
    Entry(usize),
    Flow(Option<usize>),
    Key(usize),
}

impl Context {
    /// The column the lines of a value written here must be indented more
    /// than; none at the root of the frontmatter.
    const fn column(self) -> Option<usize> {
        match self {
            Self::Entry(column) | Self::Key(column) => Some(column),
            Self::Flow(column) => column,
        }
    }
}

/// One pair as read: kept under its key, or dropped with the error of a key
/// that is a list or a mapping.
enum Read {
    Dropped(ParseError),
    Kept(String, Pair),
}

impl Read {
    /// The pair from `start` to the end of `value`, kept under `key`.
    fn new(key: Key, start: usize, value: Value) -> Self {
        match key {
            Ok(name) => {
                let pair = Pair {
                    repeats: Vec::new(),
                    span: start..value.span().end,
                    value,
                };
                Self::Kept(name, pair)
            }
            Err(span) => {
                let message = "a key cannot be a list or a mapping";
                Self::Dropped(ParseError::new(message, span))
            }
        }
    }
}

/// The values anchors name, the anchors still being read, and how much the
/// anchors and aliases have copied.
#[derive(Default)]
struct Anchors {
    copied: Size,
    named: HashMap<String, Value>,
    open: Vec<String>,
}

impl Anchors {
    /// Adds `size` to what the anchors and aliases copied, or raises the drop
    /// of the anchor or the alias at `span` when that passes `COPY_CAP` or
    /// `TEXT_CAP`.
    fn charge(&mut self, size: Size, span: &Range<usize>) -> Result<(), Stop> {
        self.copied.bytes += size.bytes;
        self.copied.values += size.values;
        let message = if self.copied.values > COPY_CAP {
            format!("the anchors and aliases copy more than {COPY_CAP} values")
        } else if self.copied.bytes > TEXT_CAP {
            format!("the anchors and aliases copy more than {TEXT_CAP} bytes of text")
        } else {
            return Ok(());
        };
        Err(Stop::Drop(ParseError::new(&message, span.clone())))
    }

    /// The value the anchor `name` names, or the drop of its alias at `span`:
    /// the alias lies inside that value, or no such anchor comes before it.
    fn get(&self, name: &str, span: &Range<usize>) -> Result<&Value, Stop> {
        if let Some(value) = self.named.get(name) {
            return Ok(value);
        }
        let message = if self.open.iter().any(|open| open == name) {
            format!("the alias `*{name}` lies inside the value its anchor names")
        } else {
            format!("no anchor `&{name}` is defined before this alias")
        };
        Err(Stop::Drop(ParseError::new(&message, span.clone())))
    }

    /// What the anchors and aliases may still copy.
    const fn left(&self) -> Size {
        Size {
            bytes: TEXT_CAP.saturating_sub(self.copied.bytes),
            values: COPY_CAP.saturating_sub(self.copied.values),
        }
    }

    /// Makes `anchor` name `value`, or nothing, and gives the change that
    /// undoes it.
    fn set(&mut self, anchor: String, value: Option<Value>) -> Change {
        let earlier = match value {
            Some(named) => self.named.insert(anchor.clone(), named),
            None => self.named.remove(&anchor),
        };
        Change { anchor, earlier }
    }
}

/// A change to what an anchor names: the anchor, and what it named before.
struct Change {
    anchor: String,
    earlier: Option<Value>,
}

/// The anchor and the tag of a node, at most one of each in either order,
/// where the first of them is written, and where the last ends.
struct Properties {
    anchor: Option<String>,
    end: usize,
    first: Range<usize>,
    tag: Option<Tag>,
}

impl Properties {
    /// The depth inside `open` values that a collection after these opens
    /// in: they count as a level of their own.
    fn level(&self, open: usize) -> Result<usize, Stop> {
        let at = self.first.start;
        nest(open, at..at + 1)
    }

    const fn new(at: usize) -> Self {
        Self {
            anchor: None,
            end: at,
            first: at..at,
            tag: None,
        }
    }

    /// Reads the properties at the cursor, each followed by what `separate`
    /// skips. A second anchor or tag, or an alias after them, is refused.
    fn read<F>(&mut self, cursor: &mut Cursor<'_>, mut separate: F) -> Result<(), ParseError>
    where
        F: FnMut(&mut Cursor<'_>) -> Result<(), ParseError>,
    {
        loop {
            let at = cursor.pos();
            match cursor.peek() {
                Some(b'!') if self.tag.is_some() => return Err(second_tag(at)),
                Some(b'&') if self.anchor.is_some() => return Err(second_anchor(at)),
                Some(b'!') => self.tag = Some(tag(cursor)),
                Some(b'&') => self.anchor = Some(anchor_name(cursor)?),
                Some(b'*') if !self.first.is_empty() => {
                    let first = self.first.clone();
                    return Err(ParseError::new("an alias holds no tag or anchor", first));
                }
                Some(_) | None => return Ok(()),
            }
            self.end = cursor.pos();
            if self.first.is_empty() {
                self.first = at..self.end;
            }
            separate(cursor)?;
        }
    }

    /// Where the properties are written, from the first to the end of the
    /// last.
    const fn span(&self) -> Range<usize> {
        self.first.start..self.end
    }
}

/// What the YAML reader keeps while it reads.
#[derive(Default)]
struct State {
    anchors: Anchors,
    /// The error of the first entry that does not read, at any depth: the
    /// error a frontmatter none of whose own entries reads fails with.
    failure: Option<ParseError>,
    /// Whether an entry of the frontmatter's own mapping reads.
    read: bool,
}

impl cursor::Undo for State {
    type Change = Change;

    fn undo(&mut self, change: Change) {
        self.anchors.set(change.anchor, change.earlier);
    }
}

/// How much a copy holds: its values, each list, mapping and scalar counting
/// as one, and the bytes of the text of its keys and scalars.
#[derive(Clone, Copy, Default)]
struct Size {
    bytes: usize,
    values: usize,
}

/// A tag as written: the type it names, `None` when arky does not read
/// it, and where it is written.
struct Tag {
    name: Option<&'static str>,
    span: Range<usize>,
}

pub fn parse(source: &str, content: Range<usize>) -> Result<Mapping, ParseError> {
    read(source, content).and_then(undropped)
}

/// The mapping [`parse`] reads, with each entry and key it dropped, in file
/// order. A frontmatter none of whose own entries reads fails with the error
/// of the first entry, at any depth, that does not read.
pub fn read(source: &str, content: Range<usize>) -> Result<(Mapping, Vec<Dropped>), ParseError> {
    let mut cursor = Cursor::new(source, content, State::default());
    let mapping = root(&mut cursor)?;
    let mut dropped = cursor.into_dropped();
    dropped.sort_by_key(|entry| entry.span.start);
    Ok((mapping, dropped))
}

/// The frontmatter's own mapping, from the start of its content: a block
/// mapping, a flow mapping, or one value, which holds no key.
fn root(cursor: &mut Cursor<'_>) -> Result<Mapping, ParseError> {
    let start = cursor.mark();
    let Some(indent) = next_line(cursor) else {
        return Ok(finish(IndexMap::new(), start.pos));
    };
    let line_start = cursor.pos();
    cursor.set_pos(line_start + indent);
    let tabbed = no_tab_before_block(cursor, 0);
    // A tab after the indentation separates a flow node from it.
    cursor.skip_spaces();
    let root = cursor.pos();
    match cursor.peek() {
        Some(b'{') if !starts_mapping(cursor, 0) => {
            let mapping = match flow_mapping(cursor, None, 0) {
                Ok(mapping) => mapping,
                Err(stop) => {
                    return Err(cursor
                        .state_mut()
                        .failure
                        .take()
                        .unwrap_or_else(|| stop.into()));
                }
            };
            let rest = end_of_line(cursor).and_then(|()| match next_line(cursor) {
                Some(_) => cursor.error("expected the end of the frontmatter after the mapping"),
                None => Ok(()),
            });
            settled(cursor, mapping, rest)
        }
        Some(b'[') if !starts_mapping(cursor, 0) => {
            let read = flow_sequence(cursor, None, 0);
            let list = caught(cursor, root, read, skip_collection);
            cursor.forget_since(start);
            match list {
                Ok(value) if cursor.state().failure.is_none() => {
                    only_value(cursor, root, value.ok())
                }
                Ok(_) | Err(_) => Err(not_a_mapping(&to_last_text(cursor, root))),
            }
        }
        Some(_) | None => {
            cursor.set_pos(line_start);
            let mapping = block_mapping(cursor, indent, 0);
            let state = cursor.state_mut();
            if !state.read
                && let Some(error) = state.failure.take()
            {
                cursor.forget_since(start);
                tabbed?;
                cursor.set_pos(root);
                let lone = lone_value(cursor, indent);
                cursor.forget_since(start);
                return lone.unwrap_or(Err(error));
            }
            let rest = match next_line(cursor) {
                Some(_) => cursor.error("this line is indented less than the first key"),
                None => Ok(()),
            };
            settled(cursor, mapping, rest)
        }
    }
}

/// The frontmatter's own mapping once its entries are read, `rest` telling
/// whether the text after them reads. With no entry that reads, it fails as
/// the first entry that does not; else text after them that does not is dropped.
fn settled(
    cursor: &mut Cursor<'_>,
    mapping: Mapping,
    rest: Result<(), ParseError>,
) -> Result<Mapping, ParseError> {
    let state = cursor.state_mut();
    if !state.read {
        return state
            .failure
            .take()
            .map_or_else(|| rest.map(|()| mapping), Err);
    }
    if let Err(error) = rest {
        cursor.set_pos(error.span.start);
        let mark = cursor.mark();
        cursor.set_pos(cursor.end());
        cursor.drop_since(mark, error);
    }
    Ok(mapping)
}

/// Records how an entry inside `open` values read, `failure` holding the
/// error of one that does not: the first such error, and whether an entry of
/// the frontmatter's own mapping, inside no value, reads.
fn settle(cursor: &mut Cursor<'_>, open: usize, failure: Option<&ParseError>) {
    let state = cursor.state_mut();
    match failure {
        Some(error) => {
            let _: &mut ParseError = state.failure.get_or_insert_with(|| error.clone());
        }
        None => state.read |= open == 0,
    }
}

/// The frontmatter whose content is the value at the cursor, which fills its
/// line as a scalar or a list does: see [`only_value`]. `None` when the line
/// holds a key and does not read, an entry inside it included, or raises a drop.
fn lone_value(cursor: &mut Cursor<'_>, indent: usize) -> Option<Result<Mapping, ParseError>> {
    let start = cursor.pos();
    let is_list = is_dash(cursor, 0);
    let is_value = is_list || !holds_key(cursor);
    let read = if is_list {
        block_sequence(cursor, start - indent, indent, 0)
    } else {
        inline_value(cursor, Context::Key(indent), 0).and_then(|value| {
            end_of_line(cursor)?;
            Ok(value)
        })
    };
    let value = caught(cursor, start, read, |inner| skip_block(inner, indent));
    let failed = cursor.state().failure.is_some();
    match value {
        Ok(Ok(lone)) if !failed => Some(only_value(cursor, start, Some(lone))),
        Ok(Err(_)) if !failed => is_value.then(|| only_value(cursor, start, None)),
        Ok(_) | Err(_) => is_value.then(|| Err(not_a_mapping(&to_last_text(cursor, start)))),
    }
}

/// The frontmatter whose content is one value, read from `start` up to the
/// cursor: `value`, or `None` when it raised a drop. With nothing after it,
/// that is an empty mapping spanning the value, as no key holds it.
fn only_value(
    cursor: &mut Cursor<'_>,
    start: usize,
    value: Option<Value>,
) -> Result<Mapping, ParseError> {
    let span = value.map_or_else(|| to_last_text(cursor, start), |read| read.span().clone());
    if end_of_line(cursor).is_err() || next_line(cursor).is_some() {
        return Err(not_a_mapping(&span));
    }
    Ok(Mapping {
        pairs: IndexMap::new(),
        span,
    })
}

/// Whether the line at the cursor holds a key: it starts a mapping, or a
/// `key:` follows an anchor, a tag or an indicator on it.
fn holds_key(cursor: &mut Cursor<'_>) -> bool {
    starts_mapping(cursor, 0) || holds_pair(uncommented(cursor.rest_of_line()).trim_end())
}

/// From `start` to the end of the frontmatter's last line of text.
fn to_last_text(cursor: &Cursor<'_>, start: usize) -> Range<usize> {
    let text = cursor
        .slice(start..cursor.end())
        .trim_end_matches([' ', '\t', '\r', '\n']);
    start..start + text.len()
}

fn not_a_mapping(span: &Range<usize>) -> ParseError {
    ParseError::new("the frontmatter must be a mapping", span.clone())
}

/// Skips blank lines and comment lines. Leaves the cursor at the start of
/// the next line with content and returns that line's indentation.
fn next_line(cursor: &mut Cursor<'_>) -> Option<usize> {
    loop {
        if cursor.at_end() {
            return None;
        }
        let line = cursor.rest_of_line();
        let content = line.trim_start_matches(' ');
        let text = content.trim_start_matches('\t');
        if !text.trim_end().is_empty() && !text.starts_with('#') {
            return Some(line.len() - content.len());
        }
        let _: &str = cursor.take_until(b"\n");
        cursor.bump();
    }
}

/// The pairs written at exactly `indent`, starting at the cursor's line: a
/// mapping `depth` values deep.
fn block_mapping(cursor: &mut Cursor<'_>, indent: usize, depth: usize) -> Mapping {
    let empty_at = cursor.pos() + indent;
    let mut pairs = IndexMap::new();
    mapping_lines(cursor, indent, depth, &mut pairs);
    finish(pairs, empty_at)
}

/// The pairs on the lines from the cursor on that are written at exactly
/// `indent`, of a mapping `depth` values deep. A pair that does not read is
/// dropped, and so is a line there that starts a list entry or a tab.
fn mapping_lines(
    cursor: &mut Cursor<'_>,
    indent: usize,
    depth: usize,
    pairs: &mut IndexMap<String, Pair>,
) {
    while let Some(found) = next_line(cursor) {
        if found < indent {
            break;
        }
        cursor.set_pos(cursor.pos() + indent);
        let mark = cursor.mark();
        let read = if cursor.peek() == Some(b'\t') {
            Err(tab_indent(mark.pos).into())
        } else if is_dash(cursor, 0) {
            cursor.error("expected a key, found a list entry")
        } else {
            entry(cursor, indent, depth, pairs)
        };
        settle_pair(cursor, mark, indent, depth, read);
    }
}

/// Settles what `read` gave for the pair of a block mapping that starts at
/// `mark`, its key at column `indent`, inside `open` values: one that does not
/// read or raises a drop is noted with its error, and the cursor passes it.
fn settle_pair(
    cursor: &mut Cursor<'_>,
    mark: Mark,
    indent: usize,
    open: usize,
    read: Result<(), Stop>,
) {
    let failure = match &read {
        Err(Stop::Fail(error)) => Some(error),
        Ok(()) | Err(Stop::Drop(_)) => None,
    };
    settle(cursor, open, failure);
    if let Err(stop) = read {
        cursor.set_pos(mark.pos);
        skip_block(cursor, indent);
        cursor.drop_since(mark, stop.into());
    }
}

/// Adds the pair `read` gives to `pairs`. A dropped pair, which starts at
/// `mark`, adds nothing: its error is noted in place of the drops noted
/// inside it.
fn add(cursor: &mut Cursor<'_>, pairs: &mut IndexMap<String, Pair>, mark: Mark, read: Read) {
    match read {
        Read::Dropped(error) => cursor.drop_since(mark, error),
        Read::Kept(name, pair) => insert(pairs, name, pair),
    }
}

/// A mapping `depth` values deep whose first key is written on a list
/// entry's line, at `column`; its other keys follow on lines indented to that
/// column.
fn entry_mapping(cursor: &mut Cursor<'_>, column: usize, depth: usize) -> Mapping {
    let mark = cursor.mark();
    let mut pairs = IndexMap::new();
    let read = entry(cursor, column, depth, &mut pairs);
    settle_pair(cursor, mark, column, depth, read);
    mapping_lines(cursor, column, depth, &mut pairs);
    finish(pairs, mark.pos)
}

/// One pair of a block mapping whose keys are at column `indent`, inside
/// `open` values, added to `pairs`: a `key:`, a `:` with the empty key, or a
/// `?`, then its value, up to the next line at that column or less.
fn entry(
    cursor: &mut Cursor<'_>,
    indent: usize,
    open: usize,
    pairs: &mut IndexMap<String, Pair>,
) -> Result<(), Stop> {
    let mark = cursor.mark();
    let read = if is_explicit_key(cursor) {
        explicit_entry(cursor, indent, open)?
    } else {
        let name = if is_empty_key(cursor, false) {
            Ok(String::new())
        } else {
            key_or_collection(cursor, Context::Key(indent), open, key)?
        };
        cursor.skip_spaces();
        if !cursor.eat(b':') {
            return cursor.error("expected `:` after the key");
        }
        if !separated_at(cursor, 0) {
            return cursor.error("expected a space after the `:`");
        }
        Read::new(name, mark.pos, indicated_value(cursor, indent, open)?)
    };
    if next_line(cursor).is_some_and(|found| found > indent) {
        return cursor.error("this line is indented more than the keys above it");
    }
    add(cursor, pairs, mark, read);
    Ok(())
}

/// A pair whose key follows a `?` at column `indent`, inside `open` values;
/// the `?` counts as a level of its own. Its value follows a `:` at that
/// column on the next line with content; with none, it is null at the end of
/// the key. Both lines are read as a list entry's `-` line is.
fn explicit_entry(cursor: &mut Cursor<'_>, indent: usize, open: usize) -> Result<Read, Stop> {
    let start = cursor.pos();
    let depth = nest(open, start..start + 1)?;
    let key = compact_value(cursor, start - indent, indent, Context::Key(indent), depth)?;
    let key_end = key.span().end;
    let name = explicit_key(cursor, key);
    let value = if next_line(cursor) == Some(indent)
        && cursor.peek_at(indent) == Some(b':')
        && separated_at(cursor, indent + 1)
    {
        let line_start = cursor.pos();
        compact_value(cursor, line_start, indent, Context::Key(indent), depth)?
    } else {
        null(key_end)
    };
    Ok(Read::new(name, start, value))
}

/// Moves the cursor from the start of a pair whose key is at column `indent`
/// past it: the rest of its line, then the lines indented more and the list
/// entries at that column, and after `?` the line of its `:` value.
fn skip_block(cursor: &mut Cursor<'_>, indent: usize) {
    let mut value_line = is_explicit_key(cursor);
    loop {
        let _: &str = cursor.take_until(b"\n");
        cursor.bump();
        let Some(found) = next_line(cursor) else {
            return;
        };
        let at_key = found == indent;
        let colon = value_line
            && at_key
            && cursor.peek_at(found) == Some(b':')
            && separated_at(cursor, found + 1);
        if found < indent || (at_key && !colon && !is_dash(cursor, found)) {
            return;
        }
        value_line &= !colon;
    }
}

/// The text of an explicit key, or where a list or a mapping is written. A
/// copy an alias made has the text of the scalar its anchor names.
fn explicit_key(cursor: &Cursor<'_>, key: Value) -> Key {
    let scalar = match key {
        Value::Scalar(scalar) => scalar,
        Value::Mapping(_) | Value::Sequence(_) => return Err(key.span().clone()),
    };
    let copied = cursor
        .slice(scalar.span.clone())
        .strip_prefix('*')
        .and_then(|name| cursor.state().anchors.named.get(name));
    if let Some(Value::Scalar(original)) = copied {
        return Ok(scalar_text(cursor, original.value.clone(), &original.span));
    }
    Ok(scalar_text(cursor, scalar.value, &scalar.span))
}

/// The key at the cursor, read by `text`, or where a key that is a list or a
/// mapping is written, with properties or not, read as a value written in
/// `context` inside `open` values.
fn key_or_collection<F>(
    cursor: &mut Cursor<'_>,
    context: Context,
    open: usize,
    text: F,
) -> Result<Key, Stop>
where
    F: FnOnce(&mut Cursor<'_>) -> Result<Key, Stop>,
{
    if !is_collection_key(cursor) {
        return text(cursor);
    }
    let start = cursor.pos();
    let _: Value = inline_value(cursor, context, open)?;
    Ok(Err(start..cursor.pos()))
}

/// Whether a `[` or a `{` starts the node at the cursor, after properties or
/// not.
fn is_collection_key(cursor: &mut Cursor<'_>) -> bool {
    let start = cursor.pos();
    skip_properties(cursor);
    let is_collection = matches!(cursor.peek(), Some(b'[' | b'{'));
    cursor.set_pos(start);
    is_collection
}

/// Moves the cursor past the tags and anchors at it, whether arky reads
/// them or not, and the spaces after each.
fn skip_properties(cursor: &mut Cursor<'_>) {
    loop {
        match cursor.peek() {
            Some(b'!') => pass_tag(cursor),
            Some(b'&') => {
                let _: &str = pass_name(cursor);
            }
            Some(_) | None => return,
        }
        cursor.skip_spaces();
    }
}

/// The value after the `:` of a key at column `indent`, with the cursor
/// right after the `:`: the rest of the line, what the lines under it hold,
/// or null right after the `:`.
fn indicated_value(cursor: &mut Cursor<'_>, indent: usize, open: usize) -> Result<Value, Stop> {
    let after = cursor.pos();
    cursor.skip_spaces();
    if at_line_end(cursor) {
        end_of_line(cursor)?;
        return nested(cursor, Context::Key(indent), after, open);
    }
    let value = inline_value(cursor, Context::Key(indent), open)?;
    end_of_line(cursor)?;
    Ok(value)
}

/// The key at the cursor, or where an alias that names a list or a mapping
/// is written.
fn key(cursor: &mut Cursor<'_>) -> Result<Key, Stop> {
    match cursor.peek() {
        Some(b'!' | b'&') => propertied_key(cursor, false, scalar_key).map(Ok),
        Some(b'*') => alias_key(cursor),
        Some(_) | None => scalar_key(cursor).map(Ok),
    }
}

/// A quoted or plain key.
fn scalar_key(cursor: &mut Cursor<'_>) -> Result<String, Stop> {
    match cursor.peek() {
        Some(b'"') => double_quoted(cursor, None).map_err(Stop::Fail),
        Some(b'\'') => single_quoted(cursor, None).map_err(Stop::Fail),
        Some(b'[' | b'{') => cursor.error("expected a key"),
        Some(_) | None => {
            no_indicator(cursor, false, "key")?;
            let line = cursor.rest_of_line();
            let Some(length) = plain_key_length(line) else {
                return cursor.error("expected `key: value`");
            };
            cursor.set_pos(cursor.pos() + length);
            Ok(line[..length].trim_end().to_owned())
        }
    }
}

/// A plain key ends at the first `:` followed by a space or the end of the
/// line, before the line's comment.
fn plain_key_length(line: &str) -> Option<usize> {
    let text = uncommented(line);
    text.match_indices(':')
        .map(|(at, _)| at)
        .find(|&at| matches!(text.as_bytes().get(at + 1), None | Some(b' ' | b'\t')))
        .filter(|&at| at > 0)
}

/// Whether the cursor, inside `open` values, is on a pair that opens a
/// mapping: an explicit key, or an empty key or a `key:` after properties or
/// not, whose key is plain, quoted, a flow collection or an alias. On a tab
/// it is not.
fn starts_mapping(cursor: &mut Cursor<'_>, open: usize) -> bool {
    if is_explicit_key(cursor) {
        return true;
    }
    let start = cursor.pos();
    skip_properties(cursor);
    let is_mapping = is_empty_key(cursor, false)
        || match cursor.peek() {
            Some(b'"' | b'\'') => {
                let is_key = scalar_key(cursor).is_ok();
                cursor.skip_spaces();
                is_key && cursor.peek() == Some(b':')
            }
            // Reading the key checks the indentation of its lines.
            Some(b'[' | b'{') => {
                let is_key = passes_collection(cursor, open);
                cursor.skip_spaces();
                is_key && cursor.peek() == Some(b':') && separated_at(cursor, 1)
            }
            Some(b'*') => {
                let _: &str = pass_name(cursor);
                cursor.skip_spaces();
                cursor.peek() == Some(b':') && separated_at(cursor, 1)
            }
            Some(b'\t' | b'|' | b'>' | b'&' | b'!') => false,
            Some(_) | None => plain_key_length(cursor.rest_of_line()).is_some(),
        };
    cursor.set_pos(start);
    is_mapping
}

/// What `look` gives, read with no anchor named and every drop it notes
/// forgotten, so that a lookahead copies, names and notes nothing.
fn unanchored<T, F>(cursor: &mut Cursor<'_>, look: F) -> T
where
    F: FnOnce(&mut Cursor<'_>) -> T,
{
    let state = mem::take(cursor.state_mut());
    let mark = cursor.mark();
    let seen = look(cursor);
    cursor.forget_since(mark);
    *cursor.state_mut() = state;
    seen
}

/// Whether the flow collection at the cursor, inside `open` values, reads,
/// and moves the cursor past it. One that raises a drop is passed over
/// unread.
fn passes_collection(cursor: &mut Cursor<'_>, open: usize) -> bool {
    let start = cursor.pos();
    let read = unanchored(cursor, |inner| {
        inline_value(inner, Context::Flow(None), open).map(|_| ())
    });
    match read {
        Ok(()) => true,
        Err(Stop::Drop(_)) => {
            cursor.set_pos(start);
            skip_collection(cursor);
            true
        }
        Err(Stop::Fail(_)) => false,
    }
}

/// Whether the cursor is on a `?` that starts an explicit key: one followed
/// by a space, a tab or the end of the line.
fn is_explicit_key(cursor: &Cursor<'_>) -> bool {
    cursor.peek() == Some(b'?') && separated_at(cursor, 1)
}

/// The value of a key, a list entry or properties with nothing after them on
/// their line, inside `open` values: what the lines indented more than it
/// hold, a list at a key's own column, or null.
fn nested(
    cursor: &mut Cursor<'_>,
    context: Context,
    null_at: usize,
    open: usize,
) -> Result<Value, Stop> {
    let Some((found, is_list)) = under(cursor, context) else {
        return Ok(null(null_at));
    };
    let line_start = cursor.pos();
    let at = line_start + found;
    if is_list {
        let depth = nest(open, at..at + 1)?;
        return block_sequence(cursor, line_start, found, depth);
    }
    cursor.set_pos(at);
    no_tab_before_block(cursor, open)?;
    if starts_mapping(cursor, open) {
        let depth = nest(open, at..at + 1)?;
        cursor.set_pos(line_start);
        return Ok(Value::Mapping(block_mapping(cursor, found, depth)));
    }
    // A tab after the indentation separates a flow node from it.
    cursor.skip_spaces();
    line_value(cursor, context, open)
}

/// The indentation of the line that holds the value of a key, a list entry
/// or properties with nothing after them on their line, and whether a list
/// entry starts that line, with the cursor at its start; `None` when the
/// value is null.
fn under(cursor: &mut Cursor<'_>, context: Context) -> Option<(usize, bool)> {
    let found = next_line(cursor)?;
    let is_list = is_dash(cursor, found);
    let compact = is_list && matches!(context, Context::Key(column) if column == found);
    if !compact && context.column().is_some_and(|column| found <= column) {
        return None;
    }
    Some((found, is_list))
}

/// Refuses the tab at the cursor, after the indentation of a line inside
/// `open` values, when a key, an explicit key or a list entry follows it.
fn no_tab_before_block(cursor: &mut Cursor<'_>, open: usize) -> Result<(), ParseError> {
    let tab = cursor.pos();
    if cursor.peek() != Some(b'\t') {
        return Ok(());
    }
    cursor.skip_spaces();
    let starts_block = is_dash(cursor, 0) || starts_mapping(cursor, open);
    cursor.set_pos(tab);
    if starts_block {
        return Err(tab_indent(tab));
    }
    Ok(())
}

/// A tab at `at`, before the first character of a line: a tab is never
/// indentation.
fn tab_indent(at: usize) -> ParseError {
    ParseError::new("a tab cannot indent a line; use spaces", at..at + 1)
}

/// A block list `depth` values deep whose first `-` is at column `indent` of
/// the line starting at `line_start`, and whose other entries are the lines
/// below at that column. An entry that does not read is dropped.
fn block_sequence(
    cursor: &mut Cursor<'_>,
    line_start: usize,
    indent: usize,
    depth: usize,
) -> Result<Value, Stop> {
    let start = line_start + indent;
    let mut entry_line = line_start;
    let mut items = Vec::new();
    loop {
        cursor.set_pos(entry_line + indent);
        let mark = cursor.mark();
        let read = if cursor.peek() == Some(b'\t') {
            Err(tab_indent(mark.pos).into())
        } else {
            compact_value(cursor, entry_line, indent, Context::Entry(indent), depth)
        };
        settle_entry(cursor, mark, entry_line, indent, depth, read, &mut items)?;
        let Some(found) = next_line(cursor) else {
            break;
        };
        let at_dash = is_dash(cursor, found) || cursor.peek_at(found) == Some(b'\t');
        if found < indent || !at_dash {
            break;
        }
        entry_line = cursor.pos();
    }
    let end = items.last().map_or(start, |item| item.span().end);
    Ok(Value::Sequence(Sequence {
        items,
        span: start..end,
    }))
}

/// Settles what `read` gave for the list entry at column `indent` of the
/// line at `line_start`, inside `open` values, into `items`: one that does not
/// read, or that a line indented more follows, is noted from `mark` and passed.
fn settle_entry(
    cursor: &mut Cursor<'_>,
    mark: Mark,
    line_start: usize,
    indent: usize,
    open: usize,
    read: Result<Value, Stop>,
    items: &mut Vec<Value>,
) -> Result<(), Stop> {
    let entry = read.and_then(|item| {
        if next_line(cursor).is_some_and(|found| found > indent) {
            return cursor.error("this line is indented more than the list above it");
        }
        Ok(item)
    });
    match entry {
        Ok(item) => items.push(item),
        Err(Stop::Fail(error)) => {
            settle(cursor, open, Some(&error));
            cursor.set_pos(line_start);
            skip_entry(cursor, indent);
            cursor.drop_since(mark, error);
        }
        Err(drop @ Stop::Drop(_)) => return Err(drop),
    }
    Ok(())
}

/// Moves the cursor from the start of the line of a list entry whose `-` is
/// at column `indent` past it: that line, then the lines indented more.
fn skip_entry(cursor: &mut Cursor<'_>, indent: usize) {
    loop {
        let _: &str = cursor.take_until(b"\n");
        cursor.bump();
        if next_line(cursor).is_none_or(|found| found <= indent) {
            return;
        }
    }
}

/// The value after the `-`, `?` or `:` at column `indent` of the line
/// starting at `line_start`, written in `context` inside `open` values. A `-`
/// after it starts a list at its own column, and a `key:` or a `?` there a
/// mapping, unless a tab comes before them: a tab is never indentation.
fn compact_value(
    cursor: &mut Cursor<'_>,
    line_start: usize,
    indent: usize,
    context: Context,
    open: usize,
) -> Result<Value, Stop> {
    cursor.set_pos(line_start + indent + 1);
    let after_indicator = cursor.pos();
    cursor.skip_spaces();
    let at = cursor.pos();
    let column = at - line_start;
    if at_line_end(cursor) {
        end_of_line(cursor)?;
        return nested(cursor, context, after_indicator, open);
    }
    let indented = !cursor.slice(after_indicator..at).contains('\t');
    if indented && is_dash(cursor, 0) {
        let depth = nest(open, at..at + 1)?;
        return block_sequence(cursor, line_start, column, depth);
    }
    if indented && starts_mapping(cursor, open) {
        let depth = nest(open, at..at + 1)?;
        return Ok(Value::Mapping(entry_mapping(cursor, column, depth)));
    }
    line_value(cursor, context, open)
}

/// The value that starts a line of a block or follows its `-`, `?` or `:`,
/// inside `open` values, up to the end of that line.
fn line_value(cursor: &mut Cursor<'_>, context: Context, open: usize) -> Result<Value, Stop> {
    let value = inline_value(cursor, context, open)?;
    end_of_line(cursor)?;
    Ok(value)
}

/// A value that starts at the cursor, inside `open` values. It ends on its
/// own line, except a flow collection, which may run over several, and a
/// block scalar, which takes the lines under it; either way the cursor stops
/// after its last text.
fn inline_value(cursor: &mut Cursor<'_>, context: Context, open: usize) -> Result<Value, Stop> {
    let start = cursor.pos();
    match cursor.peek() {
        Some(b'[') => flow_sequence(cursor, context.column(), nest(open, start..start + 1)?),
        Some(b'{') => flow_mapping(cursor, context.column(), nest(open, start..start + 1)?)
            .map(Value::Mapping),
        Some(b'"') => {
            let text = double_quoted(cursor, Some(context))?;
            Ok(string(start..cursor.pos(), text))
        }
        Some(b'\'') => {
            let text = single_quoted(cursor, Some(context))?;
            Ok(string(start..cursor.pos(), text))
        }
        Some(b'|' | b'>') => match context {
            Context::Entry(indent) | Context::Key(indent) => {
                block_scalar(cursor, indent).map_err(Stop::Fail)
            }
            Context::Flow(_) => plain(cursor, context).map_err(Stop::Fail),
        },
        Some(b'!' | b'&') => propertied(cursor, context, open),
        Some(b'*') => alias(cursor, open),
        Some(_) | None => plain(cursor, context).map_err(Stop::Fail),
    }
}

/// A flow list `depth` values deep whose lines are indented more than
/// `column`, the column of its key or `-`; none at the root.
fn flow_sequence(
    cursor: &mut Cursor<'_>,
    column: Option<usize>,
    depth: usize,
) -> Result<Value, Stop> {
    let start = cursor.pos();
    cursor.bump();
    let mut items = Vec::new();
    loop {
        skip_flow_space(cursor, column)?;
        if cursor.eat(b']') {
            break;
        }
        no_empty_entry(cursor)?;
        items.push(flow_item(cursor, column, depth)?);
        skip_flow_space(cursor, column)?;
        if cursor.eat(b']') {
            break;
        }
        if !cursor.eat(b',') {
            return cursor.error("expected `,` or `]` in the list");
        }
    }
    Ok(Value::Sequence(Sequence {
        items,
        span: start..cursor.pos(),
    }))
}

/// One entry of a flow list, inside `open` values: a value, or a pair, which
/// makes a mapping of one pair. A list or a mapping is read once, and is the
/// key of a pair when a `:` follows it on its line.
fn flow_item(cursor: &mut Cursor<'_>, column: Option<usize>, open: usize) -> Result<Value, Stop> {
    let mark = cursor.mark();
    if is_collection_key(cursor) {
        let item = inline_value(cursor, Context::Flow(column), open);
        return collection_item(cursor, column, open, mark, item);
    }
    if is_flow_pair(cursor) {
        return one_pair(cursor, column, open, mark, true);
    }
    inline_value(cursor, Context::Flow(column), open)
}

/// The entry of a flow list that `read` gave for the list or the mapping at
/// `mark`, inside `open` values, or the mapping of one pair it is the key of
/// when a `:` follows it on its line.
fn collection_item(
    cursor: &mut Cursor<'_>,
    column: Option<usize>,
    open: usize,
    mark: Mark,
    read: Result<Value, Stop>,
) -> Result<Value, Stop> {
    let item = match read {
        Err(Stop::Drop(error)) => {
            // Passed over, to see whether a `:` makes it a key.
            cursor.set_pos(mark.pos);
            skip_properties(cursor);
            skip_collection(cursor);
            Err(Stop::Drop(error))
        }
        value @ (Ok(_) | Err(Stop::Fail(_))) => value,
    };
    if matches!(item, Err(Stop::Fail(_))) || !colon_follows(cursor) {
        return item;
    }
    one_pair(cursor, column, open, mark, false)
}

/// The mapping of the one pair of a flow list that starts at `mark`, inside
/// `open` values. With `read_key`, its key is read here; otherwise the cursor
/// is after a key that is a list or a mapping.
fn one_pair(
    cursor: &mut Cursor<'_>,
    column: Option<usize>,
    open: usize,
    mark: Mark,
    read_key: bool,
) -> Result<Value, Stop> {
    let start = mark.pos;
    let depth = nest(open, start..start + 1)?;
    let key = if read_key {
        flow_pair_key(cursor, column, true, depth)
    } else {
        Ok(Err(start..cursor.pos()))
    };
    let mut pairs = IndexMap::new();
    let read = match key {
        Ok(name) => flow_pair(cursor, column, depth, mark, name, &mut pairs),
        Err(stop) => Err(stop),
    };
    match read {
        Ok(()) => {}
        Err(Stop::Drop(error)) => {
            cursor.set_pos(start);
            skip_flow_entry(cursor);
            cursor.drop_since(mark, error);
        }
        Err(failure @ Stop::Fail(_)) => return Err(failure),
    }
    Ok(Value::Mapping(finish(pairs, start)))
}

/// Whether a `:` follows the cursor on its line, after spaces or tabs.
fn colon_follows(cursor: &Cursor<'_>) -> bool {
    cursor
        .rest_of_line()
        .trim_start_matches([' ', '\t'])
        .starts_with(':')
}

/// Whether the flow entry at the cursor is a pair: an explicit key, an empty
/// key or a key followed by `:`.
fn is_flow_pair(cursor: &mut Cursor<'_>) -> bool {
    if is_flow_explicit_key(cursor) || is_empty_key(cursor, true) {
        return true;
    }
    let start = cursor.pos();
    // A key's tag that arky does not read raises a drop after the key, and
    // so does an alias with no anchor.
    let is_key = unanchored(cursor, |inner| {
        !matches!(flow_key(inner, None), Err(Stop::Fail(_)))
    });
    cursor.skip_spaces();
    let is_pair = is_key && cursor.peek() == Some(b':');
    cursor.set_pos(start);
    is_pair
}

/// Refuses a `,` where an entry of a flow collection should start.
fn no_empty_entry(cursor: &Cursor<'_>) -> Result<(), ParseError> {
    if cursor.peek() == Some(b',') {
        let at = cursor.pos();
        return Err(ParseError::new("expected a value before `,`", at..at + 1));
    }
    Ok(())
}

/// A flow mapping `depth` values deep whose lines are indented more than
/// `column`, the column of its key or `-`; none at the root. A pair that
/// raises a drop or does not read is dropped by [`settle_flow_pair`], and so
/// is text where a `,` or a `}` is expected, with the pair before it when
/// that pair starts on the text's line.
fn flow_mapping(
    cursor: &mut Cursor<'_>,
    column: Option<usize>,
    depth: usize,
) -> Result<Mapping, Stop> {
    let start = cursor.pos();
    cursor.bump();
    let mut pairs = IndexMap::new();
    loop {
        skip_flow_space(cursor, column)?;
        if cursor.eat(b'}') {
            break;
        }
        no_empty_entry(cursor)?;
        let mark = cursor.mark();
        // Held back until the text after it keeps it, as a repeated key merges.
        let mut read_pair = IndexMap::new();
        let read = match flow_pair_key(cursor, column, false, depth) {
            Ok(name) => flow_pair(cursor, column, depth, mark, name, &mut read_pair),
            Err(stop) => Err(stop),
        };
        if !settle_flow_pair(cursor, mark, column, depth, read)? {
            continue;
        }
        skip_flow_space(cursor, column)?;
        let separated = matches!(cursor.peek(), Some(b',' | b'}'));
        let with_pair = !separated && !cursor.slice(mark.pos..cursor.pos()).contains('\n');
        if !with_pair {
            for (name, pair) in read_pair {
                insert(&mut pairs, name, pair);
            }
        }
        if cursor.eat(b'}') {
            break;
        }
        if !cursor.eat(b',') {
            let broken = if with_pair { mark } else { cursor.mark() };
            let failure = cursor.error("expected `,` or `}` in the mapping");
            let _: bool = settle_flow_pair(cursor, broken, column, depth, failure)?;
        }
    }
    Ok(Mapping {
        pairs,
        span: start..cursor.pos(),
    })
}

/// Settles the flow pair `read` gave from `mark`, as [`settle_pair`] does, and
/// tells whether a `,` or a `}` follows: one that does not read runs to the
/// mapping's next line that starts no further right, and fails the mapping
/// when there is none or when the mapping closes on the pair's line.
fn settle_flow_pair(
    cursor: &mut Cursor<'_>,
    mark: Mark,
    column: Option<usize>,
    open: usize,
    read: Result<(), Stop>,
) -> Result<bool, Stop> {
    let error = match read {
        Ok(()) => {
            settle(cursor, open, None);
            return Ok(true);
        }
        Err(Stop::Drop(error)) => {
            settle(cursor, open, None);
            cursor.set_pos(mark.pos);
            skip_flow_entry(cursor);
            cursor.drop_since(mark, error);
            return Ok(true);
        }
        Err(Stop::Fail(error)) => error,
    };
    if closes_on_its_line(cursor, mark.pos) {
        return Err(Stop::Fail(error));
    }
    let Some(next) = next_pair_line(cursor, mark.pos, column) else {
        return Err(Stop::Fail(error));
    };
    settle(cursor, open, Some(&error));
    cursor.set_pos(next);
    cursor.drop_since(mark, error);
    Ok(false)
}

/// The start of the first line with text after the one at `at` that starts
/// no further right than `at` and, inside a flow collection whose lines are
/// indented more than `column`, further right than that.
fn next_pair_line(cursor: &mut Cursor<'_>, at: usize, column: Option<usize>) -> Option<usize> {
    let line_start = cursor.slice(0..at).rfind('\n').map_or(0, |end| end + 1);
    let own = at - line_start;
    cursor.set_pos(at);
    loop {
        let _: &str = cursor.take_until(b"\n");
        cursor.bump();
        let found = next_line(cursor)?;
        if column.is_some_and(|parent| found <= parent) {
            return None;
        }
        if found <= own {
            return Some(cursor.pos());
        }
    }
}

/// Whether the flow mapping holding the entry at `at` closes on that entry's line.
fn closes_on_its_line(cursor: &mut Cursor<'_>, at: usize) -> bool {
    cursor.set_pos(at);
    loop {
        skip_flow_entry(cursor);
        let closed = cursor.peek() == Some(b'}');
        if cursor.slice(at..cursor.pos()).contains('\n') {
            return false;
        }
        if closed || !cursor.eat(b',') {
            return closed;
        }
    }
}

/// The rest of a flow pair that starts at `mark`, inside `open` values, after
/// its key. Line breaks and comments may come before its `:`; with no `:`,
/// or with nothing after it, the value is null.
fn flow_pair(
    cursor: &mut Cursor<'_>,
    column: Option<usize>,
    open: usize,
    mark: Mark,
    key: Key,
    pairs: &mut IndexMap<String, Pair>,
) -> Result<(), Stop> {
    let key_end = cursor.pos();
    skip_flow_space(cursor, column)?;
    let value = if cursor.eat(b':') {
        let after_colon = cursor.pos();
        skip_flow_space(cursor, column)?;
        if matches!(cursor.peek(), Some(b',' | b'}' | b']')) {
            null(after_colon)
        } else {
            inline_value(cursor, Context::Flow(column), open)?
        }
    } else {
        null(key_end)
    };
    add(cursor, pairs, mark, Read::new(key, mark.pos, value));
    Ok(())
}

/// Moves the cursor from the start of a flow entry that is dropped to its
/// end: the first `,`, `]` or `}` outside every bracket the entry opens.
/// Quoted scalars, properties, aliases and comments are passed over whole.
fn skip_flow_entry(cursor: &mut Cursor<'_>) {
    let mut depth = 0_usize;
    // Whether a node may start here, as a quoted scalar, a property or an
    // alias may.
    let mut node_start = true;
    let mut spaced = true;
    loop {
        let Some(byte) = cursor.peek() else {
            return;
        };
        match byte {
            b',' | b']' | b'}' if depth == 0 => return,
            b'"' | b'\'' if node_start => {
                let context = Some(Context::Flow(None));
                let _: Result<String, ParseError> = if byte == b'"' {
                    double_quoted(cursor, context)
                } else {
                    single_quoted(cursor, context)
                };
                node_start = false;
                spaced = false;
                continue;
            }
            b'!' if node_start => {
                pass_tag(cursor);
                spaced = false;
                continue;
            }
            b'&' if node_start => {
                let _: &str = pass_name(cursor);
                spaced = false;
                continue;
            }
            b'*' if node_start => {
                let _: &str = pass_name(cursor);
                node_start = false;
                spaced = false;
                continue;
            }
            b'#' if spaced => {
                let _: &str = cursor.take_until(b"\n");
                continue;
            }
            b'[' | b'{' | b',' => node_start = true,
            b']' | b'}' => {
                depth -= 1;
                node_start = false;
            }
            b':' | b'?' if is_indicator(cursor, true) => node_start = true,
            b' ' | b'\t' | b'\r' | b'\n' => {}
            _ => node_start = false,
        }
        if matches!(byte, b'[' | b'{') {
            depth += 1;
        }
        spaced = matches!(byte, b' ' | b'\t' | b'\r' | b'\n');
        cursor.bump();
    }
}

/// Moves the cursor past the flow collection at it, entry by entry as
/// [`skip_flow_entry`] passes over one.
fn skip_collection(cursor: &mut Cursor<'_>) {
    cursor.bump();
    loop {
        skip_flow_entry(cursor);
        if !cursor.eat(b',') {
            cursor.bump();
            return;
        }
    }
}

/// The key of a flow pair inside `open` values, or where a list or a mapping
/// or an alias that names one is written; empty after a `?` with none or before
/// a `:` that starts the pair.
/// It goes on over lines in a flow mapping or after a `?`, else stays on one.
fn flow_pair_key(
    cursor: &mut Cursor<'_>,
    column: Option<usize>,
    in_list: bool,
    open: usize,
) -> Result<Key, Stop> {
    let explicit = is_flow_explicit_key(cursor);
    if explicit {
        cursor.bump();
        skip_flow_space(cursor, column)?;
    }
    if is_empty_key(cursor, true)
        || (explicit && matches!(cursor.peek(), None | Some(b',' | b']' | b'}')))
    {
        return Ok(Ok(String::new()));
    }
    let context = Context::Flow(column);
    key_or_collection(cursor, context, open, |inner| {
        flow_key(inner, (explicit || !in_list).then_some(context))
    })
}

/// Whether the cursor is on a `?` that starts an explicit key inside a flow
/// collection: one followed by a space, a tab, the end of the line, `,`,
/// `]` or `}`.
fn is_flow_explicit_key(cursor: &Cursor<'_>) -> bool {
    cursor.peek() == Some(b'?')
        && (separated_at(cursor, 1) || matches!(cursor.peek_at(1), Some(b',' | b']' | b'}')))
}

/// Whether a pair starts at the cursor with its `:`, with the empty key,
/// inside a flow collection or not.
fn is_empty_key(cursor: &Cursor<'_>, in_flow: bool) -> bool {
    cursor.peek() == Some(b':') && is_indicator(cursor, in_flow)
}

/// A key inside a flow collection, or where an alias that names a list or a
/// mapping is written. A plain key ends at a `:` followed by a space or a flow
/// indicator, at a flow indicator, or at a comment. With a `context`, as
/// inside a flow mapping, a quoted key may go on over several lines as a
/// quoted value does, and a plain key that runs to the end of its line goes on
/// over the lines below as a `PLAIN` does, up to that end.
fn flow_key(cursor: &mut Cursor<'_>, context: Option<Context>) -> Result<Key, Stop> {
    match cursor.peek() {
        Some(b'!' | b'&') => {
            propertied_key(cursor, true, |inner| flow_scalar_key(inner, context)).map(Ok)
        }
        Some(b'*') => alias_key(cursor),
        Some(_) | None => flow_scalar_key(cursor, context).map(Ok),
    }
}

/// A quoted or plain key inside a flow collection, read as [`flow_key`]
/// reads one.
fn flow_scalar_key(cursor: &mut Cursor<'_>, context: Option<Context>) -> Result<String, Stop> {
    match cursor.peek() {
        Some(b'"') => double_quoted(cursor, context).map_err(Stop::Fail),
        Some(b'\'') => single_quoted(cursor, context).map_err(Stop::Fail),
        Some(_) | None => {
            no_indicator(cursor, true, "key")?;
            let start = cursor.pos();
            let line = cursor.rest_of_line();
            let key = flow_key_part(line);
            let text = key.trim_end();
            if text.is_empty() {
                return cursor.error("expected a key");
            }
            let first_end = start + text.len();
            let end = match context {
                Some(within) if key.len() == line.len() => {
                    continued_end(cursor, first_end, within, |content| {
                        Some(flow_key_part(content))
                    })
                }
                Some(_) | None => first_end,
            };
            cursor.set_pos(end);
            Ok(folded(cursor.slice(start..end)))
        }
    }
}

/// The start of `line` that a plain key inside a flow collection may hold:
/// up to a `:` followed by a space or a flow indicator, a flow indicator, or
/// a comment.
fn flow_key_part(line: &str) -> &str {
    let bytes = line.as_bytes();
    let length = (0..bytes.len())
        .find(|&at| match bytes[at] {
            b',' | b'[' | b']' | b'{' | b'}' => true,
            b':' => matches!(
                bytes.get(at + 1),
                None | Some(b' ' | b'\t' | b',' | b'[' | b']' | b'{' | b'}')
            ),
            b' ' | b'\t' => bytes.get(at + 1) == Some(&b'#'),
            _ => false,
        })
        .unwrap_or(bytes.len());
    &line[..length]
}

/// Skips the spaces, line breaks and comments between the entries of a
/// flow collection. The line the next entry starts on must be indented
/// more than `column`.
fn skip_flow_space(cursor: &mut Cursor<'_>, column: Option<usize>) -> Result<(), ParseError> {
    let mut line_start = None;
    loop {
        match cursor.peek() {
            Some(b'\n') => {
                cursor.bump();
                line_start = Some(cursor.pos());
            }
            Some(b' ' | b'\t' | b'\r') => cursor.bump(),
            Some(b'#') => {
                let _: &str = cursor.take_until(b"\n");
            }
            Some(_) => break,
            None => return Ok(()),
        }
    }
    if let (Some(start), Some(parent)) = (line_start, column)
        && leading_spaces(cursor.slice(start..cursor.pos())) <= parent
    {
        return cursor.error("a line of a flow collection must be indented more than its key");
    }
    Ok(())
}

/// An unquoted scalar: the rest of the line up to a comment, or inside a
/// flow collection up to the next `,`, `[`, `]`, `{` or `}`. A line that
/// runs to its end goes on over the lines below it that are indented more
/// than its key or `-`.
fn plain(cursor: &mut Cursor<'_>, context: Context) -> Result<Value, ParseError> {
    no_indicator(cursor, matches!(context, Context::Flow(_)), "value")?;
    let start = cursor.pos();
    let line = cursor.rest_of_line();
    let bounded = plain_part(line, context);
    let text = bounded.trim_end();
    if holds_pair(text) {
        return cursor.error("a plain value cannot hold `: `; quote it");
    }
    let first_end = start + text.len();
    let end = if bounded.len() == line.len() {
        plain_end(cursor, first_end, context)
    } else {
        first_end
    };
    cursor.set_pos(end);
    let written = folded(cursor.slice(start..end));
    Ok(Value::Scalar(Scalar {
        span: start..end,
        value: scalar::yaml(&written),
    }))
}

/// Refuses a plain scalar, the `noun` named in the error, that starts with
/// `@`, a backtick, `%`, `|`, `>`, `,`, `]` or `}`, or with a `?`, `:` or `-`
/// that is an indicator. Inside a flow collection, `,`, `]` and `}` end the
/// entry instead.
fn no_indicator(cursor: &Cursor<'_>, in_flow: bool, noun: &str) -> Result<(), ParseError> {
    let Some(first) = cursor.peek() else {
        return Ok(());
    };
    let refused = match first {
        b',' | b']' | b'}' => !in_flow,
        b'@' | b'`' | b'%' | b'|' | b'>' => true,
        b'?' | b':' | b'-' => is_indicator(cursor, in_flow),
        _ => false,
    };
    if !refused {
        return Ok(());
    }
    let spaced =
        matches!(first, b'?' | b':' | b'-') && matches!(cursor.peek_at(1), Some(b' ' | b'\t'));
    let and_space = if spaced { " and a space" } else { "" };
    let message = format!(
        "a plain {noun} cannot start with `{}`{and_space}; quote it",
        char::from(first)
    );
    let at = cursor.pos();
    Err(ParseError::new(&message, at..at + 1))
}

/// The start of `line` that a plain scalar may hold: up to a comment, and
/// inside a flow collection up to the next `,`, `[`, `]`, `{` or `}`.
fn plain_part(line: &str, context: Context) -> &str {
    let uncommented = uncommented(line);
    match context {
        Context::Entry(_) | Context::Key(_) => uncommented,
        Context::Flow(_) => uncommented
            .find([',', '[', ']', '{', '}'])
            .map_or(uncommented, |at| &uncommented[..at]),
    }
}

/// Where a plain scalar whose first line ends at `first_end` ends: after the
/// last line below that goes on with it. Such a line holds no `key:` and
/// starts with no flow indicator; a comment or a flow indicator after its
/// text makes it the last.
fn plain_end(cursor: &Cursor<'_>, first_end: usize, within: Context) -> usize {
    continued_end(cursor, first_end, within, |content| {
        let bounded = plain_part(content, within);
        (!holds_pair(bounded.trim_end())).then_some(bounded)
    })
}

/// Where a plain scalar or key whose first line ends at `first_end` ends:
/// after the last line below that goes on with it. Such a line comes after
/// nothing but blank lines, is indented more than the column of `within`,
/// does not start with a comment, and has text in the part of it that `part`
/// gives, which is none when the line cannot go on with it; a part shorter
/// than the line makes it the last.
fn continued_end<F>(cursor: &Cursor<'_>, first_end: usize, within: Context, part: F) -> usize
where
    F: Fn(&str) -> Option<&str>,
{
    let rest = cursor.slice(first_end..cursor.end());
    let Some(break_at) = rest.find('\n') else {
        return first_end;
    };
    let mut end = first_end;
    let mut line_start = first_end + break_at + 1;
    for piece in cursor.slice(line_start..cursor.end()).split_inclusive('\n') {
        let line = line_text(piece);
        if !is_blank(line) {
            let content = line.trim_start_matches([' ', '\t']);
            let shallow = within
                .column()
                .is_some_and(|column| leading_spaces(line) <= column);
            if shallow || content.starts_with('#') {
                break;
            }
            let Some(bounded) = part(content) else {
                break;
            };
            let text = bounded.trim_end();
            if text.is_empty() {
                break;
            }
            end = line_start + (line.len() - content.len()) + text.len();
            if bounded.len() < content.len() {
                break;
            }
        }
        line_start += piece.len();
    }
    end
}

/// A line without the comment at its end, which starts at a `#` after a
/// space or a tab.
fn uncommented(line: &str) -> &str {
    line.match_indices('#')
        .map(|(at, _)| &line[..at])
        .find(|before| before.ends_with([' ', '\t']))
        .map_or(line, |before| &before[..before.len() - 1])
}

/// Whether a plain scalar's text holds a `:` followed by a space or a tab, or
/// ends with `:`, as a key does.
fn holds_pair(text: &str) -> bool {
    text.contains(": ") || text.contains(":\t") || text.ends_with(':')
}

/// The text of a plain scalar from the source it covers: each line without
/// the spaces around it, joined by a space, or by one line break for each
/// blank line between them.
fn folded(written: &str) -> String {
    let mut text = String::new();
    let mut blank = 0;
    for (number, line) in written.split('\n').enumerate() {
        let trimmed = line.trim_matches([' ', '\t', '\r']);
        if trimmed.is_empty() {
            blank += 1;
            continue;
        }
        if number > 0 {
            push_break(&mut text, blank);
        }
        text.push_str(trimmed);
        blank = 0;
    }
    text
}

/// The line break between two lines of a scalar: a space, or one line
/// break for each empty line between them.
fn push_break(text: &mut String, empty: usize) {
    if empty == 0 {
        text.push(' ');
    } else {
        text.push_str(&"\n".repeat(empty));
    }
}

/// A literal (`|`) or folded (`>`) block scalar whose key or list entry sits
/// at column `indent`: its header, then the lines indented more than that.
fn block_scalar(cursor: &mut Cursor<'_>, indent: usize) -> Result<Value, ParseError> {
    let start = cursor.pos();
    let folded = cursor.peek() == Some(b'>');
    cursor.bump();
    let (chomping, explicit) = block_header(cursor);
    let mut end = cursor.pos();
    end_of_line(cursor)?;
    let width = match explicit {
        Some(digits) => indent + digits,
        None => detected_width(cursor, indent)?,
    };
    let content_start = cursor.pos();
    let mut offset = 0;
    let mut lines = Vec::new();
    let mut broken = false;
    for piece in cursor.rest().split_inclusive('\n') {
        let line = line_text(piece);
        let spaces = leading_spaces(line);
        let has_break = piece.ends_with('\n');
        if is_blank(line) && spaces <= width {
            if has_break {
                lines.push(None);
            }
        } else if spaces >= width {
            lines.push(Some(&line[width..]));
            end = content_start + offset + line.len();
            broken = has_break;
        } else {
            break;
        }
        offset += piece.len();
    }
    let text_lines = lines
        .iter()
        .rposition(Option::is_some)
        .map_or(0, |at| at + 1);
    let (body, trailing) = lines.split_at(text_lines);
    let mut text = if folded { fold(body) } else { literal(body) };
    let final_break = usize::from(broken);
    let breaks = match chomping {
        Chomping::Clip => final_break,
        Chomping::Keep => final_break + trailing.len(),
        Chomping::Strip => 0,
    };
    text.push_str(&"\n".repeat(breaks));
    cursor.set_pos(end);
    Ok(string(start..end, text))
}

/// The chomping indicator and the indentation digit after `|` or `>`, in
/// either order.
fn block_header(cursor: &mut Cursor<'_>) -> (Chomping, Option<usize>) {
    let mut chomping = Chomping::Clip;
    let mut explicit = None;
    for _ in 0_u8..2 {
        match cursor.peek() {
            Some(b'-') if matches!(chomping, Chomping::Clip) => chomping = Chomping::Strip,
            Some(b'+') if matches!(chomping, Chomping::Clip) => chomping = Chomping::Keep,
            Some(digit @ b'1'..=b'9') if explicit.is_none() => {
                explicit = Some(usize::from(digit - b'0'));
            }
            Some(_) | None => break,
        }
        cursor.bump();
    }
    (chomping, explicit)
}

/// The column of a block scalar's text, from its first line of text. With
/// no text, every blank line is empty.
fn detected_width(cursor: &Cursor<'_>, indent: usize) -> Result<usize, ParseError> {
    let mut deepest_blank = 0;
    let mut offset = 0;
    for piece in cursor.rest().split_inclusive('\n') {
        let line = line_text(piece);
        let spaces = leading_spaces(line);
        if is_blank(line) {
            deepest_blank = deepest_blank.max(spaces);
        } else if spaces <= indent {
            break;
        } else if deepest_blank > spaces {
            let at = cursor.pos() + offset;
            return Err(ParseError::new(
                "a blank line before the text of a block scalar is indented more than the text",
                at..at,
            ));
        } else {
            return Ok(spaces);
        }
        offset += piece.len();
    }
    Ok(deepest_blank.max(indent + 1))
}

/// Literal lines keep their line breaks.
fn literal(lines: &[Option<&str>]) -> String {
    let texts: Vec<&str> = lines.iter().map(|line| line.unwrap_or_default()).collect();
    texts.join("\n")
}

/// Folded lines: the line break between two lines of text is a space, an
/// empty line is a line break, and a line indented more than the text keeps
/// the line breaks around it.
fn fold(lines: &[Option<&str>]) -> String {
    let mut text = String::new();
    let mut previous: Option<&str> = None;
    let mut empty = 0;
    for line in lines {
        let Some(current) = *line else {
            empty += 1;
            continue;
        };
        let breaks = match previous {
            Some(before) if !is_spaced(before) && !is_spaced(current) => {
                if empty == 0 {
                    text.push(' ');
                }
                empty
            }
            Some(_) => empty + 1,
            None => empty,
        };
        text.push_str(&"\n".repeat(breaks));
        text.push_str(current);
        previous = Some(current);
        empty = 0;
    }
    text
}

/// A line of `piece` without its line ending.
fn line_text(piece: &str) -> &str {
    let line = piece.strip_suffix('\n').unwrap_or(piece);
    line.strip_suffix('\r').unwrap_or(line)
}

fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn is_blank(line: &str) -> bool {
    line.trim_start_matches([' ', '\t']).is_empty()
}

/// Whether a line of a folded scalar is indented more than its text.
fn is_spaced(line: &str) -> bool {
    line.starts_with([' ', '\t'])
}

/// A value written after its properties, inside `open` values: read again
/// under its tag, and named by its anchor, which is open while it is read.
fn propertied(cursor: &mut Cursor<'_>, context: Context, open: usize) -> Result<Value, Stop> {
    let (properties, depth, under) = node_properties(cursor, context, open)?;
    let name = match &properties.tag {
        Some(tag) => Some(known(cursor, tag)?),
        None => None,
    };
    if let Some(anchor) = &properties.anchor {
        open_anchor(cursor, anchor);
    }
    let read = if under {
        nested(cursor, context, properties.end, depth).inspect(|_| to_line_end(cursor))
    } else {
        inline_value(cursor, context, depth)
    };
    finish_node(cursor, properties, name, read)
}

/// Reads the properties of the node at the cursor, inside `open` values, with
/// the depth its value is read at and whether that value is under them. In
/// block context they may end their line, and the value is what the lines
/// under them hold, as for a key or a list entry with nothing after it; with
/// none, it is empty. The first of those lines may start with more of them. A
/// collection counts its properties as a level of its own.
fn node_properties(
    cursor: &mut Cursor<'_>,
    context: Context,
    open: usize,
) -> Result<(Properties, usize, bool), Stop> {
    let mut properties = Properties::new(cursor.pos());
    let Context::Flow(column) = context else {
        loop {
            properties.read(cursor, |inner| {
                inner.skip_spaces();
                Ok(())
            })?;
            if !at_line_end(cursor) {
                let depth = tagged_depth(cursor, &properties, open)?;
                return Ok((properties, depth, false));
            }
            end_of_line(cursor)?;
            if let Some(depth) = depth_under(cursor, context, &properties, open)? {
                return Ok((properties, depth, true));
            }
        }
    };
    properties.read(cursor, |inner| skip_flow_space(inner, column))?;
    let depth = tagged_depth(cursor, &properties, open)?;
    Ok((properties, depth, false))
}

/// The value `read` gives under `properties`, once its anchor is no longer
/// open: retagged as `!!name`, and kept under its anchor, a copy that counts
/// toward `COPY_CAP` and `TEXT_CAP`.
fn finish_node(
    cursor: &mut Cursor<'_>,
    properties: Properties,
    name: Option<&str>,
    read: Result<Value, Stop>,
) -> Result<Value, Stop> {
    if properties.anchor.is_some() {
        let _: Option<String> = cursor.state_mut().anchors.open.pop();
    }
    let mut value = read?;
    if let (Some(tag), Some(type_name)) = (&properties.tag, name) {
        value = retag(cursor, type_name, &tag.span, value)?;
    }
    let span = properties.span();
    if let Some(anchor) = properties.anchor {
        keep(cursor, anchor, &value, &span)?;
    }
    Ok(value)
}

/// Moves the cursor from the start of a line back onto the line ending
/// before it, where the block read over the lines above ends; the span of a
/// mapping whose every pair is dropped does not reach that far.
fn to_line_end(cursor: &mut Cursor<'_>) {
    let at = cursor.pos();
    if at > 0 && cursor.slice(at - 1..at) == "\n" {
        cursor.set_pos(at - 1);
    }
}

/// The depth inside `open` values that the node after `properties`, at the
/// cursor, is read at: a flow collection counts them as a level of their own.
fn tagged_depth(cursor: &Cursor<'_>, properties: &Properties, open: usize) -> Result<usize, Stop> {
    match cursor.peek() {
        Some(b'[' | b'{') => properties.level(open),
        Some(_) | None => Ok(open),
    }
}

/// [`tagged_depth`] for the node that `nested` reads under properties that
/// end their line, with the cursor at the start of the next line: a block
/// collection there counts them as a level of their own too. `None` when that
/// line starts with more properties or an alias, with the cursor moved to
/// them.
fn depth_under(
    cursor: &mut Cursor<'_>,
    context: Context,
    properties: &Properties,
    open: usize,
) -> Result<Option<usize>, Stop> {
    let start = cursor.pos();
    let depth = match under(cursor, context) {
        None => Ok(open),
        Some((_, true)) => properties.level(open),
        Some((found, false)) => {
            cursor.set_pos(cursor.pos() + found);
            no_tab_before_block(cursor, open)?;
            if starts_mapping(cursor, open) {
                properties.level(open)
            } else {
                cursor.skip_spaces();
                if matches!(cursor.peek(), Some(b'!' | b'&' | b'*')) {
                    return Ok(None);
                }
                tagged_depth(cursor, properties, open)
            }
        }
    };
    cursor.set_pos(start);
    depth.map(Some)
}

/// An anchor at `at` on a node that already has one.
fn second_anchor(at: usize) -> ParseError {
    ParseError::new("a value has at most one anchor", at..at + 1)
}

/// A tag at `at` on a node that already has one.
fn second_tag(at: usize) -> ParseError {
    ParseError::new("a value has at most one tag", at..at + 1)
}

/// A key written after its properties, read by `read`, checked against its
/// tag and named by its anchor. A `:` right after them leaves the key empty,
/// as `is_empty_key` reads it inside a flow collection or not.
fn propertied_key<F>(cursor: &mut Cursor<'_>, in_flow: bool, read: F) -> Result<String, Stop>
where
    F: FnOnce(&mut Cursor<'_>) -> Result<String, Stop>,
{
    let mut properties = Properties::new(cursor.pos());
    properties.read(cursor, |inner| {
        inner.skip_spaces();
        Ok(())
    })?;
    let start = cursor.pos();
    let quoted = matches!(cursor.peek(), Some(b'"' | b'\''));
    let text = if is_empty_key(cursor, in_flow) {
        String::new()
    } else {
        read(cursor)?
    };
    let span = start..cursor.pos();
    let node = match &properties.tag {
        Some(tag) => {
            let name = known(cursor, tag)?;
            retag(cursor, name, &tag.span, string(span, text.clone()))?
        }
        None if quoted => string(span, text.clone()),
        None => Value::Scalar(Scalar {
            span,
            value: scalar::yaml(&text),
        }),
    };
    let at = properties.span();
    if let Some(anchor) = properties.anchor {
        keep(cursor, anchor, &node, &at)?;
    }
    Ok(text)
}

/// Reads the tag at the cursor and the type it names: `!!int` and
/// `!<tag:yaml.org,2002:int>` both name `int`, and `!point` none.
fn tag(cursor: &mut Cursor<'_>) -> Tag {
    let start = cursor.pos();
    pass_tag(cursor);
    let written = cursor.slice(start..cursor.pos());
    let named = written.strip_prefix("!!").or_else(|| {
        written
            .strip_prefix("!<tag:yaml.org,2002:")?
            .strip_suffix('>')
    });
    Tag {
        name: named.and_then(|suffix| TAGS.into_iter().find(|known| *known == suffix)),
        span: start..cursor.pos(),
    }
}

/// Moves the cursor past the tag at it: a tag written in full ends at its
/// `>`, any other at a space, a tab, a line ending or a flow indicator.
fn pass_tag(cursor: &mut Cursor<'_>) {
    if cursor.rest().starts_with("!<") {
        let _: &str = cursor.take_until(b" \t\r\n>");
        let _: bool = cursor.eat(b'>');
    } else {
        let _: &str = cursor.take_until(b" \t\r\n,[]{}");
    }
}

/// The type `tag` names, or a drop when arky does not read it.
fn known(cursor: &Cursor<'_>, tag: &Tag) -> Result<&'static str, Stop> {
    tag.name.ok_or_else(|| {
        let written = cursor.slice(tag.span.clone());
        let message = format!("the tag `{written}` is not one arky reads");
        Stop::Drop(ParseError::new(&message, tag.span.clone()))
    })
}

/// `value` read again under the tag `!!name` at `at`: a scalar's text gets
/// the tag's type, and a mapping or a list must be what the tag names. An
/// empty value is read as empty text, and an error about it points at `at`.
fn retag(
    cursor: &Cursor<'_>,
    name: &str,
    at: &Range<usize>,
    value: Value,
) -> Result<Value, ParseError> {
    let scalar = match value {
        Value::Mapping(_) if name == "map" => return Ok(value),
        Value::Sequence(_) if name == "seq" => return Ok(value),
        Value::Mapping(mapping) => return Err(misfit("a mapping", name, mapping.span)),
        Value::Sequence(sequence) => return Err(misfit("a list", name, sequence.span)),
        Value::Scalar(scalar) => scalar,
    };
    let text = scalar_text(cursor, scalar.value, &scalar.span);
    let Some(typed) = typed(name, &text) else {
        if scalar.span.is_empty() {
            return Err(misfit("an empty value", name, at.clone()));
        }
        return Err(misfit(&format!("`{text}`"), name, scalar.span));
    };
    Ok(Value::Scalar(Scalar {
        span: scalar.span,
        value: typed,
    }))
}

/// A scalar's text: a string's own, or the text any other value is written
/// as at `span`.
fn scalar_text(cursor: &Cursor<'_>, value: ScalarValue, span: &Range<usize>) -> String {
    match value {
        ScalarValue::String(text) => text,
        ScalarValue::Boolean(_)
        | ScalarValue::Date(_)
        | ScalarValue::DateTime(_)
        | ScalarValue::Float(_)
        | ScalarValue::Integer(_)
        | ScalarValue::Null
        | ScalarValue::OffsetDateTime(_)
        | ScalarValue::Time(_) => folded(cursor.slice(span.clone())),
    }
}

/// The value `text` has under the scalar tag `!!name`, read by the rules of
/// an untagged value of that type, or `None` when it does not fit.
fn typed(name: &str, text: &str) -> Option<ScalarValue> {
    match (name, scalar::yaml(text)) {
        ("str", _) => Some(ScalarValue::String(text.to_owned())),
        ("float", oversized @ ScalarValue::String(_)) if scalar::is_oversized_float(text) => {
            Some(oversized)
        }
        ("float", ScalarValue::Integer(_) | ScalarValue::String(_)) => scalar::decimal_float(text),
        ("int", oversized @ ScalarValue::String(_)) if scalar::is_oversized_integer(text) => {
            Some(oversized)
        }
        ("bool", fitting @ ScalarValue::Boolean(_))
        | ("float", fitting @ ScalarValue::Float(_))
        | ("int", fitting @ ScalarValue::Integer(_))
        | ("null", fitting @ ScalarValue::Null)
        | (
            "timestamp",
            fitting @ (ScalarValue::Date(_)
            | ScalarValue::DateTime(_)
            | ScalarValue::OffsetDateTime(_)),
        ) => Some(fitting),
        _ => None,
    }
}

fn misfit(written: &str, name: &str, span: Range<usize>) -> ParseError {
    let message = format!("{written} is not a value of the tag `!!{name}`");
    ParseError::new(&message, span)
}

/// Keeps a copy of `value` under `anchor`, written at `span`, once the copy
/// is charged.
fn keep(
    cursor: &mut Cursor<'_>,
    anchor: String,
    value: &Value,
    span: &Range<usize>,
) -> Result<(), Stop> {
    let anchors = &mut cursor.state_mut().anchors;
    let (size, _) = measure(value, anchors.left());
    anchors.charge(size, span)?;
    set_anchor(cursor, anchor, Some(value.clone()));
    Ok(())
}

/// Opens `anchor` while the value it is on is read: it names nothing until
/// that value is kept.
fn open_anchor(cursor: &mut Cursor<'_>, anchor: &str) {
    set_anchor(cursor, anchor.to_owned(), None);
    cursor.state_mut().anchors.open.push(anchor.to_owned());
}

/// Makes `anchor` name `value`, or nothing, until a dropped entry it is set
/// inside undoes it.
fn set_anchor(cursor: &mut Cursor<'_>, anchor: String, value: Option<Value>) {
    let change = cursor.state_mut().anchors.set(anchor, value);
    cursor.changed(change);
}

/// A copy of the value the alias at the cursor names, inside `open` values,
/// with every span in it on the alias. The copy counts toward `COPY_CAP` and
/// `TEXT_CAP`, and toward `VALUE_CAP` from where the alias stands.
fn alias(cursor: &mut Cursor<'_>, open: usize) -> Result<Value, Stop> {
    let (name, span) = alias_name(cursor)?;
    let anchors = &mut cursor.state_mut().anchors;
    let limit = anchors.left();
    let (size, levels) = measure(anchors.get(&name, &span)?, limit);
    anchors.charge(size, &span)?;
    // The deepest list or mapping of the copy opens inside the others.
    if let Some(inside) = levels.checked_sub(1) {
        let _: usize = nest(open + inside, span.clone())?;
    }
    Ok(copy(anchors.get(&name, &span)?, &span))
}

/// The text of the scalar the alias at the cursor names, as a key, or where
/// the alias is written when it names a list or a mapping. The text counts
/// as one copied value and its bytes.
fn alias_key(cursor: &mut Cursor<'_>) -> Result<Key, Stop> {
    let (name, span) = alias_name(cursor)?;
    let anchors = &mut cursor.state_mut().anchors;
    let Value::Scalar(scalar) = anchors.get(&name, &span)? else {
        return Ok(Err(span));
    };
    let original = scalar.clone();
    let size = Size {
        bytes: text_length(&original),
        values: 1,
    };
    anchors.charge(size, &span)?;
    Ok(Ok(scalar_text(cursor, original.value, &original.span)))
}

/// Reads the alias at the cursor: its name and where it is written.
fn alias_name(cursor: &mut Cursor<'_>) -> Result<(String, Range<usize>), ParseError> {
    let start = cursor.pos();
    let name = anchor_name(cursor)?;
    Ok((name, start..cursor.pos()))
}

/// Reads the `&` or the `*` at the cursor and the name after it.
fn anchor_name(cursor: &mut Cursor<'_>) -> Result<String, ParseError> {
    let at = cursor.pos();
    let indicator = cursor.peek().map_or_default(char::from);
    let name = pass_name(cursor);
    if name.is_empty() {
        let message = format!("expected a name after `{indicator}`");
        return Err(ParseError::new(&message, at..at + 1));
    }
    Ok(name.to_owned())
}

/// Moves the cursor past the `&` or the `*` at it and the name after it,
/// which runs to the first space, tab, line ending or flow indicator, and
/// returns the name.
fn pass_name<'src>(cursor: &mut Cursor<'src>) -> &'src str {
    cursor.bump();
    cursor.take_until(b" \t\r\n,[]{}")
}

/// How much `value` holds, itself included, counted until it passes
/// `limit`, and how many lists and mappings nest in it at its deepest.
fn measure(value: &Value, limit: Size) -> (Size, usize) {
    let mut size = Size::default();
    let mut levels = 0;
    let mut open = vec![(0, value)];
    while let Some((depth, held)) = open.pop() {
        size.values += 1;
        if size.values > limit.values || size.bytes > limit.bytes {
            break;
        }
        match held {
            Value::Mapping(mapping) => {
                levels = levels.max(depth + 1);
                for (key, pair) in &mapping.pairs {
                    size.bytes += key.len();
                    open.push((depth + 1, &pair.value));
                }
            }
            Value::Scalar(scalar) => size.bytes += text_length(scalar),
            Value::Sequence(sequence) => {
                levels = levels.max(depth + 1);
                open.extend(sequence.items.iter().map(|item| (depth + 1, item)));
            }
        }
    }
    (size, levels)
}

/// The bytes of a scalar's text: a string's own, or those it is written as.
const fn text_length(scalar: &Scalar) -> usize {
    match &scalar.value {
        ScalarValue::String(text) => text.len(),
        ScalarValue::Boolean(_)
        | ScalarValue::Date(_)
        | ScalarValue::DateTime(_)
        | ScalarValue::Float(_)
        | ScalarValue::Integer(_)
        | ScalarValue::Null
        | ScalarValue::OffsetDateTime(_)
        | ScalarValue::Time(_) => scalar.span.end - scalar.span.start,
    }
}

/// `value` with every span in it set to `span`.
fn copy(value: &Value, span: &Range<usize>) -> Value {
    match value {
        Value::Mapping(mapping) => {
            let pairs = mapping.pairs.iter().map(|(key, pair)| {
                let copied = Pair {
                    repeats: vec![span.clone(); pair.repeats.len()],
                    span: span.clone(),
                    value: copy(&pair.value, span),
                };
                (key.clone(), copied)
            });
            Value::Mapping(Mapping {
                pairs: pairs.collect(),
                span: span.clone(),
            })
        }
        Value::Scalar(scalar) => Value::Scalar(Scalar {
            span: span.clone(),
            value: scalar.value.clone(),
        }),
        Value::Sequence(sequence) => Value::Sequence(Sequence {
            items: sequence.items.iter().map(|item| copy(item, span)).collect(),
            span: span.clone(),
        }),
    }
}

/// A double-quoted string. With no `context`, as for a key held to its line,
/// it stays on one line. A line break folds as in a plain scalar, and a `\` before it
/// removes it.
fn double_quoted(cursor: &mut Cursor<'_>, context: Option<Context>) -> Result<String, ParseError> {
    cursor.bump();
    let mut text = String::new();
    loop {
        let run = cursor.take_until(b"\"\\\n");
        if cursor.eat(b'"') {
            text.push_str(run);
            return Ok(text);
        }
        if cursor.eat(b'\\') {
            text.push_str(run);
            if matches!(cursor.peek(), Some(b'\r' | b'\n')) {
                let empty = line_break(cursor, context)?;
                text.push_str(&"\n".repeat(empty));
            } else {
                text.push(escape(cursor)?);
            }
        } else {
            text.push_str(run.trim_end_matches([' ', '\t', '\r']));
            let empty = line_break(cursor, context)?;
            push_break(&mut text, empty);
        }
    }
}

/// Reads the line break at the cursor inside a quoted string, the empty
/// lines after it and the indentation of the next line, and returns the
/// number of empty lines. The next line must be indented more than the
/// string's key or `-`, also inside a flow collection.
fn line_break(cursor: &mut Cursor<'_>, context: Option<Context>) -> Result<usize, ParseError> {
    let Some(within) = context else {
        return cursor.error("the string is not closed");
    };
    let mut empty = 0;
    loop {
        let _: bool = cursor.eat(b'\r');
        if !cursor.eat(b'\n') {
            return cursor.error("the string is not closed");
        }
        let line = cursor.rest_of_line();
        if !is_blank(line) {
            if let Some(column) = within.column()
                && leading_spaces(line) <= column
            {
                return cursor.error(
                    "a line inside a quoted string must be indented more than its key or `-`",
                );
            }
            cursor.skip_spaces();
            return Ok(empty);
        }
        cursor.set_pos(cursor.pos() + line.len());
        empty += 1;
    }
}

fn escape(cursor: &mut Cursor<'_>) -> Result<char, ParseError> {
    let Some(byte) = cursor.peek() else {
        return cursor.error("the string is not closed");
    };
    cursor.bump();
    let character = match byte {
        b'0' => Some('\0'),
        b'a' => Some('\u{7}'),
        b'b' => Some('\u{8}'),
        b't' => Some('\t'),
        b'n' => Some('\n'),
        b'v' => Some('\u{b}'),
        b'f' => Some('\u{c}'),
        b'r' => Some('\r'),
        b'e' => Some('\u{1b}'),
        b' ' => Some(' '),
        b'"' => Some('"'),
        b'/' => Some('/'),
        b'\\' => Some('\\'),
        b'x' => cursor.hex_char(2),
        b'u' => cursor.hex_char(4),
        b'U' => cursor.hex_char(8),
        _ => None,
    };
    character.map_or_else(|| cursor.error("unknown escape in the string"), Ok)
}

/// A single-quoted string, on one line or over several as a double-quoted
/// one is.
fn single_quoted(cursor: &mut Cursor<'_>, context: Option<Context>) -> Result<String, ParseError> {
    cursor.bump();
    let mut text = String::new();
    loop {
        let run = cursor.take_until(b"'\n");
        if cursor.eat(b'\'') {
            text.push_str(run);
            // Inside single quotes, `''` is one quote.
            if !cursor.eat(b'\'') {
                return Ok(text);
            }
            text.push('\'');
        } else {
            text.push_str(run.trim_end_matches([' ', '\t', '\r']));
            let empty = line_break(cursor, context)?;
            push_break(&mut text, empty);
        }
    }
}

const fn string(span: Range<usize>, text: String) -> Value {
    Value::Scalar(Scalar {
        span,
        value: ScalarValue::String(text),
    })
}

const fn null(at: usize) -> Value {
    Value::Scalar(Scalar {
        span: at..at,
        value: ScalarValue::Null,
    })
}

/// Whether a list entry starts `offset` bytes after the cursor: a `-`
/// followed by a space, a tab or the end of the line.
fn is_dash(cursor: &Cursor<'_>, offset: usize) -> bool {
    cursor.peek_at(offset) == Some(b'-') && separated_at(cursor, offset + 1)
}

/// Whether the `?`, `:` or `-` at the cursor is an indicator rather than the
/// start of a plain scalar: a space, a tab or the end of the line follows
/// it, or inside a flow collection one of `,`, `[`, `]`, `{` or `}`.
fn is_indicator(cursor: &Cursor<'_>, in_flow: bool) -> bool {
    separated_at(cursor, 1)
        || (in_flow && matches!(cursor.peek_at(1), Some(b',' | b'[' | b']' | b'{' | b'}')))
}

/// Whether a space, a tab or the end of the line is `offset` bytes after the
/// cursor.
fn separated_at(cursor: &Cursor<'_>, offset: usize) -> bool {
    matches!(
        cursor.peek_at(offset),
        None | Some(b' ' | b'\t' | b'\n' | b'\r')
    )
}

fn at_line_end(cursor: &Cursor<'_>) -> bool {
    matches!(cursor.peek(), None | Some(b'\n' | b'\r' | b'#'))
}

fn end_of_line(cursor: &mut Cursor<'_>) -> Result<(), ParseError> {
    cursor.skip_spaces();
    if cursor.peek() == Some(b'#') {
        let _: &str = cursor.take_until(b"\n");
    }
    let _: bool = cursor.eat(b'\r');
    if cursor.at_end() || cursor.eat(b'\n') {
        Ok(())
    } else {
        cursor.error("expected the end of the line after the value")
    }
}
