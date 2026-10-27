//! Splitting the source into lines, and counting the columns of the white
//! space that starts a text.

/// One line of the source. `end` excludes the line ending; `next` is where
/// the following line starts. Its tab stops count from its column 0, and
/// `start` is at its column `column`.
#[derive(Clone, Copy)]
pub struct Line {
    pub column: usize,
    pub end: usize,
    pub next: usize,
    pub start: usize,
}

/// What each line is read past: each column of `frame` in turn, then at
/// most `columns` columns of the white space left. The line's tab stops
/// restart where each ends, so they count from its column 0.
#[derive(Clone, Copy, Default)]
pub struct Base<'frame> {
    pub columns: usize,
    pub frame: &'frame [usize],
}

/// The white space that starts a text: the bytes it takes and the columns
/// it spans.
#[derive(Clone, Copy)]
pub struct Space {
    pub bytes: usize,
    pub columns: usize,
}

pub fn line_at(source: &str, start: usize) -> Line {
    let (raw_end, next) = source[start..]
        .find('\n')
        .map_or((source.len(), source.len()), |at| {
            (start + at, start + at + 1)
        });
    let end = if source[start..raw_end].ends_with('\r') {
        raw_end - 1
    } else {
        raw_end
    };
    Line {
        column: 0,
        end,
        next,
        start,
    }
}

/// The line at `start`, read past `base`. A tab that a column of `base`
/// falls inside is passed, and its columns past it are the line's first
/// columns, before `start`. The line's tab stops count from its column 0.
pub fn line_from(source: &str, start: usize, base: Base<'_>) -> Line {
    let framed = base
        .frame
        .iter()
        .fold(line_at(source, start), |line, &columns| {
            past(source, line, columns)
        });
    past(source, framed, base.columns)
}

/// `line` read past at most `columns` more columns of the white space at
/// its start, its tab stops restarting there, so they count from its new
/// column 0.
fn past(source: &str, line: Line, columns: usize) -> Line {
    let left_out = space_to(&source[line.start..line.end], line.column, columns);
    let reached = line.column + left_out.columns;
    Line {
        column: reached - reached.min(columns),
        start: line.start + left_out.bytes,
        ..line
    }
}

/// The spaces and tabs that start `text`, whose first byte is at `column` of
/// its source line, a tab moving to the next multiple of 4.
pub fn space(text: &str, column: usize) -> Space {
    space_to(text, column, usize::MAX)
}

/// The spaces and tabs that start `text`, whose first byte is at `column` of
/// its source line, up to column `limit`, counted as `space` counts them. A
/// tab that `limit` falls inside is taken whole.
pub fn space_to(text: &str, column: usize, limit: usize) -> Space {
    let mut reached = column;
    let mut bytes = 0;
    for byte in text.bytes() {
        if reached >= limit {
            break;
        }
        reached = match byte {
            b' ' => reached + 1,
            b'\t' => next_stop(reached),
            _ => break,
        };
        bytes += 1;
    }
    Space {
        bytes,
        columns: reached - column,
    }
}

/// The column that `text`, written from column `from` of its line, ends at:
/// a tab moves to the next multiple of 4, and any other character one column
/// on.
pub fn width(text: &str, from: usize) -> usize {
    text.chars().fold(from, |column, character| {
        if character == '\t' {
            next_stop(column)
        } else {
            column + 1
        }
    })
}

/// The column a tab at `column` moves to.
const fn next_stop(column: usize) -> usize {
    column + 4 - column % 4
}
