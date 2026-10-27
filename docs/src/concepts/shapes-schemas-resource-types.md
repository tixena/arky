# Shapes, schemas, and resource types

Three words name three different things.

- A **shape** is what a user declares a document, or a part of a document, must
  look like.
- A **schema** is the object arky's engine holds and enforces, carrying a mode
  that decides how it treats content it did not declare.
- A **resource type** is remargin's idea of what a document *is* — a person, a
  project, a task. arky's linter enforces resource types without owning them.

## Shape

Shapes are declared by the user, not built into arky, and a shape applies either
to a whole markdown document or to a sub-part of one — a section, a fragment — not
only to a document as a unit. Whatever a path expression can address is what a
shape can be attached to, so the reach of shapes follows the reach of the
[query grammar](../reference/query-grammar.md).

*How a shape is written — its syntax, where it lives, and how it is bound to the
documents it governs — is not yet specified.* See
[Shape language](../reference/shape-language.md).

## Schema

A schema has a mode, and arky offers two of them:

- **Strict** rejects content outside the declared shape.
- **Loose** lets undeclared content through.

There is no stripping mode. arky never accepts content and silently trims it to
make a document validate.

Schemas cover both frontmatter and document structure, and what a document is
required to carry follows from the governing schema rather than from a fixed rule
— see [Documents and schemas](documents-and-schemas.md).

The schema engine is arky's. The schemas themselves are written against
remargin's vocabulary.

## Resource type

A resource type says what a document is, and carries the required and optional
properties that go with it — a `person` may carry a role, an email address, a
phone number, a company, and a title; a `project` may carry a client, a start
date, an end date, and a repository. A vault's set might read `index`, `person`,
`organization`, `project`, `task`, `proposal`, `contract`, `note`, `snippet`,
`workaround`, `walkthrough`, `research`, `reference`, `log`, and `event`, with
`asset` for non-markdown files that are referenced rather than typed.

No such list belongs to arky. Types are declared per corpus, one type can extend
another to add to it, and ownership of the vocabulary sits with remargin — see
[arky and remargin](arky-and-remargin.md).

Resource types carry actions, which are remargin's as well: a `task` type, for
example, declares `defer`, `close`, and `assign`, so a user interface can render
any resource together with its actions and an agent can execute them.

## How the three relate

remargin declares a resource type; the type is expressed as a schema, in a strict
or a loose mode; the schema is built from shapes the user specifies for a document
or for parts of one; arky's linter checks documents against those shapes and
reports what does not satisfy them.

*Whether "shape" and "schema" name one object or two, and how one type extends
another in terms of schemas and shapes, are not yet specified.* This book uses the
two words separately and asserts no relation between them; see
[Design status](../design-status.md).

## What the linter checks

arky's validation rules cover frontmatter, individual properties, document
structure, archive placement, assets nothing references, tasks embedded in notes,
and unprocessed captures. Each rule has a message and a severity, and some can be
repaired automatically. The full catalog is on the
[Validation rules](../reference/validation-rules.md) page.

## Related

- [Define a shape](../guides/define-a-shape.md) — the how-to.
- [Enforce resource types](../guides/enforce-resource-types.md) — putting types to
  work.
- [Lint a vault](../guides/lint-a-vault.md) — running the linter.
