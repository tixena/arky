# Enforce resource types

A *resource type* says what a document is: a person, a project, a task, a
contract. Types are declared by remargin; arky checks that documents satisfy
them. This guide walks that division in the order the work happens.

Types are declared per corpus rather than chosen from a fixed list, and one type
can extend another to add to it. For the three words and how they relate, see
[Shapes, schemas, and resource types](../concepts/shapes-schemas-resource-types.md).

## Step 1: declare the type on remargin's side

A type carries the properties a document of that type must have, and it may carry
*actions* — a `task` type, for example, declares `defer`, `close`, and `assign`.
arky enforces types; it does not execute actions. Declaring a type is
remargin's work and this book does not document it.

## Step 2: express what the type requires as a shape

*Not yet specified.* arky enforces a type by checking documents against shapes,
and neither the notation a shape is written in nor the handover that turns a
declared type into a shape arky can check is defined. See
[Define a shape](define-a-shape.md) and
[Shape language](../reference/shape-language.md).

The mode is fixed: a schema is strict, rejecting content the shape does not
declare, or loose, letting it through. There is no mode that accepts a document
by trimming it. So a type can be enforced as a floor or as a boundary, once there
is a way to write it down.

## Step 3: run the linter

```console
$ arky lint folder/
$ arky check src/work/
```

See [Lint a vault](lint-a-vault.md) for the run and its options, and
[arky check](../reference/cli/check.md) for the option table.

## Step 4: read the type findings

Three rules in the current catalog concern types.

| Rule | What it reports |
|---|---|
| `type-required` | A file with no `type` property. An error, and one the repair run leaves alone because a person has to decide the type. |
| `type-valid` | A `type` value that is not one of the defined types. An error whose message lists the valid types. |
| `allowed-types` | A file whose type is not in the `allows.types` list declared by its folder's index file. An error naming the allowed types and the file that declared them. |

That catalog is fixed and predates shapes: it enumerates a built-in list of types
rather than reading the declared ones. Each rule becomes a consequence of what a
shape says once shapes exist. See
[Validation rules](../reference/validation-rules.md).

## Step 5: extend a type

A type can extend another to add to it.

*Whether an extending type may relax what it extends, how the strict and loose
modes interact across an extension, and what happens when two extended types
disagree, are not yet specified.*

## Not yet specified

- Whether one resource type is one schema.
- How a folder's `allows:` block names a type declared by remargin — see
  [Inheritance](../concepts/inheritance.md).
- What a type violation reads like once shapes replace the fixed catalog.

See [Design status](../design-status.md) for the open questions here.

## Related

- [Shapes, schemas, and resource types](../concepts/shapes-schemas-resource-types.md)
  — the three words and the boundary between them.
- [arky and remargin](../concepts/arky-and-remargin.md) — where the line
  between the two runs.
- [Define a shape](define-a-shape.md) — the declaration side of the same work.
- [Lint a vault](lint-a-vault.md) — running the check that enforces the types.
- [Validation rules](../reference/validation-rules.md) — the rules in full.
