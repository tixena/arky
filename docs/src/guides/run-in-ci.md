# Run arky in continuous integration

Continuous integration is the practice of running checks automatically on every
change to a repository. arky is built for it: a validation run prints a
machine-readable report and returns a process exit code that decides whether the
build fails.

## Step 1: run the validation and capture the report

```console
$ arky check --format json > report.json
```

Three formats exist: text for a person at a terminal, JavaScript Object Notation
(JSON) for another program, and markdown for a report to commit or to paste into
a review. A build step wants JSON; a step that publishes a report may want
markdown. See [arky check](../reference/cli/check.md).

## Step 2: choose what makes the build fail

```console
$ arky check --format json --fail-on warning > report.json
```

`--fail-on` takes `error`, `warning`, or `any`, and defaults to `error`. It
changes only the exit status; the findings printed are the same either way. See
[Lint a vault](lint-a-vault.md) for reading them.

## Step 3: gate on the exit code

| Code | Meaning |
|---|---|
| 0 | Success, no issues. |
| 1 | Validation errors found. |
| 2 | Warnings only, when the run was set to fail on warnings. |
| 3 | Configuration error. |
| 4 | File system error. |
| 5 | Language-model provider error. |

Three things a build step should know. Code 2 arises only when the failure level
is set to warnings, so a job that leaves the default in place never sees it. Code
5 cannot come from a validation run, which uses no language model; it belongs to
the command that classifies inbox captures. And the table is written for the
validation run, so a job that runs any other command is gating on an unspecified
code. See [Exit codes](../reference/exit-codes.md).

## Step 4: consume the report

The JSON object carries one run: the tool version, the time of the run, the root
that was scanned, the count of files and of assets scanned, the findings grouped
into errors, warnings, and information, and a summary holding the count at each
level and the number of files that passed. Each finding names the rule that
produced it, the path it concerns, its message, and whether it can be repaired.
The object is shown in full on
[Output formats](../reference/output-formats.md).

Read the JSON rather than the text. *Whether the text output is a contract
another program may parse, or a rendering that may change, is not yet
specified.*

*Whether a missing frontmatter block is repairable is not yet specified: the
sample report marks it repairable, and the repair run's own list does not include
creating frontmatter.*

## Step 5: scope the run to what matters

```console
$ arky check src/work/ --rule frontmatter --rule type
$ arky check --ignore templates
```

A path argument limits the run to part of a vault, `--rule` limits it to named
rules, and `--ignore` adds patterns to skip on top of the configuration file's
ignore list. Rules that should always be off belong in the configuration file
rather than on the command line — see [Configuration](../reference/config.md).

## Not yet specified

- How a build machine obtains arky: no published package, release artifact, or
  container image is defined, so no workflow file can be written yet.
- Whether arky ships an example workflow, a pre-commit hook, and shell
  completions.
- Exit codes for the other commands: listing, retrieving, linting, linking,
  initializing, searching, writing, and querying have none attached.
- What a query that matched nothing returns, which decides whether a query can be
  used as a test in a script without inspecting its output.
- What a refused transaction returns. A structural operation moves nothing at all
  if any entry is refused.
- Whether a repair run has codes of its own.

See [Design status](../design-status.md) for the open questions here.

## Related

- [Exit codes](../reference/exit-codes.md) — the table and what is unstated about
  it.
- [Output formats](../reference/output-formats.md) — the JSON object and the
  other two formats in full.
- [Lint a vault](lint-a-vault.md) — the same run at a terminal.
- [arky check](../reference/cli/check.md) — the option table.
- [Configuration](../reference/config.md) — settings that hold for every run.
