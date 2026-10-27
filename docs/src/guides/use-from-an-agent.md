# Use arky from an agent

An agent — a program driven by a language model — does not open a terminal. It
calls tools. arky exposes a Model Context Protocol (MCP) server for exactly
that: the Model Context Protocol is the interface through which a model-driven
program discovers the tools a system offers and calls them.

## Step 1: know what you are calling

arky has three faces over one engine: a Rust library, a command line, and the
Model Context Protocol server. An agent uses the third. The command line and the
server are two ways into the same engine, so every capability in this book's
reference section is the subject matter of both.

*How the server is started, how it is addressed, and what it is called are not
yet specified.* See
[Model Context Protocol server](../reference/mcp/index.md).

## Step 2: expect these conventions

arky's server mirrors remargin's conventions.

- **Flat, verb-named tools.** One tool per operation, named for the operation —
  reading a file, listing a directory, searching text, querying, writing,
  editing, moving, copying, removing — rather than one tool with a mode argument.
- **Compact payloads with the column names stated once.** A result carrying many
  similar rows names the columns in one list and then emits each row as a
  positional array, so a field name is transmitted once rather than once per row.
- **Paging that never hides a truncation.** A caller asks for a page with a limit
  and an offset, and the result carries the exact total regardless of page size,
  plus an explicit marker when a page was clamped.
- **A preview step before anything mutates.** A separate call takes the operation
  a caller intends and reports what it would do — what would change, what would
  be rejected, and whether the operation would do nothing at all — without
  touching disk.
- **An atomic batch.** Several writes to one document go in one call that applies
  as a whole or not at all.
- **An identity call.** The server answers who it is acting as, so an agent can
  find out rather than assume.
- **Binary content described as well as delivered.** Non-text content comes back
  with its type and size alongside the bytes.

*Every tool name, parameter, and result shape is not yet specified* — see
[Model Context Protocol tools](../reference/mcp/tools.md).

## Step 3: read a vault

The read-side operations are listing the contents of a vault, retrieving a
resource by path — a folder, a markdown file, an image, or a document in Portable
Document Format — selecting documents by an expression over their frontmatter,
and matching a pattern against bodies and frontmatter. See
[arky ls](../reference/cli/ls.md), [arky get](../reference/cli/get.md), and
[Query and search](query-and-search.md).

Retrieving a document as parsed data rather than as text is what makes a vault
usable as context for a model without the model parsing markdown itself. *What a
retrieval returns for a markdown file — the raw bytes or the parsed document — is
not yet specified.*

## Step 4: change a vault

Every write is a transaction that produces a change object, which can be replayed
to a byte-identical result and unapplied in reverse. An agent's write is no
exception. See [Change objects](../reference/change-objects.md) and
[Write through paths](write-through-paths.md).

What an agent may see and change is bounded by remargin's permission system,
which grants view or edit access to files and to parts of markdown documents, so
that only the accessible portion is shown. arky's contribution is that a part
of a document has to be addressable before access to it can be granted or
refused. *How arky serves that mechanically is not yet specified.*

## What else a model can consume

[arky export](../reference/cli/export.md) serializes a vault for a language
model to read: for each file, its frontmatter, the categorized observations found
in its body, the typed relations to other resources, and a graph of nodes and
edges built from those relations, all in JavaScript Object Notation (JSON). See
also [Knowledge patterns](../concepts/knowledge-patterns.md).

[arky inbox](../reference/cli/inbox.md) runs the other way round: arky asks a
model to classify unfiled captures.

## Not yet specified

- Whether the tool set mirrors the command-line verbs one for one, or exposes the
  engine's operations directly with the command line arranging them into verbs.
- Whether the preview returns the change object a write would produce or a
  summary of it.
- Whether the system ends up with one Model Context Protocol server or two.
- Whether the command line or the server is arky's primary interface.

See [Design status](../design-status.md) for the open questions here.

## Related

- [Model Context Protocol server](../reference/mcp/index.md) — the server
  reference.
- [Model Context Protocol tools](../reference/mcp/tools.md) — the tools it
  exposes.
- [arky and remargin](../concepts/arky-and-remargin.md) — the boundary
  between the two systems.
- [Query and search](query-and-search.md) — the read operations an agent calls.
