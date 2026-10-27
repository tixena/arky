# arky and remargin

arky is the engine; remargin is the collaboration system built on it. remargin
holds markdown documents that carry threaded comments, participant identities and
signatures, permission realms, a Model Context Protocol (MCP) server, a command
line, and an Obsidian plugin. arky is its core, and remargin is arky's first
consumer, which is why arky's layers are designed as a library before they are
designed as a command.

This page states where the boundary between the two runs.

## What each side holds

arky holds the engine:

- the linter, with shapes the user specifies for a whole markdown document or for
  a sub-part of one;
- schemas with a strict and a loose mode, and no stripping mode;
- the markdown document and the schema as abstractions independent of any file on
  disk;
- a transactional change model in which every change is an object;
- version tracking abstracted over pluggable storage backends.

remargin holds the user-facing vocabulary and the write path:

- the resource types;
- the actions declared on each resource type — a `task` type, for example, whose
  actions are `defer`, `close`, and `assign` — performed through actions
  configured on the type;
- the write path through which every change becomes a transaction, with no write
  bypassing it.

The pattern behind the split: the concepts a user names are remargin's, and the
engine that enforces them is arky's. Each engine capability has its own page —
[Documents and schemas](documents-and-schemas.md),
[Shapes, schemas, and resource types](shapes-schemas-resource-types.md),
[Changes and versions](changes-and-versions.md), and
[Backends and capabilities](backends-and-capabilities.md).

## Resource types are remargin's

arky does not own a list of resource types, and neither does any part of its
configuration. What a `person`, a `project`, or a `task` is belongs to remargin,
because it is business logic. arky supplies the means to express and enforce
such a definition: types are declared per corpus rather than chosen from a fixed
list, and one type can extend another to add to it. arky's linter is the
enforcement engine behind them — see
[Shapes, schemas, and resource types](shapes-schemas-resource-types.md).

## Permissions

remargin grants view or edit access to files and to parts of markdown documents,
so a reader is shown only the portion of a document they hold access to. Today's
model is path-level: restrict a path, unprotect one, deny particular operations,
and trusted roots whose own restrictions are respected recursively.

arky's contribution is addressability. A part of a document has to be
addressable before access can be granted to that part, so what a path expression
can address decides what a permission can attach to. See the
[query grammar](../reference/query-grammar.md) for path expressions.

*How arky exposes addressed parts to a permission check is not yet specified.*

## Structural operations

Structural operations on a corpus — move, rename, merge, archive — are arky's,
and they are integral transactions that keep links valid by construction rather
than validating and repairing afterwards. arky holds a corpus as a structured
graph with typed resources and links as first-class edges, so a move rewrites
every inbound reference as part of the same transaction. See
[Changes and versions](changes-and-versions.md) for the model and
[Structural operations](../reference/cli/structural-operations.md) for the
operations themselves.

## Related

- [Introduction](../introduction.md) — what arky is.
- [Design status](../design-status.md) — the open questions about this boundary,
  including which of remargin's subsystems stay remargin's.
