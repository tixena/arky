# arky trace

`arky trace` prints the chain of inherited properties for one file: which
folder index files above it declare properties, what those properties are, and
whether the file carries them.

## Synopsis

```console
$ arky trace <FILE>
```

`<FILE>`, the file whose inheritance chain to print, is required.
*Options are not yet specified.*

## Behavior

The run walks from the root of a corpus down to the file. For each folder on the
way it prints that folder's index file and the properties its `inherits:` block
declares, marking which the folder took from its own parent and which it adds.
For the file itself it prints the properties it must carry and the properties it
does carry, so the two can be compared.

A folder's `index.md` declares an `inherits:` block whose properties apply to
every file in that folder and its subfolders; each level adds to what it
inherited; a child file may add properties of its own but never drops an
inherited one, and a differing value for an inherited property fails validation.

## Example

```console
$ arky trace src/work/people/team/teammate.md
Inheritance chain for: src/work/people/team/teammate.md

src/index.md
  └─ (no inherits block)

src/work/index.md
  └─ inherits:
       area: work

src/work/people/index.md
  └─ inherits:
       area: work (from parent)

src/work/people/team/index.md
  └─ inherits:
       area: work (from parent)
       role: employee

teammate.md
  └─ MUST have: area=work, role=employee
  └─ HAS: area=work, role=employee ✓
```

The check mark on the last line says the two agree.

## Not yet specified

- What the last line reads when the two do not agree.
- Whether the report is offered in any other format. The three formats of the
  validation run are written for that run alone.
- Whether inheritance keeps the `inherits:` shape this report displays.
- The exit code when the file is missing a property it must carry.

See [Design status](../../design-status.md) for the open questions here.

## Related

- [Inheritance](../../concepts/inheritance.md) — the mechanism this displays.
- [arky check](check.md) — the `inheritance` rule.
- [arky init](init.md) — creates a missing folder index file.
- [Command-line interface](index.md) — global options and the command table.
