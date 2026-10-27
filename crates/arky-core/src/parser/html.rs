//! Where an HTML block starts and what ends it, by the seven kinds of HTML
//! block of `CommonMark`, and where inline HTML ends.

use std::collections::HashMap;

/// The tag names that start an HTML block of the sixth kind, in lower case.
const BLOCK_TAGS: [&str; 62] = [
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

/// The tag names whose content is raw text, in lower case.
const RAW_TAGS: [&str; 4] = ["pre", "script", "style", "textarea"];

/// The closing tags that end a block opened by one of `RAW_TAGS`.
const RAW_ENDS: [&str; 4] = ["</pre>", "</script>", "</style>", "</textarea>"];

/// What ends an HTML block.
#[derive(Clone, Copy)]
pub enum End {
    /// The last line before a blank line.
    Blank,
    /// The first line that holds one of these, in any case.
    Holds(&'static [&'static str]),
}

impl End {
    /// Whether `text`, part of a line, ends the block.
    pub fn closes(self, text: &str) -> bool {
        match self {
            Self::Blank => false,
            Self::Holds(ends) => {
                let lower = text.to_ascii_lowercase();
                ends.iter().any(|end| lower.contains(end))
            }
        }
    }
}

/// How an HTML block starts.
#[derive(Clone, Copy)]
pub struct Start {
    pub end: End,
    /// Whether the block may start on a line that would continue a
    /// paragraph.
    pub interrupts: bool,
    /// The length of the opening; the first line is searched for the end
    /// past it.
    pub opening: usize,
}

/// The last search in one text for each closer of a comment, a processing
/// instruction, a declaration or a CDATA section.
#[derive(Default)]
pub struct Searches {
    /// Each closer sought, by where its last search started and where it
    /// found the closer, if anywhere.
    last: HashMap<&'static str, (usize, Option<usize>)>,
}

impl Searches {
    /// Where `closer` first starts in `text` at or past `from`. The last
    /// search for it answers when it started at or before `from` and found
    /// the closer at or past `from`, or found none.
    fn find(&mut self, text: &str, closer: &'static str, from: usize) -> Option<usize> {
        if let Some(&(start, found)) = self.last.get(closer)
            && start <= from
            && found.is_none_or(|place| from <= place)
        {
            return found;
        }
        let found = text[from..].find(closer).map(|offset| from + offset);
        self.last.insert(closer, (from, found));
        found
    }
}

/// The HTML block `rest`, the text of a line, starts, if any.
pub fn start(rest: &str) -> Option<Start> {
    if !rest.starts_with('<') {
        return None;
    }
    let lower = rest.to_ascii_lowercase();
    let block = |end, opening| Start {
        end,
        interrupts: true,
        opening,
    };
    if let Some(name) = RAW_TAGS.iter().find(|name| opens(&lower, name)) {
        return Some(block(End::Holds(&RAW_ENDS), name.len() + 1));
    }
    if rest.starts_with("<!--") {
        return Some(block(End::Holds(&["-->"]), 4));
    }
    if rest.starts_with("<?") {
        return Some(block(End::Holds(&["?>"]), 2));
    }
    if rest
        .strip_prefix("<!")
        .is_some_and(|after| after.starts_with(|letter: char| letter.is_ascii_alphabetic()))
    {
        return Some(block(End::Holds(&[">"]), 2));
    }
    if rest.starts_with("<![CDATA[") {
        return Some(block(End::Holds(&["]]>"]), 9));
    }
    let name = &lower[if lower.starts_with("</") { 2 } else { 1 }..];
    if BLOCK_TAGS
        .iter()
        .filter_map(|tag| name.strip_prefix(tag))
        .any(|tail| tail.is_empty() || tail.starts_with([' ', '\t', '>']) || tail.starts_with("/>"))
    {
        return Some(block(End::Blank, 0));
    }
    complete_tag(rest).then_some(Start {
        end: End::Blank,
        interrupts: false,
        opening: 0,
    })
}

/// Where the inline HTML at `at` in `text` ends, past its last character:
/// an open or closing tag, a comment, a processing instruction, a
/// declaration or a CDATA section. `searches` holds the earlier searches in
/// `text` for what ends them.
pub fn inline_end(text: &str, at: usize, searches: &mut Searches) -> Option<usize> {
    let rest = &text[at..];
    let mut closed = |opening: usize, end: &'static str| {
        searches
            .find(text, end, at + opening)
            .map(|found| found + end.len())
    };
    if rest.starts_with("<!-->") {
        return Some(at + 5);
    }
    if rest.starts_with("<!--->") {
        return Some(at + 6);
    }
    if rest.starts_with("<!--") {
        return closed(4, "-->");
    }
    if rest.starts_with("<?") {
        return closed(2, "?>");
    }
    if rest.starts_with("<![CDATA[") {
        return closed(9, "]]>");
    }
    if rest
        .strip_prefix("<!")
        .is_some_and(|after| after.starts_with(|letter: char| letter.is_ascii_alphabetic()))
    {
        return closed(2, ">");
    }
    tag_end(text.as_bytes(), at)
}

/// Whether `lower` opens the tag `name`: `<`, the name, then a space, a tab,
/// `>` or the end of the line.
fn opens(lower: &str, name: &str) -> bool {
    lower
        .strip_prefix('<')
        .and_then(|after| after.strip_prefix(name))
        .is_some_and(|tail| tail.is_empty() || tail.starts_with([' ', '\t', '>']))
}

/// Whether `rest`, which starts with `<`, is one whole opening or closing
/// tag, followed only by spaces and tabs.
fn complete_tag(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    tag_end(bytes, 0).is_some_and(|end| bytes[end..].iter().all(|&byte| is_space(byte)))
}

/// Where the opening or closing tag whose `<` is at `at` ends, past its
/// `>`. The spaces and tabs inside it may each hold one line ending.
fn tag_end(bytes: &[u8], at: usize) -> Option<usize> {
    let closing = bytes.get(at + 1) == Some(&b'/');
    let mut end = at + if closing { 2 } else { 1 };
    if !bytes.get(end).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    end = skip(bytes, end, |byte| {
        byte.is_ascii_alphanumeric() || byte == b'-'
    });
    if !closing {
        while let Some(next) = attribute(bytes, end) {
            end = next;
        }
    }
    end = gap(bytes, end);
    if !closing && bytes.get(end) == Some(&b'/') {
        end += 1;
    }
    (bytes.get(end) == Some(&b'>')).then_some(end + 1)
}

/// Where the attribute that starts at `at`, after at least one space or line
/// ending, ends: a name, then optionally `=` and a value.
fn attribute(bytes: &[u8], at: usize) -> Option<usize> {
    let name = gap(bytes, at);
    if name == at
        || !bytes
            .get(name)
            .is_some_and(|&byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b':'))
    {
        return None;
    }
    let name_end = skip(bytes, name + 1, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-')
    });
    let equals = gap(bytes, name_end);
    if bytes.get(equals) != Some(&b'=') {
        return Some(name_end);
    }
    let value = gap(bytes, equals + 1);
    match bytes.get(value) {
        Some(&quote @ (b'"' | b'\'')) => {
            let close = bytes[value + 1..].iter().position(|&byte| byte == quote)?;
            Some(value + close + 2)
        }
        Some(_) => {
            let end = skip(bytes, value, |byte| {
                byte > b' ' && !matches!(byte, b'"' | b'\'' | b'=' | b'<' | b'>' | b'`')
            });
            (end > value).then_some(end)
        }
        None => None,
    }
}

/// Past the spaces and tabs at `at`, with at most one line ending among
/// them.
fn gap(bytes: &[u8], at: usize) -> usize {
    let spaces = skip(bytes, at, is_space);
    if bytes.get(spaces) == Some(&b'\n') {
        skip(bytes, spaces + 1, is_space)
    } else {
        spaces
    }
}

/// The first index from `at` whose byte does not satisfy `keep`.
fn skip<F>(bytes: &[u8], at: usize, keep: F) -> usize
where
    F: Fn(u8) -> bool,
{
    at + bytes.get(at..).map_or(0, |rest| {
        rest.iter().take_while(|&&byte| keep(byte)).count()
    })
}

const fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
}
