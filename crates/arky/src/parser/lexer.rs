//! Body lines into tokens. The indentation that says which list item a line
//! belongs to becomes explicit item starts and ends, and a code block, a block
//! quote, an HTML block and a link definition are each read whole, so the
//! grammar over the tokens is context-free.

use core::iter;
use core::ops::Range;

use crate::parser::definition::{self, Target};
use crate::parser::html::{self, End};
use crate::parser::line::{Base, Line, line_at, line_from, space, space_to};
use crate::parser::table::{self, TableToken};

/// How deep list items, markdown blocks and table cells nest, counted
/// together: deeper ones overflow a 2 MB debug stack.
pub const BLOCK_CAP: usize = 40;

/// The marker that starts a list item.
#[derive(Clone, Copy)]
pub enum Marker {
    /// `-`, `*` or `+`.
    Bullet(char),
    /// `1.` or `1)`: the number and the delimiter.
    Ordered { delimiter: char, number: u64 },
}

impl Marker {
    /// Whether an item with this marker may start a list on a line that
    /// would otherwise continue a paragraph: a bullet, or an ordered marker
    /// numbered 1.
    const fn interrupts(self) -> bool {
        match self {
            Self::Bullet(_) => true,
            Self::Ordered {
                delimiter: _,
                number,
            } => number == 1,
        }
    }

    /// Whether an item with `other` belongs to the list this marker is in:
    /// the same bullet character, or the same delimiter.
    pub const fn same_list(self, other: Self) -> bool {
        self.symbol() == other.symbol()
    }

    /// The first number of the list this marker starts, `None` for a bullet.
    pub const fn start(self) -> Option<u64> {
        match self {
            Self::Bullet(_) => None,
            Self::Ordered {
                delimiter: _,
                number,
            } => Some(number),
        }
    }

    /// The bullet character, or the delimiter of an ordered marker.
    const fn symbol(self) -> char {
        match self {
            Self::Bullet(bullet) => bullet,
            Self::Ordered {
                delimiter,
                number: _,
            } => delimiter,
        }
    }
}

pub struct Token {
    pub kind: TokenKind,
    pub span: Range<usize>,
}

pub enum TokenKind {
    Blank,
    BlockQuote {
        value: String,
    },
    /// A code block: the range of its lines, of its info string, trimmed,
    /// and its lines as the block holds them.
    CodeBlock {
        content: Range<usize>,
        info: Range<usize>,
        value: String,
    },
    /// A link reference definition: its label as written and its target.
    Definition {
        label: String,
        target: Target,
    },
    Heading {
        level: u8,
        text: Range<usize>,
    },
    Html {
        value: String,
    },
    ItemEnd,
    ItemStart(Marker),
    /// A table, and how many containers hold it.
    Table {
        depth: usize,
        table: TableToken,
    },
    Text,
    ThematicBreak,
    /// `===` or `---` under text: the level of the heading it makes.
    Underline(u8),
}

/// The block a line starts, other than a paragraph.
enum Start {
    Break,
    Fence {
        length: usize,
        mark: char,
    },
    Heading {
        level: u8,
        text: Range<usize>,
    },
    Html(html::Start),
    Item {
        marker: Marker,
        width: usize,
    },
    Quote,
    Table {
        next: usize,
        span: Range<usize>,
        table: TableToken,
    },
    Underline(u8),
}

/// A line's text cut at a column: as spaces, the columns past it of a tab it
/// falls inside, or of the white space before an HTML block's text, then the
/// text after them.
#[derive(Clone, Copy)]
struct Cut<'src> {
    spaces: usize,
    text: &'src str,
}

/// A place on a line read: its byte, and its column. The line's tab stops
/// count from its column 0.
#[derive(Clone, Copy)]
struct Spot {
    at: usize,
    column: usize,
}

impl Spot {
    /// The spot `width` bytes on, past marks of one column each.
    const fn after(self, width: usize) -> Self {
        Self {
            at: self.at + width,
            column: self.column + width,
        }
    }

    /// The byte that holds column `limit`, the tab it falls inside or where
    /// the text after it starts, and what is written from this spot to
    /// `end` past its white space up to that column.
    fn cut(self, source: &str, end: usize, limit: usize) -> (usize, Cut<'_>) {
        let passed = space_to(&source[self.at..end], self.column, limit);
        let spaces = (self.column + passed.columns).saturating_sub(limit);
        let rest = self.at + passed.bytes;
        let cut = Cut {
            spaces,
            text: &source[rest..end],
        };
        (rest - usize::from(spaces > 0), cut)
    }

    const fn start(line: &Line) -> Self {
        Self {
            at: line.start,
            column: line.column,
        }
    }

    /// Where the text starts past the spaces and tabs at this spot, up to
    /// `end`.
    fn text(self, source: &str, end: usize) -> Self {
        let passed = space(&source[self.at..end], self.column);
        Self {
            at: self.at + passed.bytes,
            column: self.column + passed.columns,
        }
    }
}

struct Lexer<'src> {
    /// What each line is read past.
    base: Base<'src>,
    /// The content column of each open list item, outermost first.
    levels: Vec<usize>,
    /// How many containers hold the text read: list items, markdown blocks
    /// and table cells.
    outer: usize,
    pos: usize,
    source: &'src str,
    tokens: Vec<Token>,
}

impl Lexer<'_> {
    /// Reads a block quote whose first `>` is at `quote`: that line, then
    /// each line whose text starts with `>` within three columns of the
    /// enclosing item's text. Each line loses its `>` and one column of the
    /// space or tab after it.
    fn block_quote(&mut self, quote: Spot) {
        let source = self.source;
        let level = self.level();
        let first = line_at(source, quote.at);
        let mut lines = vec![unquote(source, quote, first.end)];
        let mut end = first.end;
        while self.pos < source.len() {
            let line = line_from(source, self.pos, self.base);
            let text = Spot::start(&line).text(source, line.end);
            if text.column < level
                || text.column >= level + 4
                || !source[text.at..line.end].starts_with('>')
            {
                break;
            }
            lines.push(unquote(source, text, line.end));
            end = line.end;
            self.pos = line.next;
        }
        let value = join(&lines);
        self.push(TokenKind::BlockQuote { value }, quote.at..end);
    }

    /// Ends every open item whose content column is to the right of `column`.
    fn close_items(&mut self, column: usize, at: usize) {
        while self.levels.last().is_some_and(|&level| level > column) {
            self.levels.pop();
            self.push(TokenKind::ItemEnd, at..at);
        }
    }

    /// Reads the lines after the opening fence of `length` `mark`s at
    /// `fence`, whose line's text ends at `end`, up to the closing fence,
    /// indented at most three columns past the enclosing item's text, the end
    /// of the enclosing list item, or the end of the file. Each line loses
    /// its white space up to the fence's column.
    fn code_block(&mut self, fence: Spot, end: usize, length: usize, mark: char) {
        let source = self.source;
        let at = fence.at;
        let after = &source[at + length..end];
        let from = at + length + (after.len() - after.trim_start().len());
        let info = from..from + after.trim().len();
        let level = self.level();
        let mut lines = Vec::new();
        let mut block_end = end;
        let content_start = self.pos;
        let mut content_end = content_start;
        while self.pos < source.len() {
            let line = line_from(source, self.pos, self.base);
            let leading = Spot::start(&line);
            let text = leading.text(source, line.end);
            let trimmed = trim_end_spaces(&source[text.at..line.end]);
            if !trimmed.is_empty() && text.column < level {
                break;
            }
            self.pos = line.next;
            if text.column < level + 4
                && trimmed.len() >= length
                && trimmed.chars().all(|character| character == mark)
            {
                block_end = text.at + trimmed.len();
                break;
            }
            lines.push(leading.cut(source, line.end, fence.column).1);
            block_end = line.end;
            content_end = line.end;
        }
        let value = join(&lines);
        let content = content_start..content_end;
        self.push(
            TokenKind::CodeBlock {
                content,
                info,
                value,
            },
            at..block_end,
        );
    }

    /// Classifies what is written on the line starting at `line_start`, from
    /// `white`, where the white space before its text starts, to `end`.
    /// `continues` is set when the line would continue a paragraph.
    fn content(&mut self, line_start: usize, white: Spot, end: usize, continues: bool) {
        let source = self.source;
        let text = white.text(source, end);
        let level = self.level();
        if text.column >= level + 4 {
            if continues {
                self.push(TokenKind::Text, text.at..end);
            } else {
                self.indented_code(white, level + 4);
            }
            return;
        }
        match self.start(line_start, text, end, self.pos, continues) {
            Some(Start::Break) => self.push(TokenKind::ThematicBreak, text.at..end),
            Some(Start::Fence { length, mark }) => self.code_block(text, end, length, mark),
            Some(Start::Heading {
                level: heading_level,
                text: title,
            }) => {
                let kind = TokenKind::Heading {
                    level: heading_level,
                    text: text.at + title.start..text.at + title.end,
                };
                self.push(kind, text.at..end);
            }
            Some(Start::Html(start)) => self.html(white, text, start),
            Some(Start::Item { marker, width }) => {
                let after = text.after(width);
                let inner = after.text(source, end);
                let spaces = inner.column - after.column;
                // One to four columns of white space set the content column;
                // none, or five or more, put it one column past the marker.
                let gap = if spaces == 0 || spaces > 4 { 1 } else { spaces };
                self.push(TokenKind::ItemStart(marker), text.at..after.at);
                self.levels.push(after.column + gap);
                if inner.at < end {
                    self.content(line_start, after, end, false);
                }
            }
            Some(Start::Quote) => self.block_quote(text),
            Some(Start::Table { next, span, table }) => {
                self.pos = next;
                let depth = self.depth();
                self.push(TokenKind::Table { depth, table }, span);
            }
            Some(Start::Underline(heading_level)) => {
                self.push(TokenKind::Underline(heading_level), text.at..end);
            }
            None => {
                if continues || !self.definition(text.at, end) {
                    self.push(TokenKind::Text, text.at..end);
                }
            }
        }
    }

    /// The line at `at`, from its text to its end, and where the next line
    /// starts, when it would continue a paragraph.
    fn continuation(&self, at: usize) -> Option<(Range<usize>, usize)> {
        let source = self.source;
        let level = self.level();
        let line = line_from(source, at, self.base);
        let text = Spot::start(&line).text(source, line.end);
        let end = text.at + trim_end_spaces(&source[text.at..line.end]).len();
        let continues = end > text.at
            && (text.column >= level + 4
                || (text.column >= level && self.start(at, text, end, line.next, true).is_none()));
        continues.then_some((text.at..end, line.next))
    }

    /// Reads a link reference definition that starts at `at`, over its line
    /// and as many lines after it as it needs that would continue a
    /// paragraph, each up to its line ending, so a title over a line end
    /// keeps the spaces and tabs before it. Gives whether there was one.
    fn definition(&mut self, at: usize, end: usize) -> bool {
        let source = self.source;
        if !source[at..end].starts_with('[') {
            return false;
        }
        let written = |from: usize| &source[from..line_at(source, from).end];
        let mut text = written(at).to_owned();
        let mut ends = vec![(end, self.pos)];
        let read = loop {
            let reading = definition::read(&text);
            let next = ends.last().map_or(self.pos, |&(_, next)| next);
            let more = if reading.open && next < source.len() {
                self.continuation(next)
            } else {
                None
            };
            let Some((line, after)) = more else {
                break reading.definition;
            };
            text.push('\n');
            text.push_str(written(line.start));
            ends.push((line.end, after));
        };
        let Some(found) = read else {
            return false;
        };
        let (last, next) = ends[found.lines - 1];
        self.pos = next;
        let kind = TokenKind::Definition {
            label: found.label,
            target: found.target,
        };
        self.push(kind, at..last);
        true
    }

    /// How many containers hold the line read now.
    const fn depth(&self) -> usize {
        self.outer + self.levels.len()
    }

    /// Reads an HTML block whose first line's white space starts at `white`
    /// and text at `head`, up to the line that ends it by `start`, the end of
    /// the enclosing list item, or the end of the file. Its lines are kept as
    /// written past the item's text column, the first's white space as spaces.
    fn html(&mut self, white: Spot, head: Spot, start: html::Start) {
        let source = self.source;
        let level = self.level();
        let at = head.at;
        let first = line_at(source, at);
        let from = white.cut(source, first.end, level).0;
        let mut lines = vec![Cut {
            spaces: head.column - level,
            text: &source[at..first.end],
        }];
        let mut block_end = first.end;
        let mut blanks = Vec::new();
        let mut scan = self.pos;
        let mut open = !start.end.closes(&source[at + start.opening..first.end]);
        while open && scan < source.len() {
            let line = line_from(source, scan, self.base);
            let leading = Spot::start(&line);
            let text = leading.text(source, line.end);
            let cut = leading.cut(source, line.end, level).1;
            if trim_end_spaces(&source[text.at..line.end]).is_empty() {
                if matches!(start.end, End::Blank) {
                    break;
                }
                blanks.push(cut);
            } else if text.column < level {
                break;
            } else {
                lines.append(&mut blanks);
                lines.push(cut);
                block_end = line.end;
                self.pos = line.next;
                open = !start.end.closes(&source[line.start..line.end]);
            }
            scan = line.next;
        }
        let value = join(&lines);
        self.push(TokenKind::Html { value }, from..block_end);
    }

    /// Reads an indented code block whose first line's white space starts at
    /// `white`, from its code column `column`: that line, then every line
    /// indented at least as far, with the blank lines between them.
    fn indented_code(&mut self, white: Spot, column: usize) {
        let source = self.source;
        let first = line_at(source, white.at);
        let (start, first_cut) = white.cut(source, first.end, column);
        let mut lines = vec![first_cut];
        let mut block_end = first.end;
        let mut blanks = Vec::new();
        let mut scan = self.pos;
        while scan < source.len() {
            let line = line_from(source, scan, self.base);
            let leading = Spot::start(&line);
            let text = leading.text(source, line.end);
            let cut = leading.cut(source, line.end, column).1;
            if trim_end_spaces(&source[text.at..line.end]).is_empty() {
                blanks.push(cut);
            } else if text.column >= column {
                lines.append(&mut blanks);
                lines.push(cut);
                block_end = line.end;
                self.pos = line.next;
            } else {
                break;
            }
            scan = line.next;
        }
        let kind = TokenKind::CodeBlock {
            content: start..block_end,
            info: start..start,
            value: join(&lines),
        };
        self.push(kind, start..block_end);
    }

    /// The content column of the innermost open item, or zero.
    fn level(&self) -> usize {
        self.levels.last().copied().unwrap_or(0)
    }

    fn line(&mut self) {
        let source = self.source;
        let line_start = self.pos;
        let line = line_from(source, line_start, self.base);
        self.pos = line.next;
        let white = Spot::start(&line);
        let text = white.text(source, line.end);
        let trimmed = trim_end_spaces(&source[text.at..line.end]);
        if trimmed.is_empty() {
            // An item whose marker line is empty ends at a blank line after it.
            if self
                .tokens
                .last()
                .is_some_and(|token| matches!(token.kind, TokenKind::ItemStart(_)))
            {
                self.levels.pop();
                self.push(TokenKind::ItemEnd, line.start..line.start);
            }
            self.push(TokenKind::Blank, line.start..line.start);
            return;
        }
        let open = self.levels.len();
        let after_text = self
            .tokens
            .last()
            .is_some_and(|token| matches!(token.kind, TokenKind::Text));
        self.close_items(text.column, line.start);
        let continues = after_text && self.levels.len() == open;
        self.content(line_start, white, text.at + trimmed.len(), continues);
    }

    fn push(&mut self, kind: TokenKind, span: Range<usize>) {
        self.tokens.push(Token { kind, span });
    }

    /// The block that the text at `text`, up to `end`, on the line starting
    /// at `line_start`, starts, read on from `next`, tried in the order of
    /// precedence `CommonMark` gives them. `continues` is set when the line
    /// would continue a paragraph, which only some blocks may interrupt. An
    /// item or a table whose content would lie deeper than `BLOCK_CAP` is
    /// not opened, and its line is text.
    fn start(
        &self,
        line_start: usize,
        text: Spot,
        end: usize,
        next: usize,
        continues: bool,
    ) -> Option<Start> {
        let rest = &self.source[text.at..end];
        if continues && let Some(level) = underline(rest) {
            return Some(Start::Underline(level));
        }
        if let Some((mark, length)) = fence(rest) {
            return Some(Start::Fence { length, mark });
        }
        if is_thematic_break(rest) {
            return Some(Start::Break);
        }
        if let Some((level, title)) = heading(rest) {
            return Some(Start::Heading { level, text: title });
        }
        if rest.starts_with('>') {
            return Some(Start::Quote);
        }
        if let Some(start) = html::start(rest).filter(|start| !continues || start.interrupts) {
            return Some(Start::Html(start));
        }
        let opens = self.depth() < BLOCK_CAP;
        if let Some((marker, width)) = list_marker(rest).filter(|&(marker, width)| {
            // An empty item never interrupts a paragraph.
            !continues
                || (marker.interrupts()
                    && !rest[width..].trim_start_matches([' ', '\t']).is_empty())
        }) {
            return opens.then_some(Start::Item { marker, width });
        }
        if !opens {
            return None;
        }
        table::read(
            self.source,
            line_start,
            text.column,
            text.at..end,
            next,
            self.level(),
            self.base,
        )
        .map(|(table, span, after)| Start::Table {
            next: after,
            span,
            table,
        })
    }
}

/// The tokens of `source` from byte `start`, each line read past `base`, so
/// its columns count from `base.columns`. `outer` containers hold the text.
pub fn lex_from(source: &str, start: usize, base: Base<'_>, outer: usize) -> Vec<Token> {
    let mut lexer = Lexer {
        base,
        levels: Vec::new(),
        outer,
        pos: start,
        source,
        tokens: Vec::new(),
    };
    while lexer.pos < source.len() {
        lexer.line();
    }
    lexer.close_items(0, source.len());
    lexer.tokens
}

/// The level of an ATX heading and the range of its text: from the first
/// character after the spaces and tabs that follow the `#`s, up to the
/// spaces and tabs before the closing sequence, a final run of `#`s that a
/// space or a tab precedes or that is the whole text.
fn heading(rest: &str) -> Option<(u8, Range<usize>)> {
    let text = rest.trim_start_matches('#');
    let level = u8::try_from(rest.len() - text.len())
        .ok()
        .filter(|level| (1..=6).contains(level))?;
    if !text.is_empty() && !text.starts_with([' ', '\t']) {
        return None;
    }
    let title = text.trim_start_matches([' ', '\t']);
    let start = rest.len() - title.len();
    let before_run = title.trim_end_matches('#');
    let end = if before_run.is_empty() || before_run.ends_with([' ', '\t']) {
        start + trim_end_spaces(before_run).len()
    } else {
        rest.len()
    };
    Some((level, start..end))
}

/// The character and the length of an opening fence: three or more
/// backticks, whose info string holds no backtick, or three or more tildes.
pub fn fence(rest: &str) -> Option<(char, usize)> {
    let mark = rest
        .chars()
        .next()
        .filter(|mark| matches!(mark, '`' | '~'))?;
    let info = rest.trim_start_matches(mark);
    let length = rest.len() - info.len();
    (length >= 3 && (mark == '~' || !info.contains('`'))).then_some((mark, length))
}

/// The list marker `rest` starts with, and its width: a bullet, or one to
/// nine digits and a `.` or `)`, followed by a space, a tab or the end of
/// the line.
fn list_marker(rest: &str) -> Option<(Marker, usize)> {
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (marker, width) = match digits {
        0 => {
            let bullet = rest
                .chars()
                .next()
                .filter(|bullet| matches!(bullet, '-' | '*' | '+'))?;
            (Marker::Bullet(bullet), 1)
        }
        1..=9 => {
            let delimiter = rest[digits..]
                .chars()
                .next()
                .filter(|delimiter| matches!(delimiter, '.' | ')'))?;
            let number = rest[..digits].parse().ok()?;
            (Marker::Ordered { delimiter, number }, digits + 1)
        }
        _ => return None,
    };
    matches!(rest[width..].chars().next(), None | Some(' ' | '\t')).then_some((marker, width))
}

/// Three or more of the same `-`, `*` or `_`, with nothing but spaces and
/// tabs between them.
fn is_thematic_break(rest: &str) -> bool {
    let Some(mark) = rest.chars().next().filter(|mark| "-*_".contains(*mark)) else {
        return false;
    };
    rest.chars()
        .all(|character| character == mark || matches!(character, ' ' | '\t'))
        && rest.chars().filter(|character| *character == mark).count() >= 3
}

/// The level of the heading that `rest`, a run of `=` or of `-` under
/// paragraph text, underlines: 1 for `=`, 2 for `-`.
fn underline(rest: &str) -> Option<u8> {
    let mark = rest.chars().next()?;
    let level = match mark {
        '=' => 1,
        '-' => 2,
        _ => return None,
    };
    rest.chars()
        .all(|character| character == mark)
        .then_some(level)
}

/// `text` without the spaces and tabs at its end: a line holding nothing
/// else is blank.
pub fn trim_end_spaces(text: &str) -> &str {
    text.trim_end_matches([' ', '\t'])
}

/// The quoted line whose `>` is at `quote`, up to `end`, without its `>`
/// and one column of the space or tab after it: a tab there leaves its
/// other columns as spaces.
fn unquote(source: &str, quote: Spot, end: usize) -> Cut<'_> {
    let after = quote.after(1);
    after.cut(source, end, after.column + 1).1
}

/// `lines` joined by `\n`, each with its spaces before its text.
fn join(lines: &[Cut<'_>]) -> String {
    let mut value = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            value.push('\n');
        }
        value.extend(iter::repeat_n(' ', line.spaces));
        value.push_str(line.text);
    }
    value
}

/// Reads `lines`, ranges of `source`, as consecutive lines of one text, each
/// read past `base`, the way `lex_from` reads a file, held by `outer`
/// containers. Spans count from the start of `source`.
pub fn lex_lines(source: &str, lines: &[Range<usize>], base: Base<'_>, outer: usize) -> Vec<Token> {
    let mut starts = Vec::with_capacity(lines.len());
    let mut text = String::new();
    for line in lines {
        if !starts.is_empty() {
            text.push('\n');
        }
        starts.push(text.len());
        text.push_str(&source[line.clone()]);
    }
    let place = |at: usize| {
        let index = starts
            .partition_point(|&start| start <= at)
            .saturating_sub(1);
        lines
            .get(index)
            .zip(starts.get(index))
            .map_or(at, |(line, &start)| line.start + (at - start))
    };
    let mut tokens = lex_from(&text, 0, base, outer);
    for token in &mut tokens {
        token.span = place(token.span.start)..place(token.span.end);
        match &mut token.kind {
            TokenKind::CodeBlock {
                content,
                info,
                value: _,
            } => {
                *content = place(content.start)..place(content.end);
                *info = place(info.start)..place(info.end);
            }
            TokenKind::Heading {
                level: _,
                text: range,
            } => *range = place(range.start)..place(range.end),
            TokenKind::Table { depth: _, table } => table.place(&place),
            TokenKind::Blank
            | TokenKind::BlockQuote { value: _ }
            | TokenKind::Definition {
                label: _,
                target: _,
            }
            | TokenKind::Html { value: _ }
            | TokenKind::ItemEnd
            | TokenKind::ItemStart(_)
            | TokenKind::Text
            | TokenKind::ThematicBreak
            | TokenKind::Underline(_) => {}
        }
    }
    tokens
}
