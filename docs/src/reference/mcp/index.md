# Model Context Protocol server

arky exposes a server that speaks the Model Context Protocol: the protocol
through which a language-model client calls a program's operations as structured
tool calls, instead of running the program in a shell and reading the text it
prints.

## Starting the server

```console
$ arky mcp
```

Starts the server. It speaks the protocol over standard input and standard
output rather than over a network port, so a client is configured with the
program's name and that one argument. *Client configuration is not yet
specified.*

The server resolves its configuration and its root once, when it starts, from
the directory it was started in.

## Tools

The server exposes tools and nothing else: one tool per command, named as the
command is named, taking the command's arguments and options as its parameters.
Names are short and carry no prefix, because a server already scopes the tools it
exposes — the tool is `get`, not `arky_get` — and a name that needs more than
one word joins them with an underscore. A parameter takes its option's name with
the dashes replaced by underscores.

The [tools](tools.md) page lists every tool with its parameters and its result.

## Payload conventions

- A result carrying repeated rows names its fields once, in a list of column
  names, and makes every row positional. What the server sends costs the client
  tokens, so a field name is written once rather than once per row.
- A bounded page reports the exact total alongside the page size it was clamped
  to, so a partial answer cannot be read as a complete one.
- An operation that mutates can be projected before it runs. The projection
  reports what would change and what would be refused and why, and touches
  nothing.
- Errors carry the path they concern and the reason. A failure inside a batch
  names the operation that failed.

The validation report is the one result shape the design fixes; it is shown in
full on the [Output formats](../output-formats.md) page. Three commands have
output specified for a terminal or for a file rather than for a program — the
inheritance chain of [arky trace](../cli/trace.md), the proposal screen of
[arky inbox](../cli/inbox.md), and the document written by
[arky export](../cli/export.md) — and the [tools](tools.md) page says so
rather than restating a drawing as a result.

## Configuration

arky reads a configuration file named `arky.toml` at the repository root.
Settings take precedence in this order, highest first: options given on the
command line, then environment variables, then the file, then built-in defaults.
`ARKY_CONFIG` names the configuration file and `ARKY_ROOT` names a corpus
root. What the file holds is documented on the [Configuration](../config.md)
page.

## Errors and exit codes

arky returns 0 for success with no issues, 1 for validation errors found, 2 for
warnings only when the run was set to fail on warnings, 3 for a configuration
error, 4 for a file system error, and 5 for a language-model provider error. The
[Exit codes](../exit-codes.md) page carries the table in full. A non-zero exit
becomes a protocol error response.

Codes 1 and 2 are the outcome of a validation run that did what it was asked to
do, and a client reads an error response and a result differently.

## Not yet specified

- Whether the parameters of a tool call stand where command-line options stand in
  the precedence order, so that a call may name a root the server was not started
  in.
- Whether the configuration file is re-read while the server runs.
- Whether one server serves more than one corpus.
- Which exit codes become protocol error responses and which are returned as
  results.
- Whether the server enforces the permissions remargin declares over files and
  over parts of documents.
- Whether the server exposes protocol resources or notifications as well as
  tools.

See [Design status](../../design-status.md) for the open questions around this
server.

## Related

- [Model Context Protocol tools](tools.md) — one entry per tool.
- [Command-line interface](../cli/index.md) — the surface these tools mirror.
- [Configuration](../config.md) — the `arky.toml` file and the precedence
  order.
- [Exit codes](../exit-codes.md) — the code table.
- [Output formats](../output-formats.md) — the validation report.
- [Use arky from an agent](../../guides/use-from-an-agent.md) — the how-to.
- [arky and remargin](../../concepts/arky-and-remargin.md) — the boundary
  between the two.
