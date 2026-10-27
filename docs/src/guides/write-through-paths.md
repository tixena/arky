# Write through paths

`arky write` changes part of one document. A path expression says which part,
and the write applies to exactly that part rather than to the file as a whole.

## Synopsis

```console
$ arky write folder/file.md --path='# ##["Specs"][2]' --append --from-body='path-to-file.txt'
```

It names one document, addresses a part of it with `--path`, appends rather than
replaces, and takes the content to append from the file named by `--from-body`.
The three steps below take that line apart.

## Step 1: address the part

`--path` takes an expression that addresses a part of a document — in the line
above, the second child block under the level-two heading titled "Specs".

*The path grammar is not yet specified, including what else a path may reach —
list items, paragraphs, tables, code fences, remargin comment blocks, and
frontmatter keys — and what an index counts.* See
[Query grammar](../reference/query-grammar.md).

## Step 2: choose the mutation

`--append` adds to what the path addresses. Appending is also the verb of a bulk
change driven by a query: append a section carrying the current date to each
element the query returns.

*The rest of the mutation vocabulary — prepending, setting, removing, replacing —
is not yet specified.*

## Step 3: supply the content

`--from-body=<file>` takes the content to write from a named file.

*Whether content may instead be given inline or read from standard input is not
yet specified.*

## What the write commits

Every write is a transaction, and its unit is a change object: a base document
plus a chain of changes applied in order gives a byte-identical latest version,
and unapplying the chain in reverse returns the original.

That guarantee is reachable because the content is linted markdown with a known
shape. A change to arbitrary text can only be recorded as a difference between
two texts; a change to a document whose structure is known can be recorded in
terms of that structure. So the write path rests on the linter and on shapes —
see [Define a shape](define-a-shape.md).

*Where a transaction ends when one expression changes many documents — one change
object covering every document touched, or one per document — is not yet
specified.* See [Change objects](../reference/change-objects.md).

## Why the addressing reaches beyond this command

Parts of a document are units of access. remargin grants view or edit access to
files and to parts of markdown documents, showing a reader only the portion they
may see, and whatever a path expression can address is what such a permission can
be attached to. Shapes govern sub-parts for the same reason, so the vocabulary a
path uses is the vocabulary a shape needs.

## Not yet specified

- What happens when the path addresses nothing.
- What happens when the document does not have the structure the path assumes.
- Whether a write can be previewed before it commits.
- Whether more than one document can be written in one invocation.
- What the command prints, and what it returns to the shell.

See [Design status](../design-status.md) for the open questions here.

## Related

- [arky write](../reference/cli/write.md) — the command reference.
- [Query grammar](../reference/query-grammar.md) — the path expression.
- [Change objects](../reference/change-objects.md) — the transactional model.
- [Changes and versions](../concepts/changes-and-versions.md) — the concept.
- [Query and search](query-and-search.md) — selecting the documents a change
  applies to.
- [A first session with arky](../tutorial/first-session.md) — the same steps
  front to back.
