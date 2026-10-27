# Introduction

arky validates and maintains the structure of a markdown corpus. It checks
documents against shapes their owner declares, answers queries over frontmatter
and document bodies, and performs edits — writes, links, moves, merges — as
transactions that leave a corpus consistent.

arky is written in Rust, and it is an engine before it is a program. One engine
carries the behavior; three faces expose it.

- **A Rust library.** The engine ships as a workspace of library crates, with a
  thin command-line crate on top. Callers work on documents and schemas as
  objects, not on files.
- **A command line.** `arky` runs the same engine from a shell or a build
  pipeline. See the [command-line interface](reference/cli/index.md).
- **A Model Context Protocol server.** The same operations are exposed as tools an
  agent can call. See the
  [Model Context Protocol server](reference/mcp/index.md).

arky parses markdown itself — frontmatter and body alike — rather than composing
general-purpose parsers, because its queries, its shapes, and its edits all need
to address structure inside a document. Nothing about arky requires Obsidian.

## arky and remargin

remargin is a document-collaboration system: markdown documents carrying threaded
comments, participant identities and signatures, permission realms, a Model
Context Protocol server, a command line, and an Obsidian plugin. arky is
remargin's core, and remargin is arky's first consumer — which is why the engine
is designed as a library before it is designed as a command.

The product boundary between the two runs like this. arky holds the linter, the
schema engine, the document and schema abstractions, the transactional change
model, and version tracking over pluggable storage backends. remargin holds
resource types, the actions declared on each type, and the write path through
which every change becomes a transaction. See
[arky and remargin](concepts/arky-and-remargin.md).

## How this book is organized

- **Tutorial** — [A first session with arky](tutorial/first-session.md): one
  walk from an empty corpus to a change, front to back.
- **Guides** — how to accomplish one task, one guide per task.
- **Reference** — the command line, the Model Context Protocol server, and the
  languages and formats: shapes, the query grammar, validation rules, change
  objects, configuration, output formats, and exit codes.
- **Concepts** — the ideas the rest of the book leans on, including where the
  boundary between arky and remargin runs.
- **Status** — [Design status](design-status.md): what the design leaves
  provisional, and every question still open.

## Documentation ahead of implementation

arky is documented ahead of its implementation: the behavior on these pages is
specified, and no binary ships yet. Command examples therefore show the intended
command line, and expected output appears only where it is specified. A detail the
design has not fixed is marked *Not yet specified.* on the page where it belongs,
and the open questions are collected on the
[Design status](design-status.md) page.
