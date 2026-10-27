# Documents and schemas

arky's two core objects are the markdown document and the schema. Neither is a
file. A file on disk is one way a document can be stored and one place a schema
can be written down, but the engine works on the objects, not on the bytes.

## Documents and schemas are objects, not files

The engine's core types are the markdown document as an object and the schema as
an object, neither tied to a path. Two capabilities stand on that abstraction: a
[change](changes-and-versions.md) describes an edit to a resource rather than to a
path, and [version tracking over pluggable backends](backends-and-capabilities.md)
only means something once a document can exist without a filesystem underneath it.

A document is a tree. A `Document` holds a `Frontmatter`, if the file has one, and
its body, a `MarkdownBlock`. The body holds sections by depth: each section holds
its blocks and its subsections, and a heading when the file wrote a heading line
for it. A paragraph holds its lines, and an inline node written across lines is
cut into one piece per line, each with a `continues` that says whether the node
goes on to the next line, from the previous one, or both. A code block holds its
language and its args, read as TOML when they can be and kept as written
otherwise. An extension can give any markdown node a type of its own. The full
grammar is in `crates/arky/GRAMMAR.md`.

*What identifies a document that has no path is not yet specified.*

## The frontmatter engine is the foundation

Frontmatter handling is what the rest of arky stands on. Validating reads and
checks fields; repairing reads and modifies them; classifying an unprocessed
capture writes them from scratch; tracing reads them up a chain of folders;
exporting serializes them. The smallest useful case is a query over one field:

```console
$ arky query 'tags contains "rule"' .grit/patterns/
```

This walks a directory, parses each file's frontmatter, evaluates one expression
against it, and prints the paths that match — no configuration file, no rules, no
inheritance.

arky ships as a Cargo workspace of library crates with a thin command-line crate
on top, rather than as a single program with internal modules.

*Which crates the workspace holds is not yet specified.*

## arky writes its own parser

arky parses markdown with a parser of its own, covering both halves of a
document: the YAML or TOML of the frontmatter and the markdown of the body.
General-purpose crates are reference material rather than dependencies.

The parser has to serve searches that run over document bodies, over frontmatter
when frontmatter is specified, and over both when clauses are given for both — one
of the reasons the parser is arky's own code.

*Performance goals for the parser are not yet specified, and this book makes no
claim about how fast anything is.*

## Whose formatting wins

arky writes each construct in a form of its own, whatever form the file used:
`#` headings, `-` bullets, `*emphasis*`, `***` for a thematic break, backtick
fences, inline links. It uses another form only where its own would read back as
another tree, so a file read and written twice gives the same text the second
time. The frontmatter's format is the one choice: YAML by default, TOML on
request.

Writing a canonical form — a fixed key order, no preserved comments, one spelling
for everything — is remargin's write path. Within a corpus that remargin governs,
every resource still has to satisfy the lint rules configuration allows.

The form arky writes for each construct is stated in the "Writing" section of
`crates/arky/GRAMMAR.md`. *The list of lint rules configuration may allow
is not yet specified.*

## What schemas cover

Schemas rule both halves of a document: frontmatter and structure. What a document
is required to carry follows from the governing schema rather than from a fixed
rule — whether frontmatter is required at all, and which properties are required,
are consequences of the schema, not constants.

*The schema language is not yet specified: how a schema is written, how it
attaches to a document or to a folder, and what it can say about structure as
opposed to frontmatter.* The vocabulary — how a schema relates to a shape and to a
resource type — is taken up on
[Shapes, schemas, and resource types](shapes-schemas-resource-types.md).

## Documents that are not markdown

Images and documents in Portable Document Format are resources arky can hand
back:

```console
$ arky get folder/image.png
$ arky get folder/doc.pdf
```

*What arky does with a non-markdown file beyond returning it is not yet
specified.*

## Related

- [arky get](../reference/cli/get.md) — retrieving a resource.
- [arky query](../reference/cli/query.md) — querying frontmatter.
- [Shape language](../reference/shape-language.md) — writing a shape down.
- [Design status](../design-status.md) — the open questions here.
