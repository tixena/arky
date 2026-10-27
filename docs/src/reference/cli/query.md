# arky query

`arky query` selects documents by an expression over their frontmatter and
prints, or modifies, what the expression picks out.

## Synopsis

```console
$ arky query 'tags contains "rule"' .grit/patterns/
$ arky query --exp='type=issue'
```

Two spellings are on record: the expression as a positional argument followed by
a path, and the expression as the value of `--exp` with no path. *Which spelling
is definitive is not yet specified.*

## Behavior

A run walks the given path, finds markdown files, parses the frontmatter of each,
and produces the documents the expression selects. Frontmatter querying is
arky's first capability, and the smallest form of it — walk a tree, parse
frontmatter, print the paths of the files that match — is the first slice built.

The language is modeled on `jq`: queries chain, and almost anything is
projectable. The same language drives modification, one document at a time or in
bulk, so it addresses parts of a document rather than whole documents only. A
worked case is appending a section carrying the current date to every element a
query returns.

arky parses frontmatter and markdown with its own parser rather than depending
on an outside one, which is what lets a query preserve the existing formatting of
frontmatter arky did not write. remargin's write path emits arky's canonical
form instead; preserving an author's formatting is arky's behavior, not
remargin's.

Modification through a query is a [change object](../change-objects.md): a base
document plus a chain of changes applied in order reproduces the latest version
byte for byte, and unapplying the chain in reverse returns the original.

The [Query grammar](../query-grammar.md) page carries the language in the detail
it has so far: a document as frontmatter plus a body tree in which a heading
nests the blocks under it; frontmatter comparisons such as `fm.type == "issue"`
and `fm.tags has "rule"` combined with `and`, `or`, and `not`; stages joined with
`|`; projections such as `{path, fm.title}` for values and `.body` for a
fragment; one value per line in JavaScript Object Notation (JSON) by default,
with `--format md` for fragments as markdown; and the mutation verbs `append`,
`prepend`, `set`, `remove`, and `replace` applied to a selection.

## Not yet specified

- The spelling of the language itself: operators, quoting, escaping, and how an
  expression is written as a command-line argument as opposed to inside a
  pipeline.
- Whether the path defaults to the configured corpus root when omitted, whether
  more than one path may be given, and whether a path may be a glob.
- Whether one query-driven mass modification is a single change object or one per
  document touched.
- Output, and which exit code a query with no matches returns.

Named flags for common fields (`arky query --type project --status active`)
plus a `--where` option carrying an arbitrary expression are not arky's shape;
the language above replaces them.

See [Design status](../../design-status.md) for the open questions around this
command.

## Related

- [Query grammar](../query-grammar.md) — the language in full.
- [arky search](search.md) — matching against document bodies and frontmatter.
- [arky write](write.md) — the path expression that addresses part of one
  document.
- [Change objects](../change-objects.md) — the transactional model a
  query-driven modification produces.
- [Command-line interface](index.md) — global options and the command table.
