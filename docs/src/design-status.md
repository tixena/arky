# Design status

This page collects what this book documents as provisional and every question
the design leaves open. Other pages link here rather than restating any of it.

## Provisional

Each item is documented as written and is expected to change.

- **The expression language.** Its operators, its quoting and escaping rules, its
  clause forms, chaining and projection, and its mutation verbs.
- **The path syntax.** How a path addresses a part of one document, and how it is
  written inside a pipeline as opposed to as a command-line option.
- **The command set and the spelling of every verb.** Which commands exist, what
  each one is called, and which arguments are positional and which are options.
- **The shape notation.** How a shape is written down, where it is kept, and how
  it binds to the documents it governs.
- **The change-object format.** What a change records, how it is identified, and
  where a chain of changes is stored.
- **The backend interface.** The operations a storage backend provides and which
  of them every backend has to implement.
- **The server tool surface.** The tool names, their parameters, their result
  shapes, and how a mutating call is previewed.
- **The validation-rule catalog.** Its rule names, levels, and messages, written
  against fixed requirements that declared shapes are to replace.

## Open questions

Each line is one question. No page in this book answers any of them.

### Scope and layering

- Whether the engine's library interface is designed first and the command line
  is a thin shell over it, or the command line is an interface in its own right.
- Whether the command line or the Model Context Protocol server is arky's
  primary surface.
- What it means mechanically for arky to be remargin's core: whether remargin
  depends on arky as a library, is rewritten on arky's model, or is absorbed
  into it.
- Which of remargin's parts — threaded comments, identities and signatures,
  permission realms, the protocol server, the command line, and the editor
  plugin — stay remargin's and which become arky's.
- Whether the system ends with one Model Context Protocol server or two.
- Whether the maintenance commands that serve a whole corpus — inbox
  classification and import from an external source — belong in arky, in a
  separate application built on the engine, or nowhere.
- Whether resource types carrying declared actions become a general platform for
  building small systems, and what an agent is permitted to do when it runs an
  action.
- How far one corpus's conventions harden into arky's defaults, and how much
  arky stays generic for corpora organized differently.

### The command set

- Whether the validation run is spelled `arky check` or `arky lint`.
- Whether a query takes its expression and its path as positional arguments, or
  its expression as a named option with no path.
- Whether repair is a command of its own or a flag on the validation run.
- Command spellings for move, rename, merge, and archive: one verb each, one verb
  with a mode, or no command at all because they are engine operations another
  layer calls.
- Whether the four global options are the program-wide contract every command
  honors.
- The spelling and shape of the command that starts the server: a subcommand, an
  option, or a second binary.
- What a folder retrieval, a markdown retrieval, and a binary retrieval each
  return.
- What a row of a listing carries, whether it recurses, whether it lists
  non-markdown files, and whether the ignore list applies to it.
- Whether a removal is a change that can be unapplied, and what happens to
  inbound links to what was removed.
- Whether a write is previewed before it commits, and whether the preview is an
  option on the command or a separate call returning the change.
- Whether reading a document's history is part of arky's surface, and what a
  reader asks for: a whole chain, one change, or a difference between two
  versions.
- Whether arky prescribes an order for bringing a new corpus under it.

### The query, path, and search languages

- Whether the document-set query, the search, and the within-document path share
  one grammar, or are two grammars joined at a defined seam.
- What a path expression can address: whether it selects by heading level,
  heading title, and index among children, and whether it also reaches list
  items, paragraphs, tables, code fences, remargin comment blocks, and
  frontmatter keys.
- Whether a bracketed index in a heading path selects among a heading's children
  or among its siblings.
- What a projection produces — markdown fragments, frontmatter values, file
  paths, or a mix chosen per clause — and which of those is the default.
- Which mutation verbs the language carries beyond appending, and whether one
  query-driven mass modification is a single change or one change per document.
- Whether search is a separate verb with its own clause syntax or a query whose
  default stage matches document bodies, and what the short options in a search
  invocation stand for.
- How a match inside a fenced code block or a remargin comment block is treated,
  and whether finding no match is a success or a failure.

### Shapes, schemas, and resource types

- Whether "shape" and "schema" name one object or two, and how one resource type
  extends another in those terms.
- How resource types declared by remargin reach arky's linter: passed at call
  time, kept in files arky reads, or held in arky's configuration.
- Whether the strict and loose modes belong to the shape or to the check.
- What the linter reports when a document fails a shape its owner declared.
- Which rule names, levels, and messages survive once every rule follows from a
  shape.
- Whether a folder's `allows` block survives, becomes a shape, or moves out of
  arky with the resource types.
- Whether inheritance stays a folder mechanism at all, and whether an `inherits`
  block in a folder's index file is how it is expressed.

### Changes, versions, and backends

- Whether a change object is a patch or an operation over the document tree,
  whether it is identified by a content hash or a sequence number, and whether
  the chain is arky's responsibility or the backend's.
- What happens when replay is not byte-identical, as when a document has been
  edited outside arky.
- Whether a backend stores documents, change objects, or both.
- Whether a backend declares its capabilities up front, or an unsupported
  operation is discovered only by attempting it.
- Which operations, if any, every backend must provide.
- How a backend is chosen and configured.

### Structure, links, and knowledge patterns

- Which reference spellings a move rewrites and which it preserves, how the
  operation reports what it rewrote, and what a dry run does.
- The collision policy for a merge that unions into an existing destination, and
  what becomes of an emptied source folder.
- Which frontmatter fields of a resource supply the strings a link run matches,
  and what the link target is when a resource is a folder rather than a file.
- Whether a link run has a dry run, an edit report format, and a guard against
  running over uncommitted changes.
- Whether arky builds a knowledge graph of typed edges between resources or
  only an inheritance tree.
- How a relation target written as a name resolves to a file.
- Whether the set of built-in relation types is closed, given that a proposal may
  name a type outside it.
- What becomes of an asset that no markdown file references.

### Validation and repair

- Whether a missing frontmatter block can be repaired without a person, given
  that the repair list omits it while the finding is marked repairable.
- Whether arky supports an adoption path for a corpus that has never been
  checked, including a way to record which existing findings are accepted.
- What the validation run returns when the failure level is `any` and the only
  findings are informational.
- Whether a rule filter takes a rule name, a rule category, or either, and what
  an ignore pattern may contain.
- Where custom rules fit: a rule table in the configuration, a hook that runs an
  external command, or an embedded scripting language.

### Output and exit codes

- What every command other than the validation run prints, and what a query
  prints.
- Whether one exit-code table serves every command or each verb defines its own,
  and whether "found nothing" is a distinct code.
- Which exit codes apply to commands other than the validation run, and whether a
  repair run has codes of its own.
- Whether the text format is a contract another program may parse.
- Whether a run prints a version banner, and what that line carries.
- Whether findings are grouped by message in the warning and information sections
  of the markdown report.

### Configuration

- Whether one configuration may cover more than one corpus.
- Whether the root table keeps its name and whether Obsidian's own folder stays
  in the default ignore list, now that arky requires no particular editor.
- How authentication against an external task source is handled.

### The Model Context Protocol server

- Which tools the server exposes, and whether the structural operations get tools
  before they get command spellings.
- What becomes of the commands that ask a person questions when they are reached
  as tools: drop the interactive step, split the operation in two, or expose no
  tool.
- A default result shape for every tool whose command output is unspecified,
  decided once rather than per tool.
- Which exit codes become protocol errors and which are results the caller reads.
- How configuration precedence reads for a server that keeps running: whether
  tool-call parameters stand where command-line options stand, whether the
  configuration file is re-read when it changes, and whether one server may serve
  more than one corpus.
- Whether the server exposes protocol resources or notifications as well as
  tools.
- Which tools carry a projection step, and whether a change object is handed back
  to the caller.

### Distribution and performance

- How arky is obtained — a published package, a release artifact, or a
  container image — and whether it ships a continuous-integration workflow, a
  pre-commit hook, and shell completions.
- The performance goals for the parser.
