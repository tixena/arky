# Query and search

Two verbs read a vault. `arky query` selects documents by an expression over
their frontmatter. `arky search` matches a pattern against documents. This
guide covers choosing between them, writing the expression, and reading what
comes back.

## Step 1: choose the verb

- **`arky search`** runs over document bodies, over frontmatter when
  frontmatter is specified, and over both when clauses are given for both.
  Matching frontmatter and body separately is one reason arky parses markdown
  itself.
- **`arky query`** starts from frontmatter: it walks a folder, parses each
  file's frontmatter, and emits the files whose frontmatter satisfies one
  expression.

*Whether the two are one language — search being a query whose first stage
matches body text — or two separate ones, is not yet specified.*

## Step 2: write the expression

```console
$ arky query 'tags contains "rule"' .grit/patterns/
$ arky query --exp='type=issue'
```

The expression language is like `jq`, the command-line processor for JavaScript
Object Notation (JSON) in which small filters join into pipelines: expressions
chain, and a query can name what it emits rather than always emitting whole
documents.

*The grammar itself — operators, quoting, escaping, how an expression is written
as a command-line argument, and which of the two spellings above is the command
line — is not yet specified.* See
[Query grammar](../reference/query-grammar.md).

## Step 3: search a path for a pattern

```console
$ arky search folder/ -gi "[0-9]?[a-z]*"
```

*The syntax of a frontmatter clause and of a body clause, the meaning of the
short flags, and the dialect of regular expression the pattern is written in, are
not yet specified.*

## Step 4: read what comes back

*Not yet specified.* No output is defined for either verb, for what a match is
reported as — a path, a line, a line number, a fragment, or a whole document — or
for what either verb returns to the shell when nothing matched. See
[Exit codes](../reference/exit-codes.md).

## Chain, project, and change

Stages join in the manner of `jq`, each stage receiving the previous stage's
selection, and a projection names what comes out: a set of values, or a fragment
of a document.

The same language drives modification, one document at a time or in bulk —
appending a section carrying the current date to each element a query returns,
for instance. A modification made this way is a change object, with the replay
and unapply guarantees that model carries. Writing into part of one document has
its own guide: [Write through paths](write-through-paths.md).

## Not yet specified

- Whether one bulk modification is one change object or one per document.
- Whether `query`, `search`, and the path expression given to `arky write`
  share one grammar.
- Whether a path defaults to a configured root when it is omitted, whether
  several paths may be given, and whether a path may be a glob.
- What a projection produces: markdown fragments, frontmatter values, file paths,
  or a mix chosen per clause.

See [Design status](../design-status.md) for the open questions here.

## Related

- [Query grammar](../reference/query-grammar.md) — the grammar reference.
- [arky query](../reference/cli/query.md) — the command.
- [arky search](../reference/cli/search.md) — the command and its scope.
- [Write through paths](write-through-paths.md) — addressing and changing part of
  one document.
- [Documents and schemas](../concepts/documents-and-schemas.md) — the parser the
  language runs on.
- [A first session with arky](../tutorial/first-session.md) — the same steps
  front to back.
