# Model Context Protocol tools

The [Model Context Protocol server](index.md) exposes one tool per command. This
page lists them, with the command each mirrors, its parameters, and what it
returns. Read the server page first for the transport, the payload conventions,
and the configuration the server resolves when it starts.

Tool names are the command names, short and without a prefix. A parameter takes
its option's name with the dashes replaced by underscores; types are the ordinary
ones a tool schema carries — a string, a boolean, a number, an array of strings.

| Tool | Mirrors | What it does |
|---|---|---|
| [`ls`](#ls) | [`arky ls`](../cli/ls.md) | Lists the contents of a corpus. |
| [`get`](#get) | [`arky get`](../cli/get.md) | Retrieves a resource by its path. |
| [`query`](#query) | [`arky query`](../cli/query.md) | Selects documents by an expression over their frontmatter. |
| [`search`](#search) | [`arky search`](../cli/search.md) | Matches a pattern against documents. |
| [`write`](#write) | [`arky write`](../cli/write.md) | Writes into the part of one document a path expression addresses. |
| [`rm`](#rm) | [`arky rm`](../cli/rm.md) | Removes a file or a folder. |
| [`link`](#link) | [`arky link`](../cli/link.md) | Turns mentions of a resource into links to that resource. |
| [`lint`](#lint) | [`arky check`](../cli/check.md) | Validates a corpus and reports what it finds. |
| [`init`](#init) | [`arky init`](../cli/init.md) | Sets up a corpus. |
| [`fix`](#fix) | [`arky fix`](../cli/fix.md) | Repairs what can be repaired without a person deciding. |
| [`inbox`](#inbox) | [`arky inbox`](../cli/inbox.md) | Classifies unfiled captures and proposes where each belongs. |
| [`trace`](#trace) | [`arky trace`](../cli/trace.md) | Reports the chain of inherited properties for one file. |
| [`export`](#export) | [`arky export`](../cli/export.md) | Serializes a corpus for another program to read. |
| [`import`](#import) | [`arky import`](../cli/import.md) | Pulls items in from an external source. |

## ls

Mirrors [`arky ls`](../cli/ls.md). Lists the contents of a corpus.

| Parameter | Type | Description |
|---|---|---|
| `path` | string | The folder to list. |

**Returns.** *Not yet specified.*

## get

Mirrors [`arky get`](../cli/get.md). Retrieves a resource by its path: a
folder, a markdown file, an image, or a document in Portable Document Format.

| Parameter | Type | Description |
|---|---|---|
| `path` | string, required | The resource to retrieve. |

**Returns.** *Not yet specified.* A tool call returns structured data rather than
a byte stream, so a folder, a markdown document, and a binary file do not come
back in one shape.

## query

Mirrors [`arky query`](../cli/query.md). Selects documents by an expression
over their frontmatter, in a language that also drives modification.

| Parameter | Type | Description |
|---|---|---|
| `exp` | string, required | The query expression. |
| `path` | string | The documents to run it over. |

**Returns.** *Not yet specified.* The language and what a projection produces are
described under [Query grammar](../query-grammar.md).

## search

Mirrors [`arky search`](../cli/search.md). Matches a pattern against documents
in a corpus.

| Parameter | Type | Description |
|---|---|---|
| `path` | string | The documents to search. |
| `pattern` | string, required | The pattern to match. |

Search runs over document bodies, over frontmatter when frontmatter is
specified, and over both when clauses are given for both. *The parameter that
selects that scope is not yet specified.*

**Returns.** *Not yet specified.*

## write

Mirrors [`arky write`](../cli/write.md). Writes into the part of one document
that a path expression addresses. It is the only tool that operates on a sub-part
of a document.

| Parameter | Type | Description |
|---|---|---|
| `file` | string, required | The document to write into. |
| `path` | string, required | The expression addressing the part of it to write. |
| `append` | boolean | Append rather than replace. |
| `from_body` | string | The file the new content is taken from. |

The document is named `file` and `path` carries the expression, because both are
paths and only one of them addresses a file.

Every write is a transaction that produces a
[change object](../change-objects.md). *Whether the tool returns that object, a
summary of it, or nothing is not yet specified.*

## rm

Mirrors [`arky rm`](../cli/rm.md). Removes a file or a folder from a corpus.

| Parameter | Type | Description |
|---|---|---|
| `path` | string, required | The file or folder to remove. |

**Returns.** *Not yet specified.*

## link

Mirrors [`arky link`](../cli/link.md). Wraps plain-text mentions of known
resources in links to those resources.

| Parameter | Type | Description |
|---|---|---|
| `path` | string, required | The folder or file to scan. |

**Returns.** Each edit — file, byte range, matched text, and link target —
together with the strings excluded for being claimed by more than one resource.
*The report format is not yet specified.*

## lint

Mirrors [`arky check`](../cli/check.md). Validates a corpus, or one path within
it, and reports what it finds.

| Parameter | Type | Description |
|---|---|---|
| `path` | string | The path to validate. Defaults to the root in the configuration file. |
| `format` | string, one of `text`, `json`, `markdown` | The report format. Default `text`. |
| `fail_on` | string, one of `error`, `warning`, `any` | The level at which the run fails. Default `error`. |
| `rule` | array of strings | Run only the named rules or rule categories. |
| `ignore` | array of strings | Patterns to skip, beyond the ignore list in the configuration file. |

**Returns.** The validation report: a corpus root, the counts of files and assets
scanned, the findings split into errors, warnings, and information — each
carrying its rule, its path, its message, and whether it can be repaired — and a
summary of the counts. It is shown in full on the
[Output formats](../output-formats.md) page. A tool call returns structured data,
so the JavaScript Object Notation form is the result and the other two formats
are renderings of it.

What a run checks is listed under [Validation rules](../validation-rules.md), and
what a user-declared shape adds under [Shape language](../shape-language.md).

## init

Mirrors [`arky init`](../cli/init.md). Sets up a corpus: writes the
configuration file and creates the folder index files that are missing.

| Parameter | Type | Description |
|---|---|---|
| `path` | string | The corpus to set up. |
| `config` | boolean | Create `arky.toml` if it is missing. |
| `indexes` | boolean | Create the missing `index.md` files. |
| `all` | boolean | Do both. |

**Returns.** *Not yet specified.* *What a run does when none of the three
booleans is given is not yet specified either.*

## fix

Mirrors [`arky fix`](../cli/fix.md). Repairs the findings a validation run
reports that can be repaired without a person deciding anything.

| Parameter | Type | Description |
|---|---|---|
| `path` | string | The path to repair. |
| `dry_run` | boolean | Report what would be repaired without changing any file. |
| `rule` | array of strings | Repair only the findings of the named rules. |
| `interactive` | boolean | Ask before each repair. |

**Returns.** *Not yet specified.*

A tool call is not a conversation: the client sends parameters and the server
answers once. *How `interactive` maps onto a tool call is not yet specified.*

## inbox

Mirrors [`arky inbox`](../cli/inbox.md). Reads the captures dropped into a
corpus without being filed, classifies each one with a language model, and
proposes where each belongs.

| Parameter | Type | Description |
|---|---|---|
| `path` | string | The inbox folder. Defaults to the path in the configuration file. |
| `dry_run` | boolean | Report the proposals without applying them. |
| `auto` | boolean | Apply the proposals without asking. |
| `provider` | string, one of `anthropic`, `openai` | The language-model provider. Default from the configuration file. |
| `model` | string | The model. Default from the configuration file. |

The credential is read from the environment, from `ANTHROPIC_API_KEY` or from
`OPENAI_API_KEY`. *Which is consulted when both are set, and how either reaches a
server the client started, is not yet specified.*

**Returns.** *Not yet specified.* The command's proposal screen, shown on
[arky inbox](../cli/inbox.md), waits for a keystroke and has no tool-call form.

## trace

Mirrors [`arky trace`](../cli/trace.md). Reports the chain of inherited
properties for one file: which folder index files above it declare properties,
what those properties are, and whether the file carries them.

| Parameter | Type | Description |
|---|---|---|
| `file` | string, required | The file whose chain is reported. |

**Returns.** The chain itself. The command draws it as an indented tree, shown on
[arky trace](../cli/trace.md); *the form a tool call returns it in is not yet
specified.*

## export

Mirrors [`arky export`](../cli/export.md). Serializes a corpus so another
program can read it.

| Parameter | Type | Description |
|---|---|---|
| `path` | string | The corpus to serialize. |
| `include_content` | boolean | Include the content of each file. The document becomes large. |
| `only_metadata` | boolean | Export frontmatter only, without the structure. |

**Returns.** The export document: a list of files, each with its path, its
frontmatter, its observations — each carrying a category, its content, and its
tags — and its relations, each carrying a type and its targets; and beside that
list a graph in which each resource is a node and each relation an edge carrying
its type. It is shown in full on [arky export](../cli/export.md).

*What happens when `include_content` and `only_metadata` are both given is not
yet specified.*

## import

Mirrors [`arky import`](../cli/import.md). Pulls items in from a source outside
a corpus and writes them into it.

| Parameter | Type | Description |
|---|---|---|
| `source` | string, required | The source to import from. `google-tasks` is the one source named. |
| `list` | string | A specific task list, named by its identifier. |
| `dry_run` | boolean | Report what would be imported without writing. |
| `target` | string | Where imported items are written. Default: the inbox folder. |

**Returns.** *Not yet specified.*

*How the tool authenticates against an external source is not yet specified: a
tool call has nowhere to put a browser sign-in.*

## Commands with no tool

- **`arky help`.** A client asks the server for its tool list and receives the
  tools with their parameters, so the help command has no counterpart here.
- **The command that starts the server.** A server does not expose the command
  that starts it.
- **Structural operations.** Move, rename, archive, and merge are arky's
  territory and have no command spelling, so no tool name follows from mirroring.
  *Their tools are not yet specified.* See
  [Structural operations](../cli/structural-operations.md).

## Not yet specified

- Whether a `path` parameter defaults to the root the server resolved at start,
  may be repeated, or may be a glob.
- Whether the validation command is spelled `check` or `lint`; the tool is named
  `lint`.
- What a binary resource comes back as, and whether a tool reports a file's size
  before anything fetches it.
- The result shape of every tool but `lint`.
- Which tools carry a projection step, and whether a change object is handed back
  to the caller.

See [Design status](../../design-status.md) for the open questions around these
tools.

## Related

- [Model Context Protocol server](index.md) — transport, payloads, and errors.
- [Command-line interface](../cli/index.md) — the command table these tools
  mirror.
- [Output formats](../output-formats.md) — the validation report in all three
  forms.
- [Exit codes](../exit-codes.md) — the code table.
- [Query grammar](../query-grammar.md) — the expression language behind `query`,
  `search`, and the path given to `write`.
- [Use arky from an agent](../../guides/use-from-an-agent.md) — the how-to.
