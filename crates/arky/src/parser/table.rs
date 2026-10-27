//! Tables, read whole: which lines form a table, and where its cells are.
//! Lining cells up in columns is not context-free, so a table reaches the
//! grammar as one token.

use core::iter;
use core::ops::Range;

use crate::parser::ast::{Alignment, TableFormat};
use crate::parser::lexer::trim_end_spaces;
use crate::parser::line::{Base, line_from, space};

pub struct TableToken {
    pub alignments: Vec<Alignment>,
    pub format: TableFormat,
    pub head: Vec<RowToken>,
    pub rows: Vec<RowToken>,
}

impl TableToken {
    /// Moves every offset the table holds to where `place` puts it.
    pub fn place<F>(&mut self, place: &F)
    where
        F: Fn(usize) -> usize,
    {
        for row in self.head.iter_mut().chain(&mut self.rows) {
            row.span = place(row.span.start)..place(row.span.end);
            for line in row.cells.iter_mut().flat_map(|cell| &mut cell.lines) {
                *line = place(line.start)..place(line.end);
            }
        }
    }
}

pub struct RowToken {
    pub cells: Vec<Cell>,
    pub span: Range<usize>,
}

pub struct Cell {
    /// The cell's text on each line of its row, top to bottom.
    pub lines: Vec<Range<usize>>,
}

impl Cell {
    /// A cell written on one line.
    pub fn line(range: Range<usize>) -> Self {
        Self { lines: vec![range] }
    }

    /// From the first line that holds text to the end of the last one, or
    /// empty at the start of the first line when none does.
    pub fn span(&self) -> Range<usize> {
        let written = |line: &&Range<usize>| !line.is_empty();
        let first = self.lines.iter().find(written);
        let last = self.lines.iter().rfind(written);
        first.zip(last).map_or_else(
            || {
                let at = self.lines.first().map_or(0, |line| line.start);
                at..at
            },
            |(from, to)| from.start..to.end,
        )
    }
}

/// One line of a table: its text from `start` to `end`, the column the text
/// starts at, the start of the line it is on, and where the next line starts.
#[derive(Clone)]
struct TableLine {
    column: usize,
    end: usize,
    line_start: usize,
    next: usize,
    start: usize,
}

/// The table whose first line holds `first`, at `column`, read on from
/// `next`. Every line is read past `base`, and every later one is indented
/// at least `level` columns. Returns the table, its span, and where the line
/// after it starts.
pub fn read(
    source: &str,
    line_start: usize,
    column: usize,
    first: Range<usize>,
    next: usize,
    level: usize,
    base: Base<'_>,
) -> Option<(TableToken, Range<usize>, usize)> {
    let head = TableLine {
        column,
        end: first.end,
        line_start,
        next,
        start: first.start,
    };
    pipe(source, &head, level, base)
        .or_else(|| grid(source, &head, level, base))
        .or_else(|| simple(source, &head, level, base))
}

/// The line starting at `at`, when it holds text and is indented at least
/// `level` columns once read past `base`.
fn line_after(source: &str, at: usize, level: usize, base: Base<'_>) -> Option<TableLine> {
    if at >= source.len() {
        return None;
    }
    let line = line_from(source, at, base);
    let text = &source[line.start..line.end];
    let indent = space(text, line.column);
    let column = line.column + indent.columns;
    let trimmed = trim_end_spaces(&text[indent.bytes..]);
    (!trimmed.is_empty() && column >= level).then(|| TableLine {
        column,
        end: line.start + indent.bytes + trimmed.len(),
        line_start: at,
        next: line.next,
        start: line.start + indent.bytes,
    })
}

/// A header row, a delimiter row such as `|:--|--:|`, and the rows after
/// them up to the first line without a pipe. The table is as wide as the
/// widest of them; a column the delimiter row does not reach has the
/// default alignment.
fn pipe(
    source: &str,
    head: &TableLine,
    level: usize,
    base: Base<'_>,
) -> Option<(TableToken, Range<usize>, usize)> {
    let head_cells = pipe_cells(source, head)?;
    let delimiter = line_after(source, head.next, level, base)?;
    let delimiter_cells = pipe_cells(source, &delimiter)?;
    let mut alignments = delimiter_cells
        .iter()
        .map(|cell| alignment(&source[cell.clone()], '-'))
        .collect::<Option<Vec<_>>>()?;
    let mut rows = Vec::new();
    let mut last = delimiter;
    while let Some(line) = line_after(source, last.next, level, base) {
        let Some(cells) = pipe_cells(source, &line) else {
            break;
        };
        rows.push(RowToken {
            cells: cells.into_iter().map(Cell::line).collect(),
            span: line.start..line.end,
        });
        last = line;
    }
    let width = rows
        .iter()
        .map(|row| row.cells.len())
        .chain([head_cells.len(), alignments.len()])
        .max()
        .unwrap_or_default();
    alignments.resize(width, Alignment::Default);
    let table = TableToken {
        alignments,
        format: TableFormat::Pipe,
        head: vec![RowToken {
            cells: head_cells.into_iter().map(Cell::line).collect(),
            span: head.start..head.end,
        }],
        rows,
    };
    Some((table, head.start..last.end, last.next))
}

/// The cells of a pipe row: the text between unescaped pipes, with an
/// outer pipe at either end optional. `None` when the line has no pipe.
fn pipe_cells(source: &str, line: &TableLine) -> Option<Vec<Range<usize>>> {
    let text = &source[line.start..line.end];
    let bytes = text.as_bytes();
    let mut pipes = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'|' => {
                pipes.push(at);
                at += 1;
            }
            _ => at += 1,
        }
    }
    let (&first, &last) = pipes.first().zip(pipes.last())?;
    let begin = usize::from(first == 0);
    let finish = if last + 1 == text.len() && last >= begin {
        last
    } else {
        text.len()
    };
    let walls: Vec<usize> = pipes
        .into_iter()
        .filter(|&pipe| pipe >= begin && pipe < finish)
        .collect();
    let mut cells = Vec::new();
    let mut from = begin;
    for wall in walls.into_iter().chain([finish]) {
        cells.push(trim(source, line.start + from..line.start + wall));
        from = wall + 1;
    }
    Some(cells)
}

/// A table drawn with `+---+` borders and `|` walls. Every line between two
/// borders belongs to one row. A `+===+` border ends the header.
fn grid(
    source: &str,
    head: &TableLine,
    level: usize,
    base: Base<'_>,
) -> Option<(TableToken, Range<usize>, usize)> {
    let top: Vec<char> = source[head.start..head.end].chars().collect();
    let (corners, mut alignments) = border(&top, '-')?;
    let mut head_rows = Vec::new();
    let mut rows = Vec::new();
    let mut last = head.clone();
    loop {
        let mut lines: Vec<TableLine> = Vec::new();
        let mut next = last.next;
        while let Some(line) = line_after(source, next, level, base).filter(|line| {
            line.column == head.column && source[line.start..line.end].starts_with('|')
        }) {
            next = line.next;
            lines.push(line);
        }
        let Some((first, final_line)) = lines.first().zip(lines.last()) else {
            break;
        };
        rows.push(RowToken {
            cells: grid_cells(source, &lines, &corners),
            span: first.start..final_line.end,
        });
        let closing = line_after(source, next, level, base)
            .filter(|closing| closing.column == head.column)?;
        let closing_chars: Vec<char> = source[closing.start..closing.end].chars().collect();
        if let Some((found, header)) = border(&closing_chars, '=') {
            if found != corners || !head_rows.is_empty() {
                return None;
            }
            head_rows.append(&mut rows);
            alignments = header;
        } else {
            let (found, _) = border(&closing_chars, '-')?;
            if found != corners {
                return None;
            }
        }
        last = closing;
    }
    if rows.is_empty() && head_rows.is_empty() {
        return None;
    }
    let table = TableToken {
        alignments,
        format: TableFormat::Grid,
        head: head_rows,
        rows,
    };
    Some((table, head.start..last.end, last.next))
}

/// The columns of a `+` corner in a border line made of `fill`, and the
/// alignment each segment's colons set.
fn border(chars: &[char], fill: char) -> Option<(Vec<usize>, Vec<Alignment>)> {
    if chars.len() < 3 || chars.first() != Some(&'+') || chars.last() != Some(&'+') {
        return None;
    }
    let corners: Vec<usize> = chars
        .iter()
        .enumerate()
        .filter_map(|(column, &character)| (character == '+').then_some(column))
        .collect();
    let alignments = corners
        .iter()
        .zip(corners.iter().skip(1))
        .map(|(&from, &to)| {
            let segment: String = chars[from + 1..to].iter().collect();
            alignment(&segment, fill)
        })
        .collect::<Option<Vec<_>>>()?;
    Some((corners, alignments))
}

/// The cells of a grid row written on `lines`, one per pair of the border's
/// corners. A cell's lines keep their indentation relative to each other.
fn grid_cells(source: &str, lines: &[TableLine], corners: &[usize]) -> Vec<Cell> {
    let columns: Vec<Vec<Range<usize>>> = lines
        .iter()
        .map(|line| grid_columns(source, line, corners))
        .collect();
    (0..corners.len().saturating_sub(1))
        .map(|column| {
            let written: Vec<Range<usize>> = columns
                .iter()
                .map(|ranges| ranges[column].clone())
                .collect();
            let indent = written
                .iter()
                .map(|range| &source[range.clone()])
                .filter(|text| !text.trim_matches([' ', '\t']).is_empty())
                .map(|text| text.len() - text.trim_start_matches([' ', '\t']).len())
                .min()
                .unwrap_or_default();
            let trimmed = written
                .into_iter()
                .map(|range| {
                    let start = (range.start + indent).min(range.end);
                    let text = source[start..range.end].trim_end_matches([' ', '\t']);
                    start..start + text.len()
                })
                .collect();
            Cell { lines: trimmed }
        })
        .collect()
}

/// Where each column's text is on one line of a grid row. A column runs
/// from its left corner to its right one: a `|` at a corner is a wall, and
/// a `|` that ends the line closes it, so a wall out of line stays text.
fn grid_columns(source: &str, line: &TableLine, corners: &[usize]) -> Vec<Range<usize>> {
    let text = &source[line.start..line.end];
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let wall = |column: usize| {
        chars
            .get(column)
            .is_some_and(|&(_, character)| character == '|')
    };
    let close = if chars.len() > 1 && text.ends_with('|') {
        chars.len() - 1
    } else {
        chars.len()
    };
    let byte_at =
        |column: usize| line.start + chars.get(column).map_or(text.len(), |&(offset, _)| offset);
    let count = corners.len().saturating_sub(1);
    corners
        .iter()
        .zip(corners.iter().skip(1))
        .enumerate()
        .map(|(index, (&left, &right))| {
            let from = (left + usize::from(wall(left))).min(close);
            let to = if index + 1 == count {
                close
            } else {
                right.min(close)
            };
            byte_at(from)..byte_at(to)
        })
        .collect()
}

/// A header line, a line of dash groups marking the columns, and the rows
/// after them up to a blank line.
fn simple(
    source: &str,
    head: &TableLine,
    level: usize,
    base: Base<'_>,
) -> Option<(TableToken, Range<usize>, usize)> {
    let dashes = line_after(source, head.next, level, base)?;
    let groups = dash_groups(&columns(source, &dashes, base))?;
    let mut lines = Vec::new();
    let mut last = dashes;
    while let Some(line) = line_after(source, last.next, level, base) {
        lines.push(line.clone());
        last = line;
    }
    if lines.is_empty() {
        return None;
    }
    let head_columns = columns(source, head, base);
    let head_cells = simple_cells(source, head, &head_columns, &groups);
    let alignments = head_cells
        .iter()
        .zip(&groups)
        .map(|(cell, group)| {
            let left = column_of(&head_columns, cell.start);
            let right = column_of(&head_columns, cell.end);
            match (left == group.start, right == group.end) {
                _ if cell.is_empty() => Alignment::Default,
                (true, true) => Alignment::Default,
                (true, false) => Alignment::Left,
                (false, true) => Alignment::Right,
                (false, false) => Alignment::Center,
            }
        })
        .collect();
    let row = |line: &TableLine, cells: Vec<Range<usize>>| RowToken {
        cells: cells.into_iter().map(Cell::line).collect(),
        span: line.start..line.end,
    };
    let rows = lines
        .iter()
        .map(|line| {
            row(
                line,
                simple_cells(source, line, &columns(source, line, base), &groups),
            )
        })
        .collect();
    let table = TableToken {
        alignments,
        format: TableFormat::Simple,
        head: vec![row(head, head_cells)],
        rows,
    };
    Some((table, head.start..last.end, last.next))
}

/// The columns of each run of dashes in a line made only of dashes, spaces
/// and tabs. At least two runs.
fn dash_groups(columns: &[(usize, char)]) -> Option<Vec<Range<usize>>> {
    if !columns
        .iter()
        .all(|&(_, character)| matches!(character, '-' | ' ' | '\t'))
    {
        return None;
    }
    let mut groups: Vec<Range<usize>> = Vec::new();
    for (column, &(_, character)) in columns.iter().enumerate() {
        if character != '-' {
            continue;
        }
        match groups.last_mut() {
            Some(group) if group.end == column => group.end = column + 1,
            Some(_) | None => groups.push(column..column + 1),
        }
    }
    (groups.len() >= 2).then_some(groups)
}

/// The cells of a simple row. Each column runs from its dash group to the
/// next group, the first one from the start of the line's text, and a word
/// that crosses into the next column stays in the column it starts in.
fn simple_cells(
    source: &str,
    line: &TableLine,
    columns: &[(usize, char)],
    groups: &[Range<usize>],
) -> Vec<Range<usize>> {
    let written = |column: usize| {
        columns
            .get(column)
            .is_some_and(|&(_, character)| !matches!(character, ' ' | '\t'))
    };
    let mut edges: Vec<usize> = Vec::new();
    for group in groups.iter().skip(1) {
        let mut edge = edges
            .last()
            .map_or(group.start, |&last| group.start.max(last));
        while edge > 0 && written(edge - 1) && written(edge) {
            edge += 1;
        }
        edges.push(edge);
    }
    let byte_at = |column: usize| {
        columns
            .get(column)
            .map_or(line.end, |&(at, _)| at)
            .max(line.start)
    };
    let starts = iter::once(0).chain(edges.iter().copied());
    let ends = edges.iter().copied().chain([columns.len()]);
    starts
        .zip(ends)
        .map(|(from, to)| trim(source, byte_at(from)..byte_at(to)))
        .collect()
}

/// The byte and the character at each column of `line` read past `base`,
/// from its column 0 on: a tab fills the columns up to its next tab stop.
fn columns(source: &str, line: &TableLine, base: Base<'_>) -> Vec<(usize, char)> {
    let read = line_from(source, line.line_start, base);
    let text = &source[read.start..line.end];
    let split = read.start - usize::from(read.column > 0);
    let mut columns = vec![(split, '\t'); read.column];
    let mut reached = read.column;
    for (at, character) in text.char_indices() {
        let next = if character == '\t' {
            reached + space(&text[at..=at], reached).columns
        } else {
            reached + 1
        };
        columns.extend(iter::repeat_n((read.start + at, character), next - reached));
        reached = next;
    }
    columns
}

/// The column that byte `at` starts at among `columns`.
fn column_of(columns: &[(usize, char)], at: usize) -> usize {
    columns.partition_point(|&(byte, _)| byte < at)
}

/// The alignment a cell such as `:--:` sets: a colon at the left, the
/// right or both, around one or more `fill` characters.
fn alignment(text: &str, fill: char) -> Option<Alignment> {
    let left = text.starts_with(':');
    let right = text.len() > 1 && text.ends_with(':');
    let opened = text.strip_prefix(':').unwrap_or(text);
    let inner = opened.strip_suffix(':').unwrap_or(opened);
    if inner.is_empty() || !inner.chars().all(|character| character == fill) {
        return None;
    }
    Some(match (left, right) {
        (true, true) => Alignment::Center,
        (true, false) => Alignment::Left,
        (false, true) => Alignment::Right,
        (false, false) => Alignment::Default,
    })
}

/// `range` without the spaces and tabs at either end.
fn trim(source: &str, range: Range<usize>) -> Range<usize> {
    let text = &source[range.clone()];
    let start = range.start + (text.len() - text.trim_start_matches([' ', '\t']).len());
    let end = range.end - (text.len() - text.trim_end_matches([' ', '\t']).len());
    if start > end {
        start..start
    } else {
        start..end
    }
}
