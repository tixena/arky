# Lint a vault

Linting is one run: arky walks a tree of markdown files, applies its rules, and
reports what it found at three levels of severity. The run never modifies a file;
only the repair run writes.

## Step 1: choose what to scan

```console
$ arky check
$ arky check src/work/
```

With no path, the run scans the root named in the configuration file; with a
path, it scans that path. Folders in the configuration's ignore list are skipped
either way, and the defaults skip Obsidian's own folder, the assets folder, and
the templates folder — see [Configuration](../reference/config.md). The same run
is also spelled `arky lint folder/`; every option below is written for
`check`.

## Step 2: read the report

The default output is text, grouped by severity.

```console
$ arky check
Scanning src/...
Found 127 markdown files, 45 assets

ERRORS (3):
  ✗ src/notes/random.md
    Missing frontmatter

  ✗ src/work/new/
    Missing index.md

  ✗ src/work/projects/old.md
    archived=true but not in archive/

WARNINGS (2):
  ⚠ src/product/notes/planning.md
    Contains 5 embedded task items

  ⚠ src/work/assets/screenshot.png
    Not referenced by any file

INFO (4):
  ○ src/inbox/note1.md - Needs processing
  ○ src/inbox/note2.md - Needs processing
  ○ src/inbox/note3.md - Needs processing
  ○ src/inbox/note4.md - Needs processing

─────────────────────────────────────
Summary: 3 errors, 2 warnings, 4 info
         122 files OK
```

- **Errors** break a rule: no frontmatter, frontmatter that does not parse, a
  missing or unrecognized `type`, a missing `created` date, a missing inherited
  property, a folder with no index file, a file whose type its folder does not
  allow, and a file marked as archived that is not in an archive folder.
- **Warnings** ask for a judgment: an asset no markdown file references, and a
  note holding several independent checkbox items that would be better split into
  task files. The threshold for the second is a configuration key.
- **Information** is the inbox: files that still have no frontmatter and are
  waiting to be classified.

Every rule, with its message and whether a repair can address it, is on
[Validation rules](../reference/validation-rules.md). Two other formats carry the same run: JavaScript Object Notation (JSON) for
another program to read, and markdown for a report to commit or paste. Select one
with `--format json` or `--format markdown`, or set the default in the
configuration file's `[output]` table; both appear in full on
[Output formats](../reference/output-formats.md). JSON is the one to reach for in
a build pipeline — see [Run arky in continuous integration](run-in-ci.md).

## Step 3: narrow the run

```console
$ arky check --rule frontmatter --rule type
$ arky check src/work/ --ignore templates
```

`--rule` restricts the run to named rules and may be given more than once, and
`--ignore` adds patterns to skip on top of the configuration file's ignore list.
A rule that should always be off belongs in that file instead. *Whether `--rule`
takes a rule name, a rule category, or either is not yet specified, and neither
is what an `--ignore` pattern may contain.*

## Step 4: decide when the run should fail

```console
$ arky check --fail-on warning
```

`--fail-on` takes `error`, `warning`, or `any`, and defaults to `error`. It
decides the process exit status and nothing else: the same findings are printed
either way. The run exits 0 when nothing reached the configured level, 1 when
validation errors were found, 2 when there were no errors but warnings were found
and the run was set to fail on warnings, 3 on a configuration error, and 4 on a
file system error.

*What the run returns when the level is `any` and the only findings are
informational is not yet specified.* See
[Exit codes](../reference/exit-codes.md).

## Step 5: repair what can be repaired

```console
$ arky fix --dry-run
$ arky fix src/work/ --rule inheritance
```

The repair run addresses what needs no human decision: adding a missing `created`
date from the file's modification time, adding missing inherited properties,
creating missing folder index files, and moving a file marked as archived into
the archive folder. It refuses a missing `type`, an invalid `type`, and the
classification of an inbox file. See [arky fix](../reference/cli/fix.md).

## What this run becomes

The rules above are a fixed catalog — frontmatter required everywhere with the
inbox as the lone exception, `type` and `created` as fixed requirements, a
built-in list of resource types. Each becomes a consequence of what a shape says
once shapes exist. *What the linter prints when a document fails a shape is not
yet specified.* See [Define a shape](define-a-shape.md) and
[Design status](../design-status.md).

## Related

- [arky check](../reference/cli/check.md) — the full option table.
- [Validation rules](../reference/validation-rules.md) — every rule, its level,
  its message, and whether it is repairable.
- [Run arky in continuous integration](run-in-ci.md) — this run as a build step.
