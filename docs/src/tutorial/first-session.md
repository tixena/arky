# A first session with arky

This is one pass through arky, from an empty vault to a change: set a vault up,
describe what its documents must look like, check them, read the report, select
documents, search their text, write into one of them, link the mentions inside
them, and see what the change amounts to.

Three of the eight steps end without a command line, because the piece they
describe is not yet specified. Output appears only where it is specified, which
in this session means step 3 alone.

## Step 1: set a vault up

```console
$ arky init .
```

`arky init [PATH]` prepares a vault. `--config` writes `arky.toml` at the
root if it is missing — the file whose every key is listed on
[Configuration](../reference/config.md). `--indexes` creates an `index.md` in
every folder that has none, carrying the properties that folder inherits from the
folders above it. `--all` does both.

The inheritance mechanism the second option serves is itself under
reconsideration — see [Inheritance](../concepts/inheritance.md).

*What `arky init` does when none of the three options is given, what the file
it writes contains, whether an existing configuration file is left alone or
refreshed, what the path defaults to, and what the command prints, are not yet
specified.* See [arky init](../reference/cli/init.md).

## Step 2: describe what the documents must look like

Everything after this step leans on it. A *shape* is your description of what a
markdown document, or one part of a document, has to look like, and arky's
linter checks documents against shapes. A schema carries a shape in one of two
modes: strict rejects content the shape does not declare, loose lets it through.
There is no mode that accepts a document by trimming it.

*How a shape is written down, and how it reaches the documents it governs, are
not yet specified*, so this step has no command line, and the steps below check
documents against the fixed rule catalog rather than against anything you wrote.
See [Shape language](../reference/shape-language.md) and
[Define a shape](../guides/define-a-shape.md).

## Step 3: check a vault and read the report

```console
$ arky lint folder/
```

A run walks the root named in the configuration file, or the path it is given,
skips the folders in the ignore list, applies the enabled rules, and reports
every finding at one of three levels. It never modifies a file.

The same run is also spelled `arky check`, which is the spelling every option
is written for. Its report:

```console
$ arky check
Scanning src/...
Found 127 markdown files, 45 assets

ERRORS (3):
  ✗ src/notes/random.md
    Missing frontmatter

  ✗ src/work/new/
    Missing index.md

  ✗ src/work/projects/old.md
    archived=true but not in archive/

WARNINGS (2):
  ⚠ src/product/notes/planning.md
    Contains 5 embedded task items

  ⚠ src/work/assets/screenshot.png
    Not referenced by any file

INFO (4):
  ○ src/inbox/note1.md - Needs processing
  ○ src/inbox/note2.md - Needs processing
  ○ src/inbox/note3.md - Needs processing
  ○ src/inbox/note4.md - Needs processing

─────────────────────────────────────
Summary: 3 errors, 2 warnings, 4 info
         122 files OK
```

**Errors** are documents that break a rule — no frontmatter, frontmatter that
does not parse, a missing or unrecognized `type`, a missing `created` date, a
missing inherited property, a folder with no index file, a file whose type its
folder does not allow, and a file marked as archived that sits outside an archive
folder. **Warnings** are the two findings that ask for a judgment: an asset that
no markdown file references, and a note holding several independent checkbox
items. **Information** is the inbox: files that still have no frontmatter and are
waiting to be classified.

Four options shape the run. `--format` selects plain text, JavaScript Object
Notation (JSON), or markdown; `--fail-on` takes `error`, `warning`, or `any` and
decides the process exit status and nothing else; `--rule` restricts the run to
named rules and may be given more than once; `--ignore` adds patterns to skip on
top of the configuration file's list. The other two formats of this report are on
[Output formats](../reference/output-formats.md), and the exit codes are on
[Exit codes](../reference/exit-codes.md).

These rules are a fixed catalog that predates shapes; each becomes a consequence
of what a shape says once shapes exist. See
[Validation rules](../reference/validation-rules.md) and
[Lint a vault](../guides/lint-a-vault.md).

## Step 4: select documents by their frontmatter

```console
$ arky query 'tags contains "rule"' .grit/patterns/
$ arky query --exp='type=issue'
```

A query walks a folder, parses each file's frontmatter, and emits the files whose
frontmatter satisfies the expression. The expression language is like `jq`, the
command-line processor for JSON in which small filters join into pipelines:
expressions chain, and a query can name what it emits instead of always emitting
whole documents.

*The grammar — its operators, its quoting, its escaping, how an expression is
written as a command-line argument, and which of the two spellings above is the
command line — is not yet specified*, and neither is what a query prints or what
it returns to the shell when nothing matched. See
[Query grammar](../reference/query-grammar.md) and
[arky query](../reference/cli/query.md).

## Step 5: search the text of the documents

```console
$ arky search folder/ -gi "[0-9]?[a-z]*"
```

A search runs over document bodies, over frontmatter when frontmatter is
specified, and over both when clauses are given for both. Matching frontmatter
and body separately is one reason arky parses markdown itself.

*The syntax of a frontmatter clause and of a body clause, the meaning of the
short flags, and the dialect of regular expression the pattern is written in, are
not yet specified.* See [arky search](../reference/cli/search.md).

## Step 6: write into part of one document

```console
$ arky write folder/file.md --path='# ##["Specs"][2]' --append --from-body='path-to-file.txt'
```

The command names one document, addresses a part of it with `--path` — here the
second child block under the level-two heading titled "Specs" — appends rather
than replaces, and takes the content to append from the file named by
`--from-body`. Appending is also the verb of a bulk change driven by a query:
append a section carrying the current date to each element the query returns.

Reaching into a document is the point rather than an accident of this example.
remargin grants view or edit access to files and to parts of markdown documents,
and whatever a path expression can address is what such a permission can be
attached to — and what a shape can govern.

*The path grammar, the rest of the mutation vocabulary, whether content may be
given inline or read from standard input, what happens when the path addresses
nothing or the document lacks the structure the path assumes, whether a write can
be previewed, and what the command prints, are not yet specified.* See
[arky write](../reference/cli/write.md) and
[Write through paths](../guides/write-through-paths.md).

## Step 7: turn mentions into links

```console
$ arky link folder/
```

Every collision-free mention of a resource's name or alias, in the markdown files
under `folder/`, is wrapped in a markdown link whose target is a relative path to
that resource. The matched text is preserved exactly, the longest match wins on
word boundaries, a string that two resources both claim is never linked, and
frontmatter, fenced code blocks, inline code spans, existing links and
wiki-links, and raw web addresses are never rewritten.

What counts as a resource is declared by remargin, so linking is not restricted
to one kind of document.

*Which frontmatter fields supply a resource's matchable strings, how a target is
chosen when the resource is a folder, and what a report of the run looks like,
are not yet specified.* See [arky link](../reference/cli/link.md) and
[Link resources](../guides/link-resources.md).

## Step 8: see what the change amounts to

The session has now written into documents twice — once through a path and once
by linking. This step has no command line.

Every change produces a change object, on the model git uses: a base resource
plus a chain of changes applied in sequence gives a byte-identical latest
version, and unapplying the same chain in reverse returns the original. The
guarantee is reachable because the content is linted markdown with a known shape;
a change to arbitrary text can only be recorded as a difference between two
texts, while a change to a document whose structure is known can be recorded in
terms of that structure. That is why step 2 sits where it does.

The link run in step 7 is that model in miniature. It is additive, so stripping
every link it injected reproduces the file byte for byte; it is idempotent,
because text already inside a link is a region it skips; and its inverse strips
only a link the run would have produced.

*What a change object contains, how it is serialized, how one is named, where the
chain is kept, whether one query-driven change across many documents is one
object or one per document, and what arky does when a replay is not
byte-identical because a document was edited outside arky, are not yet
specified.* No command reads a document's history either. See
[Change objects](../reference/change-objects.md),
[Changes and versions](../concepts/changes-and-versions.md), and
[Design status](../design-status.md).

## Where to go next

Each step above has a guide that takes it further.

- [Define a shape](../guides/define-a-shape.md) — the four decisions declaring a
  shape needs.
- [Lint a vault](../guides/lint-a-vault.md) — the validation run in full.
- [Query and search](../guides/query-and-search.md) — choosing between the two
  reading verbs.
- [Write through paths](../guides/write-through-paths.md) — addressing part of a
  document, and what the write commits.
- [Link resources](../guides/link-resources.md) — running and undoing a link run.
- [Enforce resource types](../guides/enforce-resource-types.md) — the same
  machinery from remargin's side.
- [Move and merge with link integrity](../guides/move-and-merge-with-links.md) —
  moving a folder without breaking the references into it.
- [Choose a backend](../guides/choose-a-backend.md) — where a vault and its
  versions are stored.
- [Use arky from an agent](../guides/use-from-an-agent.md) — the Model Context
  Protocol (MCP) server.
- [Run arky in continuous integration](../guides/run-in-ci.md) — the same check
  as a build step.
