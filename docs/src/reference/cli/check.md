# arky check

`arky check` is the validation run: it reads a corpus, applies rules to it, and
reports what it finds. It never modifies a file; repairing is
[`arky fix`](fix.md).

## Synopsis

```console
$ arky check [PATH]
```

`[PATH]` is the path to validate; it is optional, and its default comes from the
configuration file.

## Options

| Option | Effect |
|---|---|
| `--format <FORMAT>` | Output format: `text`, `json`, or `markdown`. Default: `text`. |
| `--fail-on <LEVEL>` | Exit with a non-zero status on `error`, `warning`, or `any`. Default: `error`. |
| `--rule <RULE>` | Run only the named rule or rules. May be given more than once. |
| `--ignore <PATTERN>` | Additional patterns to ignore, beyond the ignore list in the configuration file. |

## Behavior

A run walks the configured corpus root, or the given path, skips what the ignore
list excludes, applies the enabled rules, and reports every finding at one of
three levels: error, warning, or information. Every rule can be turned on or off
in the configuration file and narrowed further by `--rule`; `--fail-on` decides
whether the exit status is non-zero.

The rules come from schemas rather than a fixed catalog. A shape may be declared
for a whole markdown document or a sub-part of one; a schema has a strict mode
that rejects content outside it and a loose mode that lets it through, and no
stripping mode. Resource types are declared by remargin, per corpus, and a type
may extend another; arky holds only the means to enforce them.

## The rules

| Rule | Category | What it reports |
|---|---|---|
| `frontmatter-required` | Frontmatter | A markdown file with no frontmatter block. An error. The inbox is exempt when the configuration allows raw files there. |
| `frontmatter-valid` | Frontmatter | A frontmatter block that is not syntactically valid, reported with the line at which parsing failed. An error. |
| `type-required` | Properties | A file with no `type` property. An error. |
| `type-valid` | Properties | A `type` value that is not one of the defined resource types. An error; the message lists the valid types. |
| `created-required` | Properties | A file with no `created` date property. An error, and repairable from the file's modification time. |
| `inheritance` | Properties | A file missing a property that a parent folder's index file declares under `inherits`. An error naming the expected value and the index file that declares it; repairable. |
| `index-required` | Structure | A folder with no `index.md`, other than an ignored one. An error; repairable by creating the file with the inherited properties. |
| `allowed-types` | Structure | A file whose `type` is not in the `allows.types` list declared by its folder's index file. An error naming the allowed types and the index file that declares them. |
| `archive-placement` | Archive | A file carrying `archived: true` that is not inside an archive folder. An error; repairable by moving it. |
| `orphaned-assets` | Assets | An asset — an image, a document in Portable Document Format, and so on — that no markdown file references. A warning. It can be slow on a large corpus. |
| `embedded-tasks` | Tasks | A note holding several independent checkbox items, better split into task files. A warning, with a configurable threshold. |
| `inbox-raw` | Inbox | An inbox file with no frontmatter, which needs processing. Reported at the information level only. |

## Examples

```console
$ arky check
$ arky check src/work/
$ arky check --format json > report.json
$ arky check --rule frontmatter --rule type
```

The text output:

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

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success, no issues. |
| 1 | Validation errors found. |
| 2 | Warnings only, when `--fail-on warning` was given. |
| 3 | Configuration error. |
| 4 | File system error. |
| 5 | Language-model provider error. |

Code 5 cannot arise from a validation run, which uses no language model.

## Not yet specified

- The command's name: `arky check` and `arky lint folder/` both stand for
  this run.
- Whether `--rule` takes a rule name, a rule category, or either. The example
  above passes `frontmatter` and `type`, which are categories.
- How each rule follows from a schema once the
  [shape language](../shape-language.md) is defined. The catalog above is written
  against fixed requirements that schemas replace.

See [Design status](../../design-status.md) for the open questions here.

## Related

- [Validation rules](../validation-rules.md) — every rule with its message text.
- [Shape language](../shape-language.md) — the schemas the rules follow from.
- [arky fix](fix.md) — the repair run.
- [Output formats](../output-formats.md) — the same run as JSON and as markdown.
- [Exit codes](../exit-codes.md) — the code table.
- [Lint a vault](../../guides/lint-a-vault.md) — the how-to.
- [Command-line interface](index.md) — global options and the command table.
