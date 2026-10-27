//! Tables: a pipe table with each column as wide as its widest cell, or a
//! grid table when a cell needs more than one line or the table does not
//! have exactly one header row.

use crate::parser::ast::{Alignment, Table, TableCell, TableRow};
use crate::parser::extension::{Extension, Written};
use crate::writer::Writer;

/// The narrowest a column is written.
const NARROWEST: usize = 3;

/// A row as it is laid out: its cells' text, or what an extension wrote for
/// a row it claimed, which is written as it is.
pub enum Row {
    Cells(Vec<String>),
    Claimed(String),
}

/// The text of `cell`'s content, with its levels counted from one. `writer`
/// keeps the error met while writing it.
fn cell<T>(writer: &mut Writer, cell: &TableCell<T>) -> String
where
    T: Extension,
{
    let mut inner = writer.inner();
    inner.node(&cell.kind, Written::TableCell(cell));
    writer.keep_error(&mut inner);
    inner.finish().trim_end_matches('\n').to_owned()
}

/// The cells of `row`, `width` of them, empty ones added at the end, or
/// what an extension wrote for it.
fn row<T>(writer: &mut Writer, row: &TableRow<T>, width: usize) -> Row
where
    T: Extension,
{
    if let Some(node) = row.kind.0.as_deref() {
        let text = writer.claimed(node, Written::TableRow(row));
        return Row::Claimed(text.trim_end_matches('\n').to_owned());
    }
    let mut cells: Vec<String> = row.cells.iter().map(|each| cell(writer, each)).collect();
    cells.resize(width.max(cells.len()), String::new());
    Row::Cells(cells)
}

/// `table` as a pipe table, or as a grid table when a cell needs more than
/// one line or it does not have exactly one header row.
pub fn write<T>(writer: &mut Writer, table: &Table<T>)
where
    T: Extension,
{
    let width = table
        .head
        .iter()
        .chain(&table.rows)
        .map(|each| each.cells.len())
        .chain([table.alignments.len()])
        .max()
        .unwrap_or_default();
    let head: Vec<Row> = table
        .head
        .iter()
        .map(|each| row(writer, each, width))
        .collect();
    let rows: Vec<Row> = table
        .rows
        .iter()
        .map(|each| row(writer, each, width))
        .collect();
    lay_out(writer, &table.alignments, width, &head, &rows);
}

/// `head` and `rows`, of `width` cells, as a pipe table, or as a grid table
/// when a cell needs more than one line or there is not exactly one header
/// row.
pub fn lay_out(
    writer: &mut Writer,
    alignments: &[Alignment],
    width: usize,
    head: &[Row],
    rows: &[Row],
) {
    let one_line = head.iter().chain(rows).all(|each| match each {
        Row::Cells(cells) => cells.iter().all(|text| !text.contains('\n')),
        Row::Claimed(_) => true,
    });
    let mut out = String::new();
    if head.len() == 1 && one_line {
        pipe(&mut out, alignments, width, head, rows);
    } else {
        grid(&mut out, alignments, width, head, rows);
    }
    writer.put(&out);
}

/// `row` as a line of a pipe table, its cells not padded.
pub fn pipe_row<T>(writer: &mut Writer, row: &TableRow<T>) -> String
where
    T: Extension,
{
    let mut line = "|".to_owned();
    for each in &row.cells {
        line.push(' ');
        line.push_str(&escape_pipes(&cell(writer, each)));
        line.push_str(" |");
    }
    line
}

/// The `alignment` of a column on a rule of `length` characters of `fill`:
/// a `:` at the left, the right, or both.
fn rule(alignment: Alignment, fill: char, length: usize) -> String {
    let (left, right) = match alignment {
        Alignment::Center => (true, true),
        Alignment::Default => (false, false),
        Alignment::Left => (true, false),
        Alignment::Right => (false, true),
    };
    let mut written = String::new();
    if left {
        written.push(':');
    }
    for _ in usize::from(left) + usize::from(right)..length {
        written.push(fill);
    }
    if right {
        written.push(':');
    }
    written
}

/// The alignment of column `column`, the default past the last one given.
fn alignment_of(alignments: &[Alignment], column: usize) -> Alignment {
    alignments
        .get(column)
        .copied()
        .unwrap_or(Alignment::Default)
}

/// Each column's width: its widest line of text, and at least
/// `NARROWEST`.
fn widths(rows: &[&Row], width: usize) -> Vec<usize> {
    let mut widths = vec![NARROWEST; width];
    for each in rows {
        if let Row::Cells(cells) = each {
            for (column, text) in cells.iter().enumerate() {
                let widest = text.split('\n').map(|line| line.chars().count()).max();
                if let Some((current, found)) = widths.get_mut(column).zip(widest) {
                    *current = (*current).max(found);
                }
            }
        }
    }
    widths
}

/// `text` followed by spaces up to `width` characters.
fn padded(out: &mut String, text: &str, width: usize) {
    out.push_str(text);
    for _ in text.chars().count()..width {
        out.push(' ');
    }
}

/// `text` with a backslash before each `|` that a pipe row would read as
/// a wall: one that no backslash escapes.
fn escape_pipes(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        match character {
            '\\' => {
                escaped.push(character);
                if let Some(next) = chars.next() {
                    escaped.push(next);
                }
            }
            '|' => escaped.push_str("\\|"),
            _ => escaped.push(character),
        }
    }
    escaped
}

/// A header row, a dash line with a `:` by alignment, then the rows, each
/// cell padded to its column's width.
fn pipe(out: &mut String, alignments: &[Alignment], width: usize, head: &[Row], rows: &[Row]) {
    let escaped: Vec<Row> = head
        .iter()
        .chain(rows)
        .map(|each| match each {
            Row::Cells(cells) => Row::Cells(cells.iter().map(|text| escape_pipes(text)).collect()),
            Row::Claimed(text) => Row::Claimed(text.clone()),
        })
        .collect();
    let all: Vec<&Row> = escaped.iter().collect();
    let widths = widths(&all, width);
    let line = |written: &mut String, each: &Row| {
        match each {
            Row::Cells(cells) => {
                written.push('|');
                for (text, &column_width) in cells.iter().zip(&widths) {
                    written.push(' ');
                    padded(written, text, column_width);
                    written.push_str(" |");
                }
            }
            Row::Claimed(text) => written.push_str(text),
        }
        written.push('\n');
    };
    let (header, body) = escaped.split_at(head.len());
    for each in header {
        line(out, each);
    }
    out.push('|');
    for (column, &column_width) in widths.iter().enumerate() {
        out.push(' ');
        out.push_str(&rule(alignment_of(alignments, column), '-', column_width));
        out.push_str(" |");
    }
    out.push('\n');
    for each in body {
        line(out, each);
    }
}

/// `+---+` borders around each row and `|` walls between its cells, a
/// `+===+` border under the header rows. The alignments go on the `+===+`
/// border, or on the top one when there is no header.
fn grid(out: &mut String, alignments: &[Alignment], width: usize, head: &[Row], rows: &[Row]) {
    let all: Vec<&Row> = head.iter().chain(rows).collect();
    let widths = widths(&all, width);
    let border = |written: &mut String, fill: char, aligned: bool| {
        written.push('+');
        for (column, &column_width) in widths.iter().enumerate() {
            let alignment = if aligned {
                alignment_of(alignments, column)
            } else {
                Alignment::Default
            };
            written.push_str(&rule(alignment, fill, column_width + 2));
            written.push('+');
        }
        written.push('\n');
    };
    let lines = |written: &mut String, each: &Row| match each {
        Row::Cells(cells) => {
            let split: Vec<Vec<&str>> = cells
                .iter()
                .map(|text| text.split('\n').collect())
                .collect();
            let height = split.iter().map(Vec::len).max().unwrap_or(1).max(1);
            for index in 0..height {
                written.push('|');
                for (column, &column_width) in split.iter().zip(&widths) {
                    written.push(' ');
                    padded(
                        written,
                        column.get(index).copied().unwrap_or_default(),
                        column_width,
                    );
                    written.push_str(" |");
                }
                written.push('\n');
            }
        }
        Row::Claimed(text) => {
            written.push_str(text);
            written.push('\n');
        }
    };
    border(out, '-', head.is_empty());
    for (index, each) in head.iter().enumerate() {
        lines(out, each);
        if index + 1 == head.len() {
            border(out, '=', true);
        } else {
            border(out, '-', false);
        }
    }
    for each in rows {
        lines(out, each);
        border(out, '-', false);
    }
}
