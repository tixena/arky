# Output formats

A validation run prints its result in one of three formats: text, the default,
for a person at a terminal; JavaScript Object Notation (JSON), for another
program; and markdown, to be committed or pasted as a report. `--format` chooses
one, and its default comes from the `[output]` table of the
[configuration](config.md). JSON output and [exit codes](exit-codes.md) together
are what make arky friendly to
[continuous integration](../guides/run-in-ci.md).

## Text

Findings grouped by level, each with its path and message, closing with a count
per level and a count of the files that passed. Color is on by default; the
`color` key and the `NO_COLOR` environment variable each turn it off.

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

## JSON

One object per run: the tool version, the time of the run, a corpus root, the
counts of what was scanned, the findings grouped into errors, warnings, and
information, and a summary. Each finding names the [rule](validation-rules.md)
that produced it, its path, its message, and whether it can be repaired.

```json
{
  "version": "0.1.0",
  "timestamp": "2025-12-23T10:30:00Z",
  "vault_root": "src",
  "files_scanned": 127,
  "assets_scanned": 45,
  "results": {
    "errors": [
      {
        "rule": "frontmatter-required",
        "path": "src/notes/random.md",
        "message": "Missing frontmatter",
        "fixable": true
      }
    ],
    "warnings": [],
    "info": []
  },
  "summary": {
    "errors": 3,
    "warnings": 2,
    "info": 4,
    "ok": 122
  }
}
```

## Markdown

A report document: a summary table of counts, then a section per level in which
findings are grouped by message.

```markdown
# Arky Report

Generated: 2025-12-23 10:30:00

## Summary

| Level | Count |
|-------|-------|
| Errors | 3 |
| Warnings | 2 |
| Info | 4 |
| OK | 122 |

## Errors

### Missing frontmatter
- `src/notes/random.md`

### Missing index.md
- `src/work/new/`

...
```

## Other output

The three formats belong to the validation run. Three commands write output of
their own, in one form each: the inheritance chain of
[arky trace](cli/trace.md), the proposal screen of
[arky inbox](cli/inbox.md), and the export document of
[arky export](cli/export.md). The
[Model Context Protocol server](mcp/index.md) returns the report as data.

## Not yet specified

- What `arky ls`, `arky get`, `arky link`, `arky rm`, and `arky init`
  print.
- What a query prints, and what a projection produces. See
  [Query grammar](query-grammar.md).
- What the export document carries under each of its two options, and the
  warning and information sections of the markdown report.
- Whether the text format is a contract another program may parse.

See [Design status](../design-status.md) for the open questions around output.
