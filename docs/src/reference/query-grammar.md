# Query grammar

*The expression language is under active design.*

One expression language serves three entry points. [arky query](cli/query.md)
runs an expression over a set of documents, [arky search](cli/search.md)
matches a pattern against them, and the `--path` option of
[arky write](cli/write.md) addresses a part of one document. The verb fixes
what the expression runs against — a set of documents, or one document — and what
comes out.

## The expressions

```console
$ arky query 'tags contains "rule"' .grit/patterns/
$ arky query --exp='type=issue'
$ arky write folder/file.md --path='# ##["Specs"][2]' --append --from-body='path-to-file.txt'
$ arky search folder/ -gi "[0-9]?[a-z]*"
```

## The document model

A document is frontmatter, which is a mapping, plus a body tree. In the body tree
a heading nests the blocks under it — paragraphs, lists and their items, tables,
code fences, remargin comment blocks — until the next heading of equal or higher
level. arky parses both the frontmatter and the markdown body with its own
parser.

## Selecting

Three kinds of clause:

```
fm.type == "issue"
fm.tags has "rule"
body ~ /[0-9]?[a-z]*/
```

A body position is a path of heading steps followed by an index:
`# ##["Specs"][2]` is the level-one heading, then the level-two heading titled
"Specs" beneath it, then the second child block of that heading. Clauses combine
with `and`, `or`, and `not`.

## Chaining and projecting

Stages join with `|`, in the manner of `jq`, and each stage receives the previous
stage's selection. Almost anything is projectable: a projection names what comes
out, `{path, fm.title}` for values or `.body` for a fragment. The default output
is one value per line in JavaScript Object Notation, with a markdown form for
fragments.

## Mutating

A mutation verb applies to a selection: `append`, `prepend`, `set`, `remove`,
`replace`. The same language selects and modifies, one document at a time or in
bulk — appending a section carrying the current date to every document a query
returns is one invocation. A mass modification is a
[change object](change-objects.md); *whether one invocation produces one change
object or one per document touched is not yet specified.*

## Searching

`search` is `query` with body-text matching as its default first stage. It runs
over document bodies, over frontmatter when frontmatter is specified, and over
both when clauses are given for both; frontmatter and body clauses combine as
they do anywhere else. *The short flags in the example above are not yet
specified.*

## Formatting

arky can preserve an existing YAML formatting rather than rewriting it.
remargin's write path writes arky's canonical form.

## Not yet specified

- Whether the three entry points share one expression language, or the
  document-set query and the within-document path are separate grammars with a
  defined seam between them.
- What a path can address beyond heading level, heading title, and an index among
  children — list items, paragraphs, tables, code fences, remargin comment
  blocks, frontmatter keys — and what the index counts.
- What a projection outputs by default: markdown fragments, frontmatter values as
  YAML or as JavaScript Object Notation, file paths, or a mix chosen per clause.
- Which mutation verbs beyond appending the language carries.
- Whether search is a separate verb with its own clause syntax or a query with
  body matching as its default stage.
- Performance goals for the parser.
- The spelling itself: operators, quoting, escaping, and how a path is written
  inside a pipeline as opposed to as the `--path` option.

See [Design status](../design-status.md) for the open questions around the
language.

## What depends on it

- [Change objects](change-objects.md) — a query-driven mass modification is a
  change, and the transaction boundary is part of the language.
- [Shape language](shape-language.md) — whatever the language can address is
  what a shape can govern.
- Permissions. remargin grants view or edit access to files and to parts of
  documents, showing only the portion a person may see; the language is what
  makes a part addressable.

## Related

- [arky query](cli/query.md), [arky search](cli/search.md), and
  [arky write](cli/write.md) — the three commands the language serves.
- [Output formats](output-formats.md) — what a run prints.
