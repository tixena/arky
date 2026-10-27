# Shape language

A *shape* is a user-written description of what a markdown document, or a part of
one, must look like. arky's linter checks documents against shapes.

## What a shape governs

A shape governs a whole markdown document or a sub-part of one — a section, a
fragment. A shape can require frontmatter or not require it: whether a document
needs frontmatter at all, and which properties it must carry, follows from the
shape that governs it rather than from a fixed list.

The linter and the shapes it enforces belong to arky. The resource types a
shape is written against, and the actions declared on them, belong to remargin;
arky provides the means to enforce them. See
[Shapes, schemas, and resource types](../concepts/shapes-schemas-resource-types.md).

## Strict and loose

A shape has two modes.

- **Strict** rejects content that falls outside the declared shape.
- **Loose** lets that content through.

There is no stripping mode. A document is either accepted as it stands or
reported as failing; arky never silently removes the parts a shape does not
mention.

*Which mode applies when a shape declares none, and whether the mode belongs to
the shape, to the document, or to the command that runs the check, is not yet
specified.*

## Extension

A type can extend another type to add more to it. Types are defined per corpus:
one corpus declares its own set, another declares a different one.

*Whether an extending type may relax what it extends, how the two modes interact
across an extension, and what happens when two extended types disagree, are not
yet specified.*

## Not yet specified

- The notation a shape is written in.
- How a shape is bound to the documents it governs. A folder's `index.md` can
  declare which resource types are permitted inside it and which properties every
  file below it must carry, and the configuration file can enable and disable
  rules, but neither attaches a shape to a document. See
  [Configuration](config.md) and [Inheritance](../concepts/inheritance.md).
- The vocabulary a shape uses to name a sub-part of a document body, which is the
  same addressing question the expression language asks. See
  [Query grammar](query-grammar.md).
- Whether "shape" and "schema" name one thing or two.
- What the linter reports when a document fails a shape. The rules in the
  catalog each carry a fixed message; a shape violation has none. See
  [Validation rules](validation-rules.md).

See [Design status](../design-status.md) for the open questions around shapes.

## Related

- [Validation rules](validation-rules.md) — the rule catalog shapes replace.
- [Query grammar](query-grammar.md) — the addressing a sub-part shape depends on.
- [Documents and schemas](../concepts/documents-and-schemas.md) — the document
  and the schema as objects rather than files.
- [Define a shape](../guides/define-a-shape.md) — the how-to.
