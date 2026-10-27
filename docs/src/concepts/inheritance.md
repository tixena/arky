# Inheritance

A folder is not just a container: it declares things about everything below it. A
folder's `index.md` says which properties every file underneath must carry, and
which kinds of document are allowed to live there.

## The mechanism

1. Every folder has an `index.md`.
2. That file's frontmatter declares an `inherits:` block — properties that apply
   to every file in the folder and in all of its subfolders.
3. Child files must carry those properties, and the linter enforces it.
4. Child files may add properties of their own, and may never remove an inherited
   one.

Inherited properties accumulate down the tree. An area folder declares
`area: research`; the team folder below it inherits that and adds
`role: employee`; a person's file in that folder carries both, and may add an
email address, a job title, and a start date on top. There is no override: a child
that declares a different value for an inherited property fails validation.

## Folder rules

The same `index.md` carries an `allows:` block, which names the resource types
that may live in the folder and whether the folder may have subfolders at all. A
team folder allows only the `person` type and no subfolders; a projects folder
allows both `index` and `project` and permits subfolders, because a project can be
a single file or a folder with its own index.

An `index.md` therefore does three jobs at once: it describes the folder for a
reader, declares what belongs there, and declares what children inherit.

*How a folder rule naming remargin's resource types is expressed is not yet
specified* — see
[Shapes, schemas, and resource types](shapes-schemas-resource-types.md) for where
those types are owned.

## What arky does with it

Three validation rules act on the mechanism.

- `inheritance` reports a missing inherited property as an error, naming the
  property, the value expected, and the `index.md` that defined it. Automatic
  repair adds the missing property.
- `index-required` reports a folder that has no `index.md`. Repair creates one
  carrying the inherited properties.
- `allowed-types` reports a file whose type the folder's `allows` block does not
  permit, naming the allowed types and the file that declared them.

One command prints the chain. `arky trace` walks from the root down to one file
and shows, level by level, what each `index.md` contributes and where it came
from, ending with the properties the file must have and the properties it does
have.

```console
$ arky trace src/work/people/team/teammate.md
```

Configuration names which properties cascade at all, as a list holding `area`,
`client`, and `role`. See [Configuration](../reference/config.md) for that key,
[Validation rules](../reference/validation-rules.md) for the message each rule
prints, and [arky trace](../reference/cli/trace.md) for the command.

## The organization it serves

- **Folder-first.** The folder structure represents areas and hierarchy, and every
  file takes context from its location.
- **Every file is a resource**, with a type, properties in frontmatter, and flat
  keyword tags — properties carry structured values, tags exist for discovery.
- **One global inbox, per-area archives.** A single inbox folder catches
  unprocessed captures; each area keeps its own archive for finished content.

Archiving has one trigger and no other: a resource moves to an archive when its
`archived` property is set to true. Nothing is archived on age. The archive
mirrors the structure of the area it belongs to, and a rule reports any file that
claims to be archived while living outside an archive folder.

The inbox is the one place where the rules relax: inbox files may have no
frontmatter at all, the single exception to the expectation that every file
carries metadata. An informational rule reports inbox files still to process.

*Whether inheritance stays a folder mechanism under the schema-driven direction is
not yet specified:* the mechanism above is bound to folders and to a file named
`index.md`, while shapes and schemas are bound to documents and to parts of
documents. See [Design status](../design-status.md).
