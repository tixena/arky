# Define a shape

A *shape* is your description of what a markdown document, or one part of a
document, has to look like. arky's linter checks documents against shapes, and
the shapes belong to whoever writes them rather than to arky.

To declare what a `person` or a `project` document must carry, start from
[Enforce resource types](enforce-resource-types.md) instead: resource types are
declared by remargin, and arky enforces them.

## Step 1: decide what the shape governs

A shape governs either a whole markdown document or a sub-part of one — a
section, a fragment. A shape for a sub-part names that sub-part with a path
expression, so the reach of shapes follows the reach of the
[query grammar](../reference/query-grammar.md).

*Which parts of a document a path can address — heading levels, heading titles,
an index among children, list items, paragraphs, tables, code fences, remargin
comment blocks, and frontmatter keys — is not yet specified.*

## Step 2: choose strict or loose

The mode decides what happens to everything the shape did not mention.

- **Strict** rejects content that falls outside the declared shape. Choose it
  when a document must carry nothing but what the shape names.
- **Loose** lets that content through. Choose it when the shape is a floor rather
  than a boundary.

There is no third mode. arky never makes a document validate by trimming it: a
document is accepted as it stands or reported as failing.

*Which mode applies when a shape declares none, and whether the mode belongs to
the shape, to the document, or to the command that runs the check, are not yet
specified.*

## Step 3: bind the shape to the documents it governs

*Not yet specified.* Neither the notation a shape is written in nor the mechanism
that attaches one to a document or a folder is defined. Two nearby mechanisms are
not the answer:

- A folder's `index.md` declares which resource types are permitted inside it and
  which properties every file below it must carry. That mechanism is itself under
  reconsideration — see [Inheritance](../concepts/inheritance.md).
- The configuration file lists which rules are enabled. That turns rules on and
  off; it does not attach a shape to a document. See
  [Configuration](../reference/config.md).

See [Shape language](../reference/shape-language.md).

## Step 4: run the linter against the shape

```console
$ arky lint folder/
$ arky check src/work/
```

Both spellings name the validation run; every option is written for `check`. See
[Lint a vault](lint-a-vault.md) for the run itself and
[arky check](../reference/cli/check.md) for the option table.

*What the linter prints when a document fails a shape is not yet specified.* The
messages arky specifies today belong to the fixed rule catalog on
[Validation rules](../reference/validation-rules.md).

## Related

- [Shape language](../reference/shape-language.md) — the notation.
- [Shapes, schemas, and resource types](../concepts/shapes-schemas-resource-types.md)
  — the three words and how they relate.
- [Documents and schemas](../concepts/documents-and-schemas.md) — the document
  and the schema as objects rather than files.
- [Enforce resource types](enforce-resource-types.md) — the same machinery from
  remargin's side.
- [Lint a vault](lint-a-vault.md) — running the check and reading its report.
- [A first session with arky](../tutorial/first-session.md) — the same steps
  front to back.
