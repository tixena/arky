# The grammar of the markdown arky reads

This file states what `arky_core::parser::parse` and `parse_with` accept, the tree
they build, and the text `arky_core::writer::write` writes back, as the code is
written today. The unit tests under [`src/parser/tests/`](src/parser/tests) show,
sample by sample, the tree each rule produces, and
[`src/writer/tests.rs`](src/writer/tests.rs) the text each written form gives.

## Two stages

A grammar is context-free when each rule can be applied without looking at anything
outside the piece of text it matches. The constructs below break that. Each one is
taken out of the grammar and handled by a stage that reads characters, so the grammar
never sees it.

| What is not context-free | What handles it | What the grammar sees instead |
|---|---|---|
| Which list item a line belongs to depends on how far it is indented compared with earlier lines. | The lexer keeps a stack of the open items. | `ITEM_START` and `ITEM_END` tokens. |
| A fenced code block ends at a fence of the same character, at least as long as the one that opened it. An indented code block, a block quote, an HTML block and a link reference definition each run over the lines that a rule about their first line admits. | The lexer reads each one whole. | One `CODE_BLOCK`, `BLOCK_QUOTE`, `HTML` or `DEFINITION` token. |
| A line of `=` or `-` underlines a heading only right after paragraph text. | The lexer, which knows whether the line before was text. | One `UNDERLINE_n` token, only after `TEXT`. |
| A table's cells line up in columns, and a table's first line is a header only when the right line follows it. | The table reader reads the whole table. | One `TABLE` token. |
| A code span ends at a run of exactly as many backticks as opened it. | The inline scanner finds the next run of that length. | One `Code` node. |
| Which runs of `*` and `_` pair into emphasis depends on the runs around them. | CommonMark's procedure for emphasis, run over the paragraph. | `Emphasis` and `Strong` nodes. |
| Which brackets make a link depends on the brackets around them, and a reference link on a definition written anywhere in the file. | One bracket pass per paragraph, with the definitions of the whole file read first. | `Link` and `Image` nodes. |
| Which YAML mapping a line belongs to depends on its indentation. | The YAML reader compares each line's indentation with that of the block it is reading. | `INDENT` and `DEDENT` in the rules below. |
| A YAML block scalar ends at the first line of text indented less than its own text. | The YAML reader reads the whole scalar. | One `BLOCK_SCALAR` token. |
| A TOML header or dotted key adds to a table that other lines wrote. | The TOML reader keeps the tables made so far. | Each `header` and `pair` on its own. |

With those removed, every rule below is context-free.

| Stage | File |
|---|---|
| Finding the frontmatter, and the value cap | [`src/parser/frontmatter.rs`](src/parser/frontmatter.rs) |
| TOML frontmatter | [`src/parser/toml.rs`](src/parser/toml.rs) |
| YAML frontmatter | [`src/parser/yaml.rs`](src/parser/yaml.rs) |
| Typing an unquoted value | [`src/parser/scalar.rs`](src/parser/scalar.rs) |
| Lexer, and the block cap | [`src/parser/lexer.rs`](src/parser/lexer.rs), [`src/parser/line.rs`](src/parser/line.rs) |
| Tables | [`src/parser/table.rs`](src/parser/table.rs) |
| HTML blocks and inline HTML | [`src/parser/html.rs`](src/parser/html.rs) |
| Link reference definitions, labels, destinations and titles | [`src/parser/definition.rs`](src/parser/definition.rs) |
| Block grammar | [`src/parser/block.rs`](src/parser/block.rs) |
| Inline grammar, and the inline cap | [`src/parser/inline.rs`](src/parser/inline.rs) |
| Escapes and character references | [`src/parser/entity.rs`](src/parser/entity.rs) |
| The tree | [`src/parser/ast.rs`](src/parser/ast.rs) |
| Extensions and the claim pass | [`src/parser/extension.rs`](src/parser/extension.rs), [`src/parser/claim.rs`](src/parser/claim.rs) |
| Writing | [`src/writer.rs`](src/writer.rs), [`src/writer/inline.rs`](src/writer/inline.rs), [`src/writer/table.rs`](src/writer/table.rs), [`src/writer/yaml.rs`](src/writer/yaml.rs), [`src/writer/toml.rs`](src/writer/toml.rs) |

## Notation

```text
name := ...      a rule
A B              A followed by B
A | B            A or B
A*  A+  A?       zero or more, one or more, optional
ε                nothing
UPPER            a token made by a lexer
"x"              the literal characters x
rule_n           one rule for each n in the range given beside it
```

## Document

```text
document    := frontmatter? markdown_block
frontmatter := TOML_OPEN toml TOML_CLOSE
             | YAML_OPEN yaml YAML_CLOSE
```

- A byte-order mark, U+FEFF, at the start of the file is skipped.
- `TOML_OPEN` is the first line of the file when that line is `+++`. `TOML_CLOSE` is
  the next line that is `+++`.
- `YAML_OPEN` is the first line of the file when that line is `---`. `YAML_CLOSE` is
  the next line that is `---` or `...`.
- Spaces after a delimiter are allowed. Spaces before it are not.
- When the first line is a delimiter and no later line closes it, the file has no
  frontmatter, and its first line is read as the rest of the markdown is: `---` alone
  is a thematic break.
- The markdown block starts on the line after the closing delimiter. Both `\n` and
  `\r\n` end a line.

A frontmatter entry that arky cannot read is refused: it is dropped, and arky reads
the rest of the file. The innermost entry that holds the error is dropped, at any depth;
where a refused entry ends is stated under [TOML frontmatter](#toml-frontmatter) and
[YAML frontmatter](#yaml-frontmatter). `Parsed.dropped`, beside the document a parse
gives, lists every dropped entry and key in file order, each a `ParseError` whose
`message` is the error and whose `span` covers the dropped text, up to its last line
that holds more than white space or a comment. The keys dropped by the value cap, by a
tag arky does not read, for being a list or a mapping, or by an alias that cannot be
copied are listed too. A write leaves the dropped text out.

A frontmatter that keeps no entry, and refuses or drops at least one, is invalid, and
arky reads the body after it. The parse gives `RawDocument::Invalid`, an
`InvalidDocument` whose body arky's markdown extension alone claims, and whose
`InvalidFrontmatter` holds the frontmatter's `format`, its `text`, which is its content
between the delimiters without the line ending of its last line, and the `error`
message of the first entry it refuses or drops, at any depth. `Parsed.dropped` lists
that error alone, pointing at the place. `{a: 1, b: [}`, `a: !point 1` then
`b: !point 2`, and `a = """never closed` are invalid. Any other file is
`RawDocument::Valid`: a `Document`, whose `frontmatter` is `None` when the file has
none. An empty frontmatter, and one that holds one list or one scalar that reads, keep
no entry, drop none, and hold an empty mapping. A strict parse,
`ParseOptions::strict()`, fails with the error of the first entry the frontmatter
drops, or of an invalid frontmatter, pointing at the place, as for an extension's
error (see [Extensions](#extensions)). The markdown never fails: what no rule
recognizes is text.

A `Document`'s `frontmatter` is a `Mapping`, each key and its `Value` in the order the
keys were first written. A `Value` is a `Boolean`, a `Date`, a `DateTime` with no
offset, a finite `Float`, an `Integer`, a `Mapping`, a `Null`, an `OffsetDateTime`,
a `Sequence` of values, a `String` or a `Time`, and holds no span.

## Markdown block

A markdown block, `MarkdownBlock` in the tree, is any stretch of markdown read by the
same rules: the document's body after its frontmatter, the content of every list item
and of every table cell, and the body of the document a code block whose language is
`markdown` or `md` holds.

### Tokens

The lexer reads a markdown block one line at a time. It keeps a stack with one number
per open list item: the column where that item's text starts. Columns count from the
start of the source line, or of a grid cell's line. In the white space that builds the
blocks, a line's indentation, the white space after a list marker, after a heading's
`#`s and after a block quote's `>`, and between the marks of a thematic break, a space
is one column and a tab moves to the next multiple of 4. Where a column limit falls
inside a tab, as when a line loses its columns up to an item's text, a code column or
a fence's column, the tab's columns past the limit are kept as that many spaces:
`- foo`, a blank line, then two tabs and `bar` is an item holding the code `  bar`. A
tab anywhere else is a character of text, kept as written: in a paragraph's text, and
in a code block's content past its code column or its fence's column.

| Token | What it covers |
|---|---|
| `BLANK` | A line holding only spaces and tabs. |
| `BLOCK_QUOTE` | A line whose text starts with `>`, and each line after it whose text starts with `>` within three columns of the enclosing item's text. It holds their text, each line without its `>` and one column of the space or tab after it, joined by `\n`. |
| `CODE_BLOCK` | A fenced code block, from its opening fence to its closing fence, or an indented code block. It holds the lines between the fences, their range in the source, and the info string, the text after the opening fence with the white space around it trimmed, as a range of the source. An indented block has an empty info string. |
| `DEFINITION` | A link reference definition: `[label]:`, a destination and an optional title, with nothing after them on their line, over as many lines as it needs. It holds the label as written and the target. |
| `HEADING_n` | One to six `#`, `n` of them, followed by a space, a tab or the end of the line. Its text runs from the first character after the spaces and tabs that follow them to the end of the line's text. A closing sequence, a final run of `#`s that a space or a tab precedes or that is the whole text, ends the text before the spaces and tabs in front of it: `# Title ##` holds `Title`, and `# #` no text. A final run that touches the text before it, a backslash included, is text: `# Title#` holds `Title#`, and `# Title \#` holds `Title #`. |
| `HTML` | An HTML block, by the seven kinds of CommonMark's HTML blocks, from its first line to the line that ends it by its kind. It holds the lines as written, from the column of the enclosing item's text, with the white space before the text of its first line as the spaces it spans: `1. j`, a blank line, then three spaces, a tab and `<div>` is an item whose HTML block holds one space, then `<div>`. |
| `ITEM_END` | The end of a list item. |
| `ITEM_START` | A list marker: `-`, `*` or `+`, or one to nine digits and `.` or `)`, followed by a space, a tab or the end of the line. It holds the marker, so the block grammar knows the bullet character or the delimiter: an item whose bullet character or delimiter differs from the item's before it starts a new list. |
| `TABLE` | A whole table (see [Tables](#tables)), and the depth of the containers that hold it. |
| `TEXT` | A line of paragraph text, from its first character to the end of its text. |
| `THEMATIC_BREAK` | Three or more of the same `-`, `*` or `_`, with nothing but spaces and tabs between them. |
| `UNDERLINE_n` | A line of only `=` (`n` is 1) or only `-` (`n` is 2), right after paragraph text. |

A line's text ends before the spaces and tabs at its end, and only those: a no-break
space or any other white space at the end of a line is text.

Each line is read as follows.

1. A `BLANK` line is `BLANK`. When the token before it opened an item whose marker
   line holds nothing else, that item ends first, with `ITEM_END`.
2. Otherwise the spaces and tabs that start the line are counted in columns: that
   count is the line's column. Every open item whose text column is greater than the
   line's column is closed, innermost first, with one `ITEM_END` each.
3. The line continues a paragraph when the token before it is `TEXT` and it closed no
   item.
4. A line whose column is four or more past the text column of the innermost open
   item, or past column zero, is `TEXT` when it continues a paragraph. Otherwise it
   starts an indented code block: one `CODE_BLOCK` holding that line and every later
   line indented as far, with the blank lines between them, each without its columns
   up to that code column.
5. Otherwise what is written from that column on is classified by the first of these
   that fits, in the order of precedence CommonMark gives them:
   1. `UNDERLINE_n`, when the line continues a paragraph.
   2. A fenced code block: three or more backticks whose info string holds no
      backtick, or three or more tildes. The lexer takes every line up to and
      including the first line made only of the fence's character, at least as many
      of it as opened the block, indented at most three columns past the enclosing
      item's text: a line of them indented further is content. With no closing fence,
      the block ends before the first line of text indented less than the enclosing
      item's text, or at the end of the text. Each line inside loses its spaces and
      tabs up to the column of the opening fence.
   3. `THEMATIC_BREAK`.
   4. `HEADING_n`. A list item may hold one.
   5. `BLOCK_QUOTE`.
   6. `HTML`, by the start conditions of CommonMark's seven kinds. Only the first six
      kinds may start on a line that continues a paragraph. The block ends at the
      line that holds its closer (`-->`, `?>`, `>`, `]]>`, or `</pre>`, `</script>`,
      `</style>` or `</textarea>`, in any case), or before a blank line for the last
      two kinds, or at the end of the enclosing item or of the text.
   7. `ITEM_START`. On a line that continues a paragraph, only a bullet, or the number
      1, with text after it starts an item. The item's text column is the column of
      its first word, when one to four columns of spaces and tabs follow the marker;
      with none, or with five or more, it is the column one past the marker. Whatever
      follows the marker on its line is classified again by these same rules. At the
      block cap (see [Blocks](#blocks)), a marker opens no item, and it and the text
      after it are `TEXT`.
   8. `TABLE`, also on a line that continues a paragraph, which then ends before the
      table. The lexer goes on after the table's last line. At the block cap no table
      is read, and its lines are read as they would be without it.
   9. `DEFINITION`, when the line does not continue a paragraph. A definition may go
      on over the lines after it as long as each would continue a paragraph; it takes
      them up to their line endings, so a title over a line end keeps the spaces and
      tabs before it.
   10. `TEXT`: anything else.
6. At the end of the text every open item is closed.

### Blocks

```text
markdown_block := space* lead? section_1*
lead           := node block* sub_1 | gap_1             the first section
section_n      := heading_n block* sub_n                n = 1..6
sub_n          := gap_n+1? section_n+1*                 n = 1..4
sub_5          := section_6*
sub_6          := ε
gap_n          := gap_n+1 section_n+1* | section_n+1+   n = 1..4, a skipped level
gap_5          := section_6+
heading_n      := HEADING_n | TEXT+ UNDERLINE_n         n = 1, 2
heading_n      := HEADING_n                             n = 3..6
block          := node | space
node           := paragraph | list | BLOCK_QUOTE | CODE_BLOCK | HTML | TABLE
                | THEMATIC_BREAK
space          := BLANK | DEFINITION
paragraph      := TEXT+
list           := list_c                                c is one of - * + . )
list_c         := item_c ( BLANK* item_c )*
item_c         := ITEM_START_c markdown_block ITEM_END  a marker with c
```

A section holds its heading, the blocks after it, then its subsections, one level
deeper. It ends at the next heading of its own level or a higher one. A block after a
subsection belongs to that subsection, because no rule returns to the parent. The tree
stores no level: a section's level is its depth in its markdown block, and each
markdown block, inside an item, a cell or a code block, counts its levels from one.

The grammar is context-free because its set of rules is finite. A markdown block has
six levels, so `section_1` to `section_6`, with their `sub_n` and `gap_n`, are a fixed
number of rules, and the five `list_c` rules give one list per bullet character or
delimiter. A heading line of level `n` can only start a `section_n`, or a `gap_m` for
each level `m` above it that it skips, so no rule looks at the heading before it. A
markdown block nests inside an item between `ITEM_START` and `ITEM_END`, which pair as
parentheses do; a table cell and a `markdown` code block hold markdown blocks read
again from their own text by the same rules.

A section with no heading line, whose `heading` is `None` in the tree, is `ε` in the
grammar. There are two: `lead`, the section that holds what comes before a markdown
block's first heading, and exists only when something does; and `gap_n`, a level that
the next heading skips, which holds no blocks and holds the sections of that heading.
A heading line with no text, such as `#` alone, is not `ε`: it is a heading with no
children, and its section is a `section_n`. A seven-`#` line is not a heading; it is
`TEXT`.

`# Hi H1` then `### This is H3` derives as `section_1 := HEADING_1 sub_1`,
`sub_1 := gap_2`, `gap_2 := section_3`, `section_3 := HEADING_3 sub_3`: the middle
section's heading is `None`, and the writer writes back the same two heading lines,
with a blank line between them. `intro` then `## B` derives as `lead := node sub_1`
and `sub_1 := section_2`: `B` is a subsection of the first section, which has no
heading line.

The block cap, 40, counts list items, markdown blocks and table cells together, so
that reading, writing, cloning, comparing, printing and dropping the tree stay within
a 2 MB stack in a debug build. Each of them puts what it holds one level deeper: an
item its text, a fenced block whose language is `markdown` or `md` its content, and a
table its cells, those of a pipe or simple table as well as a grid table's. Text 40
levels deep opens none of them and stays as it is written: a marker is text, a table
is paragraph text, and a `markdown` block is a plain code block whose value is its
content. No node that deep is offered to an extension, and it stays the node arky
built.

Two rules settle what the grammar leaves open:

- A paragraph takes every `TEXT` that follows it; when `UNDERLINE_n` follows them,
  those lines are the text of `heading_n` instead, joined with a space where each line
  ended.
- Blank lines followed by an item of the same list continue the list.

`BLANK` and `DEFINITION` make no node: blank lines alone make no section, and a
definition adds its label and its target to the definitions of the file. The first
definition of a label wins, also one written inside a table cell.

A list is loose when a blank line separates two of its items, or two blocks of one of
its items. Its `start` is the number of its first item, or `None` for a bullet list.
An item whose marker line holds nothing else ends at a blank line right after it;
otherwise the line after the marker belongs to it when it is indented to its text.

A fenced code block's info string is its `language` and its `args`. The language is
the first word, up to the first white-space character, as written, and `None` when the
info string is empty. The args are the rest, with the white space before it trimmed.
They are `Args::Toml` when `args = {`, the rest, then `}` reads as a TOML document
holding the key `args` alone, written once, whose inline table ends at that last `}`,
with no key written twice at any depth inside it. Dotted keys that only share a
prefix, such as `a.b=1,a.c=2`, are one table and not a key written twice. Otherwise,
and also when the args nest deeper than the value cap of the frontmatter, the args are
`Args::Text`, the rest as written. A fence with nothing after its language has empty
`Toml` args.

| Opening fence | `language` | `args` |
|---|---|---|
| ```` ```md title="README.md",showLineNumbers=true ```` | `md` | `Toml`: `title` is the text `README.md`, `showLineNumbers` is `true` |
| ```` ```md title="README.md" showLineNumbers ```` | `md` | `Text`: `title="README.md" showLineNumbers` |
| ```` ```md a=1,a=2 ```` | `md` | `Text`: `a=1,a=2` |
| ```` ```md a=1} # x ```` | `md` | `Text`: `a=1} # x` |
| ```` ```markdown Example of a list ```` | `markdown` | `Text`: `Example of a list` |
| ```` ``` ```` | `None` | `Toml`, empty |

A code block whose language is `markdown` or `md`, in any case, is one `CODE_BLOCK`
token. arky's own markdown extension reads its content again as a markdown block in
the claim pass, so that content follows this same grammar (see
[Extensions](#extensions)).

### Tables

A table is read whole, and only when its lines have the shape below; otherwise they
are read as they would be without it, most often as paragraph text. Three formats are
read, tried in this order, and every line of a table after its first is indented at
least to the enclosing item's text.

| Format | Written as | Read when |
|---|---|---|
| Pipe | A header row, then a delimiter row such as `\|:--\|:-:\|--:\|`, then body rows, each with cells between `\|`. The pipes at either end of a row are optional. | The header line holds a `\|`, and every cell of the delimiter row is one or more dashes with an optional colon at either end. The body rows run to the first line with no `\|`, or to a blank line. |
| Grid | `+---+---+` borders around rows of `\| a \| b \|` lines; a `+===+===+` border under the header rows. | Every border has its `+` at the same columns as the top border, every row line starts with `\|` at the top border's column, a border follows the lines of each row, and at most one border is `+===+`. A row runs over every line between two borders. |
| Simple | A header line, then a line of dash groups separated by spaces or tabs, then rows up to a blank line. Its columns count with tab stops of 4: a tab moves to the next multiple of 4, as in a line's indentation. | The dash line has at least two groups of dashes and nothing but spaces and tabs before and between them, and at least one row follows it. |

- A cell holds a markdown block, read from its text by the same rules as the document,
  one container deeper. A pipe or simple cell is its text on its line, without the
  spaces and tabs around it; `\|` in a pipe cell is a pipe, through the escape rule. A
  grid cell is its text on each line of its row, read one after another, with the
  indentation the lines share left out and the spaces and tabs at their ends dropped.
  So `| # Foo |` is a cell holding a section `Foo`.
- A table is as wide as the widest of its header, its delimiter row and its rows. A
  shorter row, the header too, gets empty cells at its end, and a column the
  delimiter row does not reach has the default alignment.
- A grid column runs from one `+` column of the top border to the next. A `\|` at a
  `+` column is a wall, and a `\|` that ends the line closes the last column, so a
  wall out of line stays text in a cell.
- A simple column runs from its dash group to the next group, the first one from the
  start of the line, and a word that crosses into the next column stays in the column
  it starts in.
- The alignment of a pipe column comes from the colons of its delimiter cell, and that
  of a grid column from the colons of the header border, or of the top border when
  there is no header: `:--` left, `--:` right, `:-:` center, `--` default.
- The alignment of a simple column comes from where its header sits on its dashes:
  flush with the left end only, left; with the right end only, right; with neither,
  center; with both, or with no header text, default.
- A pipe or simple table has one header row; a grid table has the rows above its
  `+===+` border, or none.
- A link definition in a cell is a definition of the file, and its cell holds no text
  for it.

### Inline nodes

Inline nodes are read in the text of a paragraph and of a heading.

```text
inlines     := inline*
inline      := code | autolink | html | link | image | emphasis | strong
             | escape | reference | line_break | text
code        := TICKS code_text TICKS             both runs of backticks the same length
autolink    := "<" ( URI | EMAIL ) ">"
html        := OPEN_TAG | CLOSING_TAG | COMMENT | PROCESSING | DECLARATION | CDATA
link        := wiki | "[" inlines "]" target
image       := "!" wiki | "!" "[" ALT "]" target
wiki        := "[[" wiki_target ( "|" alias )? "]]"
target      := "(" destination? title? ")" | "[" label "]" | "[]" | ε
emphasis    := RUN inlines RUN                   paired by CommonMark's rules
strong      := RUN inlines RUN                   paired by CommonMark's rules
escape      := "\" PUNCTUATION
reference   := "&" NAME ";" | "&#" DIGITS ";" | ( "&#x" | "&#X" ) HEX ";"
line_break  := ( "\" | SPACES ) LINE_END
text        := any other characters
```

A paragraph's lines are read as one text: its lines joined by their line endings,
each line that another follows keeping the spaces and tabs before its line ending.
Every rule above may match across a line end. The nodes are then cut at each line end
into the paragraph's `Line`s, and each line holds the nodes written on it. A node that
crosses a line end is cut into one piece per line, each with its `continues`: `Next`
for the first piece, `Both` for a middle one, `Previous` for the last, and `Neither`
for a node whole on its line. Text never crosses a line end, since each line has its
own. A heading reads its text the same way and holds whole nodes: the pieces of each
node are joined into one, the line end between two lines is a space unless a line
break ends the first, a code span's line endings become spaces before its strip, the
parts of an image's alt are joined with a space, and the parts of inline HTML with
`\n`.

- **Code spans.** A run of backticks opens a code span that ends at the next run of
  exactly as many; a run with none stays text. Its value is what lies between the
  runs, as written. When both ends are spaces and it is not all spaces, one space
  goes from each end, a line ending counting as a space; a line ending at an end
  stays, as the boundary between the pieces. The spaces before a line end inside a
  code span are code. Nothing else is read inside a code span, an autolink or inline
  HTML, so a bracket or a `*` there is in no other node.
- **Autolinks.** `<scheme:rest>`, with a scheme of 2 to 32 letters, digits, `+`, `.`
  or `-` that starts with a letter and a rest with no space, control character or
  `<`, is a `Link` of form `Markdown` to what it shows, which is its only child. So is
  `<address>` for an email address, to `mailto:address`.
- **Inline HTML.** An open tag, with its attributes, a closing tag, a comment
  (`<!-->`, `<!--->`, or up to `-->`), a processing instruction (`<?` to `?>`), a
  declaration (`<!` and a letter, to `>`) or a CDATA section (`<![CDATA[` to `]]>`) is
  an `Html` node holding what is written. The spaces and tabs inside a tag may hold one
  line ending each.
- **Links and images.** `[text](destination "title")` is an inline link;
  `[text][label]`, `[label][]` and `[label]` are reference links, which take the
  target and the title of the definition of their label. The inline form is tried
  first. A label with no definition stays text, and so does `[text]` before a
  `[label]` with no definition. `!` before a link form makes an image. A link and an
  image record which form they were written in, `Markdown` or `Wiki`.
- **Labels, destinations and titles.** A label holds no unescaped `[`, at most 999
  characters, not all of them white space. Labels match by Unicode case folding, each
  run of spaces, tabs and line endings counting as one space and none at either end.
  A destination is written between `<` and `>` on one line, or as a nonempty run with
  no space or control character whose parentheses pair up, nested at most 32 deep; one
  nested deeper is no destination, and its brackets stay text. A title is written
  between `"`, `'`, or `(` and `)`. The spaces and tabs around a destination and a
  title may hold one line ending, and a title keeps the spaces before a line ending
  inside it. `target` and `title` hold their escapes and character references
  resolved.
- **Balanced brackets.** Brackets pair as CommonMark's stack of brackets pairs them, in
  one pass over each paragraph that keeps every code span, autolink, inline HTML and
  link target it reads for the scanner. Link text may hold balanced brackets:
  `[a [b] c](d)` is a link holding `a [b] c`. Once a link, a wiki link or an autolink
  is read, no `[` before it opens a link, so a link never holds a link, at any depth,
  also through an image: `[a ![[b](c)](d) e](f)` is the text `[a `, an image of `d`,
  and the text ` e](f)`, and `[<https://a.example>](https://b.example)` is the text
  `[`, an autolink, and the text `](https://b.example)`. In the tree, a link's children
  are `LinkInline` nodes: every inline node but `Link`, with `LinkEmphasis` and
  `LinkStrong` for an emphasis and a strong, whose children are `LinkInline` nodes too.
  A bracket after a backslash, or inside a code span, an autolink, inline HTML, a wiki
  link, or a link's destination or label, is in no pair.
- **Wiki links.** `[[target]]` and `[[target|alias]]` are read before a bracketed link
  at the same `[`, on one line, with something between the brackets and no bracket
  inside them. The target is the text before the first `|`, and the alias the rest; an
  empty alias is none. A wiki link's alias is its text, one `Text` child, and a wiki
  embed's alias is its `alt`.
- **An image's alt.** The text between an image's brackets, as written, without the
  spaces and tabs before each line ending in it. No node is read in it.
- **Emphasis and strong.** Runs of `*` and of `_` pair by CommonMark's procedure for
  emphasis. A run can open when it is left-flanking and close when it is
  right-flanking, by the Unicode white space and punctuation around it, symbols
  counting as punctuation and a line end as white space; a `_` run inside a word does
  neither. A closer pairs with the nearest opener of its character that the rule of
  three allows. Two characters on each side make `Strong`, one makes `Emphasis`, so
  `***a***` is emphasis around strong. Pairing keeps one list of the runs not yet
  paired and, for each kind of closer, the first run that may still open for it, so a
  paragraph is read in time linear in its length.
- **Escapes.** A backslash before an ASCII punctuation character makes that character
  text. Any other backslash is text.
- **Entity and numeric references.** `&name;` with a name of the HTML5 list, `&#` and
  one to seven digits then `;`, and `&#x` or `&#X` and one to six hexadecimal digits
  then `;` are the characters they name; a code point that is zero or names no
  character gives U+FFFD. They are resolved in text, in link destinations and in
  titles, and kept as written in code spans, autolinks and inline HTML.
- **Line breaks.** A line of a paragraph that another line follows ends with a
  `LineBreak` when it ends with a backslash, or with spaces and tabs that end with two
  spaces. Otherwise the spaces and tabs before a line ending are dropped, as are the
  spaces that start a line. The last line of a paragraph has no line break.
- **Text.** Neighbouring text, escapes and references on one line become one `Text`
  node. Brackets, backticks and runs of `*` or `_` that complete no rule are text.
- **The inline cap.** Inline nodes nest at most 1000 deep. A pair of runs or a link
  that would nest deeper is not made: its marks stay text. The parser reads, cuts and
  joins nested nodes from stacks of its own, so how deep they nest costs it no call
  depth; the cap comes from the tree's derived traits, cloning first, which must stay
  within a 2 MB stack in a debug build.

## TOML frontmatter

```text
toml          := ( statement? COMMENT? NEWLINE )*
statement     := header | pair
header        := "[" key_path "]" | "[[" key_path "]]"
pair          := key_path "=" value
key_path      := key ( "." key )*
key           := BARE_KEY | basic_string | literal_string
value         := basic_string | literal_string | multi_basic | multi_literal
               | BARE_VALUE | array | inline_table
array         := "[" ( value ( "," value )* ","? )? "]"
inline_table  := "{" ( pair ( "," pair )* )? "}"
```

- `BARE_KEY` is ASCII letters, digits, `_` and `-`. Spaces and tabs may sit around
  `=`, around each `.` of a key path, and inside a header's brackets. A `.` inside a
  quoted key is part of the key.
- `COMMENT` is `#` to the end of the line.
- Inside an array, line breaks and comments may sit between the values, and a `,` may
  follow the last one. An inline table stays on one line, and no `,` follows its last
  pair.
- `basic_string` is `"..."` and `literal_string` is `'...'`, on one line.
  `multi_basic` is `"""..."""` and `multi_literal` is `'''...'''`, over any lines: a
  line ending right after the opening quotes is not part of the text, up to two quotes
  right before the closing three are, and in `multi_basic` a `\` at the end of a line
  removes the line ending and the white space after it.
- `BARE_VALUE` runs to the first `,`, `]`, `}`, `#`, space, tab or line ending. A date
  followed by one space and a time is one value.
- A header opens the table it names, and the pairs after it, up to the next header,
  go into it. `[[name]]` adds an empty mapping at the end of the list `name` and opens
  it. A dotted key `a.b = 1` writes `b = 1` into the table `a`.
- At every part of its path but the last, a dotted key or a header goes on into the
  table that a dotted key or a header made there, also into the last mapping of a list
  of them: `a.b = 1` then `a.c = 2` is one table `a` holding both, and `[[authors]]`
  then `[authors.site]` opens `site` in the last author.
- A header names a table again only when nothing but a deeper header made it:
  `[server.tls]` then `[server]` adds to `server`.
- A key or a table written more than once in one mapping becomes one key, in the place
  where it was first written. Its value is a list of every value written for it, in
  the order of the file: a list adds its items, and any other value adds itself as one
  item. `title = "Draft"` then `title = "Final"` is `title = ["Draft", "Final"]`. A key
  is written again by a second `key = value`, by a header that names a table a header
  or a dotted key made, such as `[params]` twice or `site.name = "x"` then `[site]`,
  and by a dotted key or a header through a key whose value was written with `=`,
  because TOML closes an inline table at its brace and an array at its bracket:
  `a = {b = 1}` then `a.c = 2` is `a = [{b = 1}, {c = 2}]`. A later dotted key or
  header through such a key goes on into the last mapping of its list.
- A pair that does not read is refused with its whole value: from its `=` to the line
  where every `[` and `{` after it is closed, then up to the next line that starts a
  key followed by `=`, or a `[`, after spaces or tabs. A line inside the value is not a
  statement. When a bracket never closes, the pair ends before the first later line
  that starts a key followed by `=`; a line that starts with `[` does not end it. A
  header that does not read is refused with its table and the pairs under it, up to
  the next header, even after an open bracket: each pair under it ends as a refused
  pair's value does. Strings and comments are passed over whole, so a `"""` or `'''`
  string left open runs to the end of the frontmatter, dropping the rest, and a
  frontmatter that keeps no statement before it is invalid. `title = Notes` then
  `date = 2026-10-17` is `{date: 2026-10-17}`, and so is `tags = ["a", "b"` then
  `date = 2026-10-17`. `a = [`, `[1],`, `@` and `]` on four lines, then `b = 2`, is
  `{b: 2}`, with one drop. `[a]`, `x = 1`, `[owner` then `y = 2` is `{a: {x: 1}}`, and
  `[t`, `x = [`, `y = 1`, `[u]` then `z = 3` is `{u: {z: 3}}`. A pair is read to the
  end of its line before it is kept: `a = 1 2` is refused with `expected the end of
  the line after the value`.
- Values nest at most 400 deep, the value cap: arrays, inline tables, and the parts of
  dotted keys and of headers count together, and in YAML so do flow and block
  collections, properties on collections and explicit keys. The innermost pair that holds a
  value past that depth is dropped, and the mapping keeps its other pairs, so that
  every operation on the tree stays within a 2 MB stack in a debug build: a value or a
  dotted key past the cap drops its pair, and a header past it drops its table with the
  pairs under it, up to the next header. `deep =` and arrays nested 401 deep, then
  `b = 2`, is `{b = 2}`, and inline tables `{a = {a = ...}}` nested 401 deep lose only
  their innermost pair. What follows the value that passes the cap, up to the end of
  the pair, is not read: a pair past the cap ends as a refused pair's value does, so
  `deep =` and 401 `[` never closed, then `b = 2`, `[u]` and `z = 3`, is
  `{b: 2, u: {z: 3}}`. The same cap holds for a code block's args, which past it stay
  `Text`.

A value's type comes from how it is written:

| Written as | Type |
|---|---|
| `"..."`, `"""..."""`, with the escapes `\b \t \n \f \r \" \\ \uXXXX \UXXXXXXXX` | String |
| `'...'`, `'''...'''`, with no escapes | String |
| `true`, `false` | Boolean |
| `2026-10-10`, when that day exists in the calendar; `2026-02-30` is not a value | Date |
| `07:32:00`, optionally with a fraction such as `.250` | Time |
| A date, then `T`, `t` or one space, then a time, then `Z`, `z` or an offset such as `-04:00` | OffsetDateTime |
| A date, then `T`, `t` or one space, then a time, with no offset | DateTime |
| An optional sign, then `0` or a digit from 1 to 9 followed by any digits; or, with no sign, `0x`, `0o` or `0b` and digits in that base. An underscore may stand between two digits, but not right after `0x`, `0o` or `0b`: `0`, `1_000` and `0xff_ff` are integers, and `01`, `1__0`, `_1`, `1_` and `0x_ff` are not values. An integer that does not fit 64 bits is the String of its text as written, underscores and prefix kept, such as `"9_999_999_999_999_999_999"`, and is written back as quoted text | Integer, 64 bits; String when it does not fit |
| A whole part written as a decimal integer, with its optional sign, then a fraction of digits, an exponent or both. The exponent is `e` or `E`, an optional sign and digits, which may start with zeros. An underscore may stand between two digits of the whole part, of the fraction or of the exponent: `0.5`, `1e02` and `1e0_1` are floats, and `01.5` is not a value. A float too large for a float to hold is the String of its text as written, such as `"1e400"`, and is written back as quoted text | Float, finite; String when it does not fit |
| `inf` and `nan`, with an optional sign: arky holds no float that is not a finite number. Each is the String of its text as written, such as `"-inf"`, and is written back as quoted text | String |

## YAML frontmatter

```text
yaml          := mapping? | flow_mapping
mapping       := entry+
entry         := key? ":" value
               | "?" compact ( ":" compact )?
value         := node | NEWLINE block?
compact       := node | mapping | sequence | NEWLINE block?
sequence      := ( "-" ( node | mapping | sequence | NEWLINE indented? ) )+
node          := PROPERTIES? inline NEWLINE
               | PROPERTIES? BLOCK_SCALAR
               | PROPERTIES NEWLINE block?
               | ALIAS NEWLINE
block         := indented | sequence
indented      := INDENT mapping DEDENT
               | INDENT sequence DEDENT
               | INDENT node DEDENT
inline        := flow_sequence | flow_mapping | double_quoted | single_quoted | PLAIN
flow_sequence := "[" ( flow_entry ( "," flow_entry )* ","? )? "]"
flow_entry    := flow_key ":" flow_node? | explicit_pair | empty_pair | flow_node
flow_mapping  := "{" ( flow_pair ( "," flow_pair )* ","? )? "}"
flow_pair     := flow_key ( ":" flow_node? )? | explicit_pair | empty_pair
explicit_pair := "?" flow_key? ( ":" flow_node? )?
empty_pair    := ":" flow_node?
flow_node     := PROPERTIES? inline | ALIAS
key           := PROPERTIES? ( double_quoted | single_quoted | PLAIN_KEY
                             | flow_sequence | flow_mapping )
               | PROPERTIES | ALIAS
flow_key      := PROPERTIES? ( double_quoted | single_quoted | FLOW_KEY
                             | flow_sequence | flow_mapping )
               | PROPERTIES | ALIAS
PROPERTIES    := TAG ANCHOR? | ANCHOR TAG?
```

- The frontmatter is a block mapping, or one flow mapping with nothing but comments
  after it. A frontmatter that holds one list or one scalar, with nothing but comments
  after it, holds no key, and is an empty mapping: `abc`, `[a, b]`, and `- a` then
  `- b` are all `{}`. One whose list or scalar does not read, an entry inside it
  included, is invalid with `the frontmatter must be a mapping`, as `[unclosed`,
  `!!int abc` and `- !!int abc` are, and so is one whose first line starts a flow list
  with more after it, as `[a, b]` then `c: 1` is. A first line that holds a scalar or
  starts a block list, with a key after it, is an entry that does not read:
  `hello` then `c: 1` is `{c: 1}`.
- An entry that does not read is refused. A pair of a block mapping runs from its
  key's line up to the next line at that key's column or less, a list entry up to the
  next line at its `-`'s column or less, and a pair of a flow mapping up to the next
  line of the mapping that starts no further right than the pair; with no such line,
  the flow mapping itself does not read. `title: Notes` then `tags: [a, b` then
  `date: 2026-10-17` is `{title: Notes, date: 2026-10-17}`, `owner:` then
  `  name: Ada` then `  @handle: ada` then `  role: dev` is
  `{owner: {name: Ada, role: dev}}`, `items:` then `- [a` then `- b` is
  `{items: [b]}`, and `{`, `  title: Notes,`, `  tags: [a, b,`, `  date: 2026-10-17`,
  `}` is `{title: Notes, date: 2026-10-17}`. A broken pair of a flow mapping that
  closes on the pair's line fails the mapping: `m: {a: @x}` then `  c: 1` then
  `d: 2` is `{d: 2}`, refused with ``a plain value cannot start with `@`; quote it``.
  Text where a `,` or a `}` is expected after a pair of a flow mapping is refused
  with ``expected `,` or `}` in the mapping``: on the line the pair starts on, it is
  dropped with the pair, and on a later line it starts an entry of its own, which
  runs as a pair does. `{`, `  a: "x" junk,`,
  `  b: 1`, `}` is `{b: 1}`, and `{`, `  title: Notes`, `  date: 2026-10-17`, `}` is
  `{title: Notes}`, dropping `date: 2026-10-17`. The lines a pair's value does not take
  belong to the pair: `a: 1` then `  b: 2` then `c: 3` is `{c: 3}`, refused with
  `this line is indented more than the keys above it`. A line indented less than the
  frontmatter's first key, and text after a flow mapping that is the whole
  frontmatter, drop the rest of the frontmatter: `  a: 1` then `b: 2` is `{a: 1}`.
- `INDENT` is a line indented more than the key above it; `DEDENT` is the return to
  an earlier indentation. Every key of one mapping starts at the same column.
- A `block` may be a sequence written at the same column as its key (the second form
  of `block`).
- After a key or a bare `-`, the next line indented more than it may hold any
  value: a mapping, a list, a scalar, a quoted string, a flow collection, a block
  scalar or a tagged value. `title:` then `  Notes` is `"Notes"`.
- A key with nothing after its colon and no block under it is null, and so is a `-`
  with nothing after it and no block indented under it, and a pair of a flow
  mapping with no `:` or nothing after it.
- In a list entry, a tag that ends its line takes only the block indented more than
  the `-`. With nothing there, its empty value is checked against the tag:
  `- !!seq` then `- x` is refused with
  ``an empty value is not a value of the tag `!!seq` ``.
- A list entry whose line goes on with a plain, quoted or tagged `key:`, with an
  explicit key, or with a `:` and the empty key, tagged or not, holds a mapping. Its
  first key is on the `-` line, and its other keys start at that key's column on the
  lines below. `- !!str k: v` then `  m: n` is `[{k: v, m: n}]`, and `- : v` is
  `[{"": v}]`.
- A list entry whose line goes on with `-` and a space or a tab, or with a `-` that
  ends the line, holds a list. Its first entry is on the `-` line, and its other
  entries start at the second `-`'s column on the lines below. `- - a` then `  - b`
  then `- c` is `[[a, b], c]`.
- A list entry starts at a `-` followed by a space, a tab or the end of the line. A
  tab after the `-` separates it from the value as a space does: `-`, a tab and `x`
  is `[x]`, and `- -`, a tab and `a` is `[[a]]`. A tab is never indentation: after
  a tab, the rest of a `-`, `?` or `:` line is one value, never a list or a mapping
  that starts at a column. `-`, a tab and `- a` is refused with
  ``a plain value cannot start with `-` and a space; quote it``.
- A line that starts a key, an explicit key, an empty key or a list entry is refused
  with `a tab cannot indent a line; use spaces`, pointing at the tab, when a tab
  comes before its first character. Every line at the column of a mapping's keys or
  of a list's `-` counts as one, and so does the frontmatter's first line. `a:` then
  a tab and `b: 1` is refused, and so are a tab and `a: 1`, a tab and `- a`, `a:` then
  a tab and `- b`, `a:` then two spaces, a tab and `- x`, and `a:` then two spaces, a
  tab and `b: 1`.
- The first line of a value under a key or a bare `-`, and the frontmatter's first
  line, may have tabs and spaces after their indentation when no key, explicit key,
  empty key or list entry follows them. They separate the value from the
  indentation, and the value is read from its first character: a scalar, a quoted
  string, a flow collection, a block scalar or a tagged value. `a:` then two spaces,
  a tab and `"x"` is `{a: "x"}`, `a:` then two spaces, a tab and `[x]` is
  `{a: [x]}`, `a:` then two spaces, a tab and `!!str x` is `{a: "x"}`, `items:` then
  `-` then two spaces, a tab and `"x"` is `{items: ["x"]}`, and a tab and `{a: 1}` is
  `{a: 1}`.
- Other lines may have a tab after their indentation: a line that goes on with a
  plain or quoted value, a line of a block scalar's text, a line of a flow
  collection, and a line holding only a comment, before its `#`. `a: b` then two
  spaces, a tab and `c` is `{a: "b c"}`, `t: [a,` then a space, a tab and `b]` is
  `{t: [a, b]}`, and `a: |` then `  x` then two spaces, a tab and `y` is
  `{a: "x\n\ty\n"}`.
- An explicit key is written after `?` and a space or a tab, or after a `?` that
  ends its line, at the column of the mapping's keys. An optional next line at the
  `?`'s column that starts with `:` and a space or a tab, or with a `:` that ends
  the line, gives the pair's value. With no such line, the value is null. `? k`
  then `: v` is `{k: v}`, and `? k` alone is `{k: null}`.
- The `?` line and the `:` line go on as a list entry's `-` line does: a `-` and a
  space start a list at that column, and a `key:` or a `?` there starts a mapping
  at that column. With nothing after it on its line, the `?` or the `:` takes the
  block under it, as a key's `:` does. `? k` then `: a: 1` is `{k: {a: 1}}`, and
  `? k` then `: - x` is `{k: [x]}`.
- The key after `?` is a scalar, plain, quoted or tagged, or a list or a mapping,
  on the `?` line or on the lines indented under it. A pair whose key is a list or
  a mapping is dropped, as stated below: `? - a` then `: v`, and `? a: 1` then
  `: v`, are both `{}`.
- A `:` line indented more than its `?` does not give the pair's value: `? k` then
  `  : v` is refused with `this line is indented more than the keys above it`.
- `PLAIN_KEY` runs to the first `:` followed by a space, a tab or the end of the
  line, before the line's comment. `a: 1` then `abc # c: d` refuses `abc # c: d` with
  ``expected `key: value` ``, and `a#b: 1` is `{"a#b": 1}`. `FLOW_KEY` runs to the
  first `:` followed by a space, a tab, the end of the line or one of `, [ ] { }`, to
  the first of `, [ ] { }`, or to a comment.
- Inside a flow mapping, and after a `?` inside a flow list, a `FLOW_KEY` that runs
  to the end of its line goes on over the following lines up to its `:`, or up to a
  `,`, `[`, `]`, `{` or `}`, as a `PLAIN` inside a flow collection does: a comment
  ends it, each of its lines must be indented more than the flow collection's key,
  and its lines are folded as a `PLAIN`'s are. `{x` then `  y: 1}` is
  `{"x y": 1}`, `a: {x` then `  y: 1}` is `{a: {"x y": 1}}`, `{x` then `  y` then
  `  z: 1}` is `{"x y z": 1}`, and `a: [? x` then `  y : 1]` is
  `{a: [{"x y": 1}]}`. Inside a flow list, a key with no `?` before it stays on its
  line: `[x` then `  y: 1]`, and `a: [x` then `  y: 1]`, are refused with
  ``expected `,` or `]` in the list``.
- Spaces and tabs may sit between a key and its `:`: `"a" : 1` is `{a: 1}`.
- Outside a flow collection, a space, a tab or the end of the line must follow the
  `:` of a key. `"a":1` is refused with ``expected a space after the `:` ``,
  and `{"a":1}` is `{a: 1}`.
- A comment starts at a `#` that follows a space or a tab.
- `PLAIN` runs to the end of the line, or to a comment; inside a flow collection it
  stops at `,`, `[`, `]`, `{` or `}`. It cannot hold a `:` followed by a space or a
  tab, or end with `:`; such a value is refused with
  ``a plain value cannot hold `: `; quote it``.
- A `PLAIN` cannot start with `@`, `` ` ``, `%`, `,`, `]` or `}`, nor with `?`, `:`
  or `-` followed by a space, a tab or the end of the line. Such a value is refused
  with ``a plain value cannot start with `@`; quote it``, naming the
  character, or with ``a plain value cannot start with `-` and a space; quote it``
  when a space or a tab follows. `?x`, `:x` and `-x` are text. Inside a flow
  collection, `,`, `]` and `}` end the entry instead.
- Inside a flow collection, a `PLAIN` cannot start with `|` or `>` either, nor with
  `?`, `:` or `-` followed by `,`, `[`, `]`, `{` or `}`. `[|x]` is refused with
  ``a plain value cannot start with `|`; quote it``, and `[-]` with
  ``a plain value cannot start with `-`; quote it``. `[-x, ?y, :z]` holds three
  strings.
- A plain key is checked as a `PLAIN` is. A `PLAIN_KEY` cannot start with `@`,
  `` ` ``, `%`, `|`, `>`, `,`, `]` or `}`, nor with `?`, `:` or `-` followed by a
  space, a tab or the end of the line. A `FLOW_KEY` cannot start with `@`, `` ` ``,
  `%`, `|` or `>`, nor with `?`, `:` or `-` followed by a space, a tab, the end of
  the line or one of `, [ ] { }`. Such a key is refused with
  ``a plain key cannot start with `@`; quote it``, naming the character as for a
  `PLAIN`: `@a: 1`, `` `a: 1 ``, `|x: 1`, `>x: 1` and `a: {@y: 1}` are all refused.
- A key may be a list or a mapping, in block or flow form, tagged or not. A flow
  collection followed by `:` and a space, a tab or the end of the line is such a
  key on a line at the column of a mapping's keys, on the first line of a value
  under a key or a bare `-`, on the rest of a `-`, `?` or `:` line, and on the
  frontmatter's first line. Inside a flow mapping, a flow collection where a key
  starts is a key; inside a flow list, a flow collection followed by `:` on its line
  is the key of a pair. After `?`, a block list or a block mapping is a key too.
- A key that is a list or a mapping is read to the end of its value, and the pair
  is dropped: TOML cannot hold such a key. The mapping keeps its other pairs, and a
  mapping that held no other pair is an empty mapping. `title: Trip` then
  `[48.85, 2.35]: Paris` then `year: 2026` is `{title: Trip, year: 2026}`,
  `{x: 2}: 3` is `{}`, `c:` then two spaces and `[x]: y` is `{c: {}}`, `items:`
  then `- {x: 1}: 12` is `{items: [{}]}`, `a: {[x]: 1, b: 2}` is `{a: {b: 2}}`,
  `a: [[x]: 1, c]` is `{a: [{}, c]}`, and `a: [? [x] : 1]` is `{a: [{}]}`.
- A dropped pair's key and value are read as any other's, and an error in them
  refuses the pair with that error: `[x]: [unclosed` is refused with
  ``expected `,` or `]` in the list``, as `a: [unclosed` is, and `!!str [x]: y` with
  ``a list is not a value of the tag `!!str` ``. A pair whose key and value read is
  dropped with `a key cannot be a list or a mapping`.
- A frontmatter whose first line starts with `{` is one flow mapping, and one whose
  first line starts with `[` is one list, unless the flow collection is a key as
  above.
- Outside a flow collection, a `PLAIN` whose line has no comment goes on over each
  following line that is indented more than its key or `-`, is not a comment and
  holds no `key:`; blank lines may sit between them, and a comment ends the last
  one. Inside a flow collection, a `PLAIN` goes on over the following lines in the
  same way, up to the next `,`, `[`, `]`, `{` or `}`. The lines are folded by
  YAML 1.2.2: each loses the spaces around it, a line break between two lines of
  text is a space, and each blank line between them is a `\n`. `title: a long`
  then `  title` is `"a long title"`.
- A flow collection may run over several lines: line breaks and comments may sit
  between its entries, and a `,` may follow the last one. A `,` where an entry
  should start is refused with ``expected a value before `,` ``.
- Inside a flow mapping, and after an explicit key inside a flow list, line breaks
  and comments may also sit between a key and its `:`. `a: {x` then `  : 1}` is
  `{a: {x: 1}}`, and `a: [? x` then `  : 1]` is `{a: [{x: 1}]}`. Inside a flow list,
  a key with no `?` before it has its `:` on the key's line: `a: [x` then `  : 1]`
  is refused with ``expected `,` or `]` in the list``.
- Each line of a flow collection must be indented more than its key or `-`, the
  line of its closing bracket too; a line indented less is refused with
  `a line of a flow collection must be indented more than its key`. Inside a flow
  mapping that is the whole frontmatter, a line may start at any column.
- Inside a flow collection, a `?` followed by a space, a tab, the end of the line,
  `,`, `]` or `}` starts an explicit key. Its key is the `flow_key` after it, or
  `""` when none is written. Its value follows a `:` as a `flow_pair`'s does; with
  no `:`, it is null. `{? x : 1}` is `{x: 1}`.
- Inside a flow collection, a `:` with nothing before it, followed by a space, a
  tab, the end of the line or one of `, [ ] { }`, starts a pair with the empty key
  `""`. `{: x}` is `{"": x}`.
- Outside a flow collection, a mapping line that starts with `:` followed by a
  space, a tab or the end of the line is a pair with the empty key `""`. Its value
  is read as the value after a key's `:` is, and the empty key written more than
  once holds a list as any key does. `: v` is `{"": v}`, and `a: 1` then `: v` is
  `{a: 1, "": v}`. After an explicit key, the first such line at the `?`'s column
  gives that key's value: `? k` then `: v` then `: w` is `{k: v, "": w}`, and `? a`
  then `:` then `: b` is `{a: null, "": b}`.
- A `flow_entry` of a flow list that is a key followed by `:`, an explicit key or an
  empty key is a mapping of one pair: `[a: b, c]` is a list holding `{a: b}` and
  `c`, `[? x]` is `[{x: null}]`, `[: x]` is `[{"": x}]`, and `[?]` is
  `[{"": null}]`.
- `double_quoted` reads the escapes `\0 \a \b \t \n \v \f \r \e \" \/ \\`, a
  backslash before a space, `\xXX`, `\uXXXX` and `\UXXXXXXXX`. Inside
  `single_quoted`, `''` is one quote.
- A quoted value may go on over several lines, folded as a `PLAIN` is, and so may a
  quoted key inside a flow mapping or after a `?`; inside `double_quoted`, a `\` at
  the end of a line removes the line break. Each line of a quoted value must be
  indented more than its key or `-`, inside a flow collection too, and each line of
  a quoted key inside a flow collection more than that collection's key or `-`,
  except inside a flow mapping that is the whole frontmatter. `a: {"x` then
  `  y": 1}` and `a: {'x` then `  y': 1}` are `{a: {"x y": 1}}`. Any other quoted
  key stays on its line: `"x` then `  y": 1` is refused with
  `the string is not closed`, and `a: ["x` then `  y": 1]` with
  ``expected `,` or `]` in the list``.
- Blank lines and lines holding only a comment are skipped. A line holding only a
  comment may have tabs before its `#`.
- A key written more than once becomes one key holding a list of every value
  written for it, by the same rule as in TOML: `tags: [a, b]` then `tags: c` is
  `tags: [a, b, c]`. An explicit key counts as written: `? k` then `: a` then
  `k: b` is `k: [a, b]`.
- `ANCHOR` is `&` and a name, and `ALIAS` is `*` and a name. A name runs to the first
  space, tab, line ending or one of `, [ ] { }`; a `&` or a `*` with no name is
  refused with ``expected a name after `&` `` or ``expected a name after `*` ``.
  The properties of a node, its anchor and its tag, come in either order; spaces and
  tabs may follow each, and inside a flow collection line breaks and comments too.
- An anchor on a value or on a key names that node, and an alias reads as a copy of
  the node its anchor names, tag and all. `author: &ada Ada` then `reviewer: *ada` is
  `{author: Ada, reviewer: Ada}`, `base: &b` then `  x: 1` then `copy: *b` is
  `{base: {x: 1}, copy: {x: 1}}`, `tags: [&t one, two, *t]` is
  `{tags: [one, two, one]}`, and `a: !!str &s 1` and `a: &s !!str 1` both name the
  String `"1"`. An anchor on an empty value names null.
- An anchor on a key names the key's scalar, typed as the same text written as a
  value, and an alias as a key is the text of the scalar it names: `&k a: 1` then
  `*k : 2` is `{a: [1, 2]}`, the key written twice. A `:` right after an alias is part
  of its name. An alias that names a list or a mapping, as a key, drops its pair, as
  a key that is a list or a mapping does.
- An alias copies the most recent anchor of its name before it: `a: &x 1`, `b: &x 2`,
  `c: *x` is `{a: 1, b: 2, c: 2}`. An anchor inside a dropped entry is forgotten with
  it, so an anchor of its name before the entry is the most recent again:
  `a: &x 1`, `b: [&x 2, @bad]`, `c: *x` is `{a: 1, c: 1}`, and `a: [&x 1, @bad]`,
  `b: *x`, `c: 3` is `{c: 3}`, `b` dropped as an alias whose anchor is not defined
  before it.
- An alias inside the value its own anchor names drops the innermost key that holds
  it, with ``the alias `*A` lies inside the value its anchor names``, and so does an
  alias whose anchor is not defined before it, with
  ``no anchor `&B` is defined before this alias``; the mapping keeps its other pairs.
  `a: &A [*A]` then `b: 2` is `{b: 2}`, and `a: *B` then `b: &B 1` is `{b: 1}`.
- The anchors and aliases of one frontmatter copy at most 100000 values and at most
  1000000 bytes of text. Each list, mapping and scalar of a copy counts as one value,
  and each key and scalar of it counts the bytes of its text: a string's own, any
  other scalar's as written. The copy each alias makes counts, and so does the copy
  each anchor keeps of the node it names; an alias as a key counts as one value and
  the bytes of its text. The anchor or the alias that passes either bound, and each
  anchor and alias after it, drops the innermost key that holds it, with
  `the anchors and aliases copy more than 100000 values` or
  `the anchors and aliases copy more than 1000000 bytes of text`. `a: &s` and a
  string of 10000 bytes, then `b:` and a list of 100 aliases `*s`, drops `b`: with
  the anchor's own copy, the text copied comes to 1010000 bytes.
- An alias with a tag or an anchor is refused with
  `an alias holds no tag or anchor`, pointing at the first of them: `!!str *x` is
  refused. A value has at most one anchor: `a: &x &y 1` is refused with
  `a value has at most one anchor`, pointing at the second.
- In block context, properties may end their line. The value is then what the lines
  under them hold, as for a key with nothing after its `:`, and the first of those
  lines may start with the rest of the properties: `a: !!str` then `  &s 1` names the
  String `"1"`.
- Every node of a copy spans the alias. The writer writes each copy in full and no
  anchor.
- A `<<` key has no special meaning, as in YAML 1.2.2: `e:` then `  <<: *d` is a key
  `<<` holding a copy of `d`.
- Values nest at most 400 deep, by the cap stated under
  [TOML frontmatter](#toml-frontmatter), and the innermost key that holds a value past
  it is dropped with its value: `deep:` and a list nested 401 deep under it, then
  `b: 2`, is `{b: 2}`. The properties of a collection, a tag, an anchor or both, and
  an explicit key each count as a level of their own, and an explicit key whose `?`
  passes the cap drops its own pair. A copy counts from where its alias stands: an
  alias inside 2 values that names a list nested 399 deep passes the cap. A
  frontmatter that holds one list or one scalar nested past the cap is empty.

`BLOCK_SCALAR` is read whole, by YAML 1.2.2 section 8.1:

- Its header is `|` (literal) or `>` (folded), then in either order an optional
  chomping indicator, `-` (strip) or `+` (keep), and an optional indentation digit
  from 1 to 9, then the end of the line, or a comment.
- Its lines are those indented more than the key or the `-` it belongs to. The
  column of its text is that key's or that `-`'s column plus the digit; with no
  digit, it is the column of the first line of text, and a blank line before that
  line may not be indented more: such a line is refused with
  `a blank line before the text of a block scalar is indented more than the text`.
  The scalar ends before the first line of text indented less than that column.
- A literal scalar keeps every line break. A folded scalar joins two lines of text
  with a space, turns each empty line into a line break, and keeps the line breaks
  around a line indented more than the text.
- Clip, the default, keeps the line break after the last line of text and drops the
  empty lines after it. Strip drops both. Keep keeps both.

`TAG` is `!!` followed by `str`, `int`, `float`, `bool`, `null`, `map`, `seq` or
`timestamp`, or the same type written in full, such as `!<tag:yaml.org,2002:str>`.
Any other tag, such as `!point`, drops the innermost key that holds it, with its
value, and the mapping keeps its other pairs. `a: !point {x: 1}` then `b: 2` is
`{b: 2}`, `!point a: 1` then `b: 2` is `{b: 2}`, `tags: [!point x, y]` then `b: 2` is
`{b: 2}`, `owner:` then `  pin: !point 1` then `  name: Ada` is
`{owner: {name: Ada}}`, and `a: [!point x: 1, c]` is `{a: [{}, c]}`. The value after
such a tag is not read, up to the end of the pair; a key after it is read first. A
frontmatter that holds one list or one scalar with such a tag is empty. A value has
at most one tag:
`a: !!str !!str x` is refused with `a value has at most one tag`, pointing at the
second tag, and so is a second tag on a key, inside a flow collection, or on the
line under properties that end their line.

- A tagged scalar's text is read again by the rules for an unquoted value of the
  tag's type in the table below: `!!str 01234` is the String `"01234"`, and
  `!!timestamp` reads a Date, an OffsetDateTime or a DateTime. `!!float` reads a
  Float, and also reads decimal digits of any size with an optional sign, so
  `!!float 1` is the Float `1` and `!!float 99999999999999999999` is the Float
  `1e20`. `.inf` and `.nan` are text, so `!!float .inf` is refused.
  `!!float 1e400` is the String `"1e400"`, and
  `!!int 99999999999999999999` is the String `"99999999999999999999"`, as the same
  digits are untagged. `!!map` and `!!seq` only check that the value is a
  mapping or a list.
- A value that does not fit its tag is refused, as
  ``"`abc` is not a value of the tag `!!int`"``, or
  ``"a list is not a value of the tag `!!map`"``.
- A tag on a key checks the key's text the same way.
- A tag followed by `:` and a space, a tab or the end of the line, or inside a flow
  collection by `:` and one of `, [ ] { }`, is a tagged empty key: the key is `""`,
  and it is checked against the tag as an empty value. `!!str : v` is `{"": v}`,
  `a: 1` then `!!str : v` is `{a: 1, "": v}`, `{!!str : v}` is `{"": v}`,
  `{!!str :}` is `{"": null}`, `{!!str :, a: 1}` is `{"": null, a: 1}`, and
  `a: [!!str :]` is `{a: [{"": null}]}`. A tag written in full ends at its `>`, so
  `!<tag:yaml.org,2002:str>: v` is `{"": v}` too. `!!int : v` is refused with
  ``an empty value is not a value of the tag `!!int` ``.

A `PLAIN` value's type comes from how it is written:

| Written as | Type |
|---|---|
| Nothing, `~`, `null`, `Null`, `NULL` | Null |
| `true`, `True`, `TRUE`, `false`, `False`, `FALSE` | Boolean |
| An optional sign and digits; or `0x`, `0o` and digits in that base. A decimal, octal or hexadecimal integer that does not fit 64 bits is the String of its text as written, such as `"0xFFFFFFFFFFFFFFFFFF"`, and is written back as quoted text | Integer, 64 bits; String when it does not fit |
| An optional sign; then digits, a dot and optional digits, or a dot and digits, or digits alone when an exponent follows; then an optional exponent: `e` or `E`, an optional sign and digits. `.5`, `5.`, `-.5e3` and `1e3` are floats, and `.` is a string. A float too large for a float to hold is the String of its text as written, such as `"1e400"`, and is written back as quoted text | Float, finite; String when it does not fit |
| `.inf`, `.Inf`, `.INF`, with an optional sign; `.nan`, `.NaN`, `.NAN`: arky holds no float that is not a finite number. Each is the String of its text as written, such as `".inf"`, and is written back as quoted text | String |
| `2026-10-10`, when that day exists in the calendar | Date |
| A date-time, written as in TOML, whose date exists in the calendar | OffsetDateTime, or DateTime when it has no offset |
| Anything else | String |

## What each node's span covers

Spans belong to the parse: the parser builds the located tree of
[`src/parser/located.rs`](src/parser/located.rs), each node with its span, which the
claim pass reads, and `Parsed.dropped` gives each dropped entry with its span. The
document a parse gives, a `RawDocument`, holds none. The rows `Mapping`, `Pair`,
`Sequence` and `Scalar` give the ranges the TOML and YAML readers keep for a
frontmatter's values while they read, which the error of a dropped entry or of an
invalid frontmatter points into.

| Node | From | To |
|---|---|---|
| `Document` | The first byte of the file | The last |
| `Frontmatter`, `BrokenFrontmatter` | The opening delimiter | The end of the closing delimiter |
| `Mapping` written as `{ ... }` | `{` | `}` |
| `Mapping` inside a value, whose every pair is dropped | Empty, at the start of its first pair | |
| `Mapping` with no pairs written, at the root | Empty, on the line after the opening delimiter | |
| `Mapping` of a YAML frontmatter that holds one list or one scalar | The value | The end of the value, or of the frontmatter's last text when the value is dropped |
| `Mapping` of a TOML table with no pairs | Empty, right after its header | |
| Any other `Mapping` | The start of its first pair | The end of the last value written in it |
| `Pair` | Its key, the `?` of an explicit key, the first part of a dotted key, or the header that makes it | The end of its value, or for a header, the end of the last line of the table it opens that is not a dropped pair, where the key is first written; `repeats` holds the same range for each later place the key is written |
| `Sequence` written as `[ ... ]` | `[` | `]` |
| `Sequence` written as `-` lines | The first `-` | The end of the last value |
| `Sequence` written as `[[name]]` headers | The first header | The end of the last line of the last table |
| `Sequence` of the values of a key written more than once | The first value's start | The first value's end; for tables a header wrote again, the end of the last line of the last table |
| `Scalar` | Its first character, quotes included | Its last |
| `Scalar` that is null because nothing is written | Empty, right after the `:`, the `-` or the properties, or at the end of a flow key with no `:` | |
| `Scalar` that is null because an explicit key has no `:` line | Empty, at the end of the key | |
| `Scalar` written as a block scalar | `\|` or `>` | The end of its last line of text, or of its header when it has none |
| `Scalar`, `Mapping` or `Sequence` after properties | The value, without its properties | |
| `Scalar`, `Mapping`, `Sequence` or `Pair` of a copy an alias made | The alias | |
| `MarkdownBlock` of the document | The line after the frontmatter, or the first byte of a file with none | The last byte of the file |
| `MarkdownBlock` of an item or a table cell | The start of its first section | The end of its last section; empty at the end of the item's marker, or at the start of the cell, when it has no section |
| `MarkdownBlock` of the document a `markdown` code block holds | The first byte of the block's lines between its fences, or of the line after their frontmatter | The end of the last of them |
| `Section` | Its heading, or its first block or subsection when it has no heading | The end of its last block or subsection |
| `Heading` written with `#` | The first `#` | The end of the line's text |
| `Heading` written underlined | Its first line's text | The end of its underline |
| `Heading` with no text | Empty, at its first `#` | |
| `Paragraph` | Its first character | The end of its last line's text |
| `Line` | Its first character | The end of its text, or past the spaces of a line break that ends it, or past the spaces and tabs before its line ending when a code span or inline HTML goes on from it |
| `List` | The first marker | The end of the last item |
| `Item` | Its marker | The end of its last section, or of the marker when the item is empty |
| `CodeBlock` written fenced | The opening fence | The end of the closing fence, or of its last line when no fence closes it |
| `CodeBlock` written indented | The code column of its first line, or the tab it falls inside | The end of its last line |
| `BlockQuote` | Its first `>` | The end of its last line |
| `Html` block | Its first line, from the column of the enclosing item's text, or the tab it falls inside | The end of its last line |
| `ThematicBreak` | Its first mark | Its last mark |
| `Table` | Its first line's text | Its last line's text |
| `TableRow` | The row line's text, or the first line's text of a grid row | The end of that line, or of the grid row's last line |
| `TableCell` | The cell's text without the spaces around it, or for a grid cell from the first of its lines that holds text; empty at the start of an empty cell, and at the end of its row for a cell added to fill the row | The end of that text |
| `Text`, `Code`, `Link`, `Image`, `Emphasis`, `Strong`, inline `Html` | The first character, delimiters included | The last |
| `LineBreak` | Its backslash, or the spaces that make it | |
| A piece of a node cut over lines | The node's start, or the start of its line's text | The node's end, or the end of its `Line` |

Apart from `Document` and the document's `MarkdownBlock`, a span never includes the
line ending after the node. Inside an underlined heading, a `Text` that goes on from
one line to the next covers the line ending and the indentation between them.

## Extensions

A consumer of arky gives its own node types to the constructs it claims, through
the `Extension` trait in [`src/parser/extension.rs`](src/parser/extension.rs):

```rust
pub struct Kind<E>(pub Option<Box<E::Node<E>>>)
where
    E: Extension;

pub trait Extension: Clone + Debug + PartialEq + Sized {
    type Node<T>: Clone + Debug + PartialEq
    where
        T: Extension;

    fn claim<T>(
        construct: Construct<'_, T>,
        context: &Context<'_>,
    ) -> Option<Result<Self::Node<T>, ParseError>>
    where
        T: Extension;

    fn write<T>(node: &Self::Node<T>, plain: Written<'_, T>, writer: &mut Writer)
    where
        T: Extension;
}
```

- Every node of the markdown tree takes the extension set as a type parameter and
  carries `kind: Kind<E>`: the node an extension gave it, or `None`. The node is boxed,
  because a markdown block can hold a markdown block. `T`, the full set of extensions
  the file is parsed with, types the arky nodes an extension's own node holds.
- `Construct<'node, T>` has one variant per node type, each holding a reference to
  that node of the located tree, spans included: `BlockQuote`, `Code`, `CodeBlock`,
  `Document`, `Emphasis`, `Heading`, `Html`, `Image`, `Item`, `Line`, `LineBreak`,
  `Link`, `List`, `MarkdownBlock`, `Paragraph`, `Section`, `Strong`, `Table`,
  `TableCell`, `TableRow`, `Text` and `ThematicBreak`. `Written<'node, T>` has the same
  variants over the model's nodes, which hold no span: `write` gets the plain node a
  claimed node is written over. It also has `LinkEmphasis` and `LinkStrong`: an
  emphasis or a strong that an extension claimed inside a link's text, offered as
  `Construct::Emphasis` or `Construct::Strong`, comes back as one of them.
- arky builds the whole located tree first, then offers every node to the
  extensions, children before parents: a section's heading, blocks and subsections
  before the section, an item's or a cell's markdown block before the item or the
  cell, a document's markdown block before the document. A claimed node goes into
  `kind`, and the plain node stays in the tree under it. At the end of the parse, the
  located tree moves into the model, each `kind` with its node.
- `claim` returns `None` to leave the node as arky built it, and `Some(Ok(..))` to
  claim it. `Some(Err(..))` leaves the plain node by default
  (`OnExtensionError::Fallback`), and arky reads the rest of the file. A strict
  parse, `ParseOptions::strict()`, which sets `OnExtensionError::Fail`, fails with
  that error instead.
- `Context` holds the `source`, whether the parse is `strict`, the `column` the
  offered construct's first line starts at, the `depth`, how many list items, markdown
  blocks and table cells hold the construct, its `frame`, and the `lines` of the grid
  cell it lies in, or `None` outside grid cells. The `frame` holds the column each
  markdown block around the construct reads its lines past, outermost first, and only
  arky reads it. The `column` counts in the frame of the markdown block that holds
  the construct: on its line, or its cell's line in a grid cell, read past each column
  of the frame in turn, its tab stops restarting where each ends. A grid cell's lines
  start a frame of their own. `context.markdown::<T>(range)` reads a range of the
  source as a markdown block with the extensions `T`, one container deeper, each line
  read past the frame, then with at most `column` columns of its leading white space
  left out and its tab stops counted from `column`, as in a code block's value: a tab
  moves to the next multiple of 4 past it. In a grid cell it reads only the parts of
  the range on the cell's lines, and its tab stops count from `column` there too. It
  gives the model's `MarkdownBlock`; while the block is claimed, its spans count from
  the start of the source. `context.document::<T>(range)` reads a range as a whole
  document, its lines read as `context.markdown` reads them: a frontmatter on its
  first lines, then its body. It gives a `RawDocument`, `Invalid` when no entry of the
  frontmatter reads, whose body arky's markdown extension alone claims.
  In a strict parse, an entry its frontmatter drops fails it; the entries it drops are
  not listed in `Parsed.dropped`, which lists the file's own frontmatter.
- No node that the block cap of list items, markdown blocks and table cells holds is
  offered, so a markdown block an extension reads with `context.markdown` nests under
  the same cap as the file's own.
- `Plain` claims nothing; its `Node<T>` is `Infallible`. `Markdown` is arky's own
  extension, written as a consumer writes one: it claims a code block whose language
  is `markdown` or `md`, in any case, as the whole document its content reads as, a
  `RawDocument`, its frontmatter included, and writes it back fenced with the block's
  info string, a valid document's frontmatter in the syntax the write picks.
- `(A, B)` is two extensions, whose `Node<T>` is `Either<A::Node<T>, B::Node<T>>`: the
  first one that claims a construct gets it. More nest as pairs.
- `With<E>` is `(Markdown, E)`. `parse(source)` reads with `Markdown` alone, and
  `parse_with::<E>(source, &options)` reads with `With<E>`, so arky's markdown
  extension is always first. Each gives a `Parsed`: its `document`, a `RawDocument`,
  and `dropped`, each frontmatter entry the parse could not read, with its span into
  the source. `parse` returns it as it is, since a parse that is not strict cannot
  fail, and `parse_with` a `Result`, an error only in a strict parse.
- A code block's `language` and `args`, `Args::Toml` or `Args::Text`, come from its
  info string by the rule under [Blocks](#blocks).
- `read_toml(source, range)` and `read_yaml(source, range)` read the TOML or YAML
  written at any range of a source into a `Mapping`, as the frontmatter is read. Each
  drops nothing: it fails with the error of the first entry or key the frontmatter
  would drop, as a strict parse does, its span counting from the start of the source.
- `read_toml_lines(source, lines)` and `read_yaml_lines(source, lines)` read the TOML
  or YAML written on `lines`, ranges of the source, as consecutive lines of one text
  joined by `\n`, and place the span of an error back in the source.
  `context.lines_of(range)` gives the parts of a range on the construct's own lines:
  on each of its grid cell's lines, or else the range split at each `\n`, which it
  leaves out, each line past the construct's indentation: past the frame, then past
  at most `column` columns of its leading white space, a tab that `column` falls inside
  passed whole, as `context.markdown` reads it. A part that starts later on its line
  keeps its start. So an extension reads a header the same way inside a grid cell and
  out, with `read_yaml_lines(context.source, &context.lines_of(range))`.
- `to_json(document)` writes a `RawDocument` as JSON, and `from_json::<E>(text)` reads
  it back, at any depth arky reads. No node writes its `kind` or a span. A document
  is written `{"t":"valid",...}`, or `{"t":"invalid",...}` when no entry of its
  frontmatter reads, each field under its short name: `bd` for the body and `data`
  for the frontmatter. An invalid frontmatter is `{"er": the error, "fo": "yaml" or
  "toml", "tx": its text}`. A frontmatter value is written plain: a boolean, a number,
  a text, a list and a mapping as themselves, a null as `null`; a date, a date-time
  and a date-time with an offset as `{"$date":{"$numberLong":"<milliseconds>"}}`, read
  back as a `Date` when it is at midnight UTC and as an `OffsetDateTime` in UTC
  otherwise; a time of day as `{"t":"time","v":"07:32:00"}`.
  `rebuild_with::<E>(document, &options)` builds the extension nodes again on
  such a document: it writes the plain tree in arky's form, with the frontmatter in
  YAML, an invalid one as it was read, and parses that text with `E`. It fails with
  `RebuildError::Write`, naming the key, when the `Toml` args of a code block hold a
  null, or naming the line when the text of an invalid frontmatter holds one that
  would close it, and with `RebuildError::Parse` when the parse fails.
- `Writer`, which `write` hands to an extension, has `push_str`, which adds text as
  it is; `markdown_block`, which writes a markdown block with its levels counted from
  one; `raw_document`, which writes a whole document, its frontmatter then its body,
  as `write` does; `construct`, which writes a plain node in arky's form, each of
  its children by the extension that claimed it, if one did; `fenced`, which writes a
  fenced block with the info string it is given; and `info_string`, which gives a
  code block's info string as the writer writes it and records the error of a null in
  its `Toml` args.
- The tree types that take the extension as a parameter compare with `PartialEq`
  only, so an extension's own node types need not implement `Eq`.

## Writing

`write(document, &options)` writes a document in arky's own form: one form for each
construct, whatever form the file used. It uses another form only where its own form
would read back as another tree. Every form meets one rule: a file read and written,
then read and written again, gives the same text the second time.

| Construct | Written as |
|---|---|
| Document | The frontmatter, if it has one, in the syntax `WriteOptions::frontmatter` picks, YAML by default, also for a file read with TOML frontmatter, and so for the document a `markdown` code block holds; an invalid document's frontmatter as it was read, its text between `---` lines for YAML and `+++` lines for TOML, whatever `WriteOptions::frontmatter` picks, because the text reads only in its own syntax. When that text holds a line that would close the frontmatter, as `TOML_CLOSE` or `YAML_CLOSE` does, such as `---` alone on its line for YAML, the write fails with an error that names that line, because the text would read back cut at it. Then its markdown block. A byte-order mark, U+FEFF, that would be the first character of the file is written `&#65279;`, because the reader skips a plain one there; one anywhere else is written as it is. The text ends with one line ending; an empty document is empty text. |
| Blank lines | One between two blocks and after a heading. Inside a tight list, none between the blocks of an item. |
| Heading | `#` marks, one per level of depth, then a space and its text, also for a heading read underlined. A final run of `#`s in its text that a space or a tab precedes, or that is the whole text, gets a backslash before its first `#`, because the reader would take the run for the closing sequence: `Title ##` is written `# Title \##`. A heading line with no text is its marks alone. Every heading line is written. |
| A heading that holds a line break | At depth 1 or 2, written underlined: its lines, each but the last ended by the break's `\`, guarded as a paragraph's lines, then `===` at depth 1 or `---` at depth 2 on a line of its own, because a `#` heading is one line. At depth 3 to 6, and right after other text in a tight list item, where a blank line would make the list loose, the one-line form, with the break written `&#10;`. |
| A section with no heading | No line where a later read builds it again: as the first section of a markdown block that holds blocks or subsections, or as the first subsection of its parent that holds subsections and no blocks, a skipped level. Any other section with no heading, which only a tree built in code holds, gets a heading line with no text, the `#` marks of its depth. |
| A section seven or more levels deep | Inside its parent, as a ```` ```markdown ```` block where the levels start over at `#`. The rule repeats inside each block, and each block's fence is longer than any fence inside it. |
| Paragraph | Each line on a line of its own. |
| Bullet list | `-` and one space before each item. A bullet list right after another bullet list in the same section uses `*`, and the next one `-` again, in turn; a list an extension claimed, or any other block between them, ends the turn. An item of a `*` list whose first line is the thematic break `***` writes it `___`, because `* ***` reads as a thematic break. |
| Ordered list | The number, `.` and one space before each item, numbered up from the list's first number. An ordered list right after another ordered list in the same section uses `)`, as in `1)`, and the next one `.` again, in turn. |
| Item | Its lines after the first indented to its text. An empty item is its marker alone. A first line that starts with a space or a tab goes on the line after the marker, indented to the item's text, and so does a first line that would make the marker's line a thematic break: an item holding an item holding an empty item is written `-`, then `- -` on the next line, because `- - -` reads as a thematic break. |
| Loose list | A blank line between its items. |
| Code block | Fenced, also when it was indented: a fence of backticks one longer than the longest run of backticks in its content, and at least three, or of tildes, measured on the tildes, when its info string holds a backtick, because a backtick fence's info string cannot hold one. The info string is the language, then one space and the args: `Text` args as written, and `Toml` args as one line of `key = value` pairs separated by `, `, each value in the TOML form below, with a mapping as an inline table, such as `site = { lang = "en" }`. Empty `Toml` args write nothing. A pair holding a null is left out, and the write fails with an error that names its key. |
| Block quote | `> ` before each line of its text, and `>` alone on an empty line. |
| HTML block | As written. |
| Thematic break | `***`. A `***` line is always a thematic break: `---` right under a line of text underlines a heading, and `---` on a file's first line opens frontmatter. |
| Table | A pipe table, its columns padded to the widest cell and at least three wide, its delimiter row holding the alignments, and every `\|` of a cell escaped, when it has exactly one header row and every cell's content fits on one line. Otherwise a grid table: `+---+` borders, a `+===+` border under the header rows, and the alignments on the `+===+` border, or on the top one when there is no header row. |
| Emphasis, strong | `*a*` and `**b**`. Emphases next to each other, or strongs, each whole on its line, are written as one span: `*a*_b_` is written `*ab*`, because `*a**b*` reads as one emphasis holding `a**b`. Strong around emphasis is written `***a***`, which reads back as emphasis around strong and looks the same. |
| Code span | Backticks, a run one longer than any run of backticks inside, and the same run on every piece of a span cut over lines. A space pads the content inside them when it starts or ends with a backtick, or starts and ends with a space and is not all spaces, because the reader strips one space from each end. |
| Link | Inline, `[text](destination "title")`, also for a reference link, whose definition is not written, and for an autolink. A wiki link stays `[[target\|text]]`. |
| Image | `![alt](destination "title")`, or `![[target\|alt]]` for a wiki embed. |
| Destination, title | A destination that holds a space or a control character, starts with `<`, or whose parentheses do not pair up, between `<` and `>`; an empty one with a title as `<>`. A title between `"`. A backslash goes before each `<` and `>` of a destination between them, each `"` of a title, each `&` that would start a reference, and each backslash before punctuation or the end. A line ending in a destination or a title is written `&#10;`. |
| Hard line break | A backslash at the end of the line. |
| Inline HTML | As written. |
| A node cut over lines | Once, over the same lines: its opening mark before its first piece, its closing mark, or a link's target, after its last. |

Text is written so that it reads back as the same text:

- a backslash goes before each character that would read as markup there: a
  backslash that stands before punctuation or the end of the line, a backtick, a run
  of `*` or `_` that could open or close emphasis by the reader's flanking rules, a
  `&` that would start a reference, a `<` before anything but white space or the end
  of the line, a `[` before `[`, a `]` before `(`, and a `!` before `[`; between a
  link's brackets, every `[` and `]`;
- a space or a tab at the start or at the end of a line of text is written `&#32;` or
  `&#9;`, and a line ending inside a text `&#10;`, because the reader drops a plain
  space or tab there and reads a plain line ending as a new line.

A line that starts with text gets a backslash before its first character when that
character would start a block: a heading, a block quote, a bullet, a thematic break, a
tilde fence, or, on a line after a paragraph's first, a heading underline; and before
the `.` or `)` of what would read as an ordered list's number. A line after the first
of a paragraph or of an underlined heading that starts with inline HTML or a piece of
a code span, where a backslash would read as text or as code, is indented four spaces
when it would start one of those blocks, a backtick fence, or an HTML block of a kind
that interrupts a paragraph: indented four spaces after text, a line continues the
paragraph, and the reader drops the spaces. The lines of a paragraph, and those of an
underlined heading, are guarded as a whole:

- a backslash goes before the `[` of a first line that starts with text when the lines
  would read as a link reference definition;
- the first line of a file with no frontmatter, `+++`, is written `\+++`, so that it
  does not open a TOML frontmatter;
- when the lines would read as a table, every `|` of their text is escaped.

The frontmatter keeps its keys in the order they were read, and a key read twice is
written once, with its list.

- **YAML**, between `---` lines. Mappings in block form, the first key of a mapping in
  a list on its `-` line. A list that holds a mapping, also inside a list it holds, in
  block form; any other list in flow form, `[a, b]`, and an empty mapping as `{}`.
  Text plain, unless written plain it would read as something else: when it starts
  with a space or an indicator, one of `` ! " # % & ' * , - : > ? @ [ ] ` { | } ``,
  ends with a space or `:`, holds `: `, ` #` or a control character, holds `,`, `[`,
  `]`, `{` or `}` inside a flow collection, has the form of a time of day, of an
  integer that does not fit 64 bits, of a float too large for a float to hold or of
  a float that is not a finite number, such as `.inf`, or reads as another type;
  then in double quotes, with `\"`, `\\`, `\n`, `\r`, `\t` and `\xXX` escapes.
  `null` for a null; `true` and `false`; a float as the shortest text that reads
  back as it, with `.0` when that text would read as an integer, and a float that is
  not a finite number, which no parse gives, as the quoted text `".inf"`, `"-.inf"`
  or `".nan"`; dates and date-times after the tag `!!timestamp`, such
  as `!!timestamp 2026-10-10T07:32:00Z`, with `Z` for UTC or an offset such as
  `+02:00`; a time of day as quoted text, such as `at: "07:32:00"`, which reads back
  as text.
- **TOML**, between `+++` lines. The plain keys of a table first, then, in the order
  they were read, a `[path]` header for each mapping it holds and a `[[path]]` header
  for each mapping of a list that holds mappings and nothing else, each after a blank
  line unless it opens the frontmatter. Any other list as an inline array, a mapping
  in it as an inline table. A key bare when it is ASCII letters, digits, `_` and `-`,
  and quoted otherwise. Text always in double quotes, with `\"`, `\\`, `\n`, `\r`,
  `\t`, `\b`, `\f` and `\uXXXX` escapes; dates, times and date-times bare; a float
  that is not a finite number, which no parse gives, as the quoted text `"inf"`,
  `"-inf"` or `"nan"`. TOML cannot hold a null: writing one fails with
  `TOML cannot hold a null`, naming the key's path.

One case is set aside for now: emphasis directly inside emphasis, such as `_*a*_`, is
written `**a**`, and reads back as strong.

[`src/writer/tests.rs`](src/writer/tests.rs) holds the full list of written forms, each
with the text it reads from and the text it writes.

## Where it differs

### From CommonMark

1. A block quote holds text: nothing is read inside it, and only its `>` lines belong
   to it. CommonMark reads blocks inside it, and a line with no `>` can continue its
   paragraph.
2. A table cell holds markdown: paragraphs, lists, code blocks, tables and sections
   with headings, as long as they stay inside the cell.
3. A fenced block whose language is `markdown` or `md` holds a whole document, its
   frontmatter included, read by these same rules. CommonMark reads it as code.
4. A line further left than a list item's text ends the list and starts a new
   paragraph. CommonMark would continue the item's paragraph with it.
5. An image's `alt` is the text between its brackets as written. CommonMark reads
   inline nodes there.
6. `[[target|alias]]` is a wiki link and `![[target|alias]]` a wiki embed. CommonMark
   reads both as text.
7. Inline nodes nest at most 1000 deep, and list items, markdown blocks and table
   cells at most 40 deep together. Past either cap, the marks stay text.

### From GitHub's tables

1. No cell is dropped. A row with more cells than the header widens the table, and
   every shorter row gets empty cells. GitHub drops the extra cells.
2. A dash line of another width than the header still makes a table, as wide as the
   widest of its rows. GitHub reads it as text.
3. A link definition in a pipe cell is a definition of the file. GitHub keeps it as
   the cell's text.

### From Pandoc's tables

1. A simple table needs its header line, and multiline tables are not read: the dash
   line that opens either of them is a thematic break, and the lines after it are read
   on their own.
2. A simple table's rows run to the next blank line, so a dash line that closes the
   table is one more row. Pandoc ends the table at that line.
3. A wall out of line in a grid row stays text in the cell it falls in, and the row
   keeps one cell per column. Pandoc joins the cells on either side of it into one.

### From TOML

1. A key or a table written more than once becomes one key holding a list of its
   values, and so does a key that a dotted key or a header goes through after its value
   was written with `=`, or a table a header names after a header or a dotted key made
   it. TOML refuses each of them.
2. An integer that does not fit 64 bits is the String of its text as written. TOML
   refuses it.
3. A value or a dotted key nested past 400 levels drops its pair, and a header past
   them drops its table with its pairs. TOML reads them.
4. A pair or a header that does not read is dropped, and arky reads the rest of the
   file. TOML refuses the file.

### From YAML

1. Dates and date-times are in the schema: an unquoted date whose day exists in the
   calendar, or a date-time whose date does, is a Date, an OffsetDateTime or a
   DateTime. The YAML 1.2.2 core schema has no date type and reads it as a string.
2. An alias is a copy of the value its anchor names, and a write holds the copies and
   no anchor; in YAML both name one node. An alias inside the value its anchor names,
   an alias whose anchor is not defined before it, and the anchors and aliases past
   100000 copied values or 1000000 copied bytes of text drop the innermost key that
   holds them, with its value; YAML's graph may hold a node inside itself, and an
   alias with no anchor before it is an error there. A tag other than the eight above,
   and a value nested past 400 levels, drop the innermost key that holds them too.
   YAML reads both.
3. A key written more than once becomes one key holding a list of its values. YAML
   does not allow it.
4. A pair whose key is a list or a mapping is dropped, and the mapping keeps its
   other pairs: TOML cannot hold such a key. YAML reads the pair.
5. An entry that does not read is dropped, and arky reads the rest of the file.
   YAML refuses the file.
6. Where the JavaScript library `yaml` 2.9.1 departs from the YAML 1.2.2 grammar,
   arky follows the grammar. `? k` then `  : v` is refused, where the library reads
   `{k: v}`: the `:` of an explicit key's value stands at the column of its `?`. `? a`
   then `:` then `: b` is `{a: null, "": b}`, where the library reads
   `{a: {"": b}}`. `a:` then two spaces, a tab and `!!str x` is `{a: "x"}`, and
   `!<tag:yaml.org,2002:str>: v` is `{"": v}`, both of which the library refuses.
   `!!int : v` is refused, where the library reads `{"": v}`.
