# arky fix

`arky fix` repairs the findings a validation run reports that can be repaired
without a person deciding anything. It is the only command that changes files
without being told what content to write.

## Synopsis

```console
$ arky fix [PATH] [--store <STORE>]
```

`[PATH]` is the path to repair; it is optional. The repair of expired locks is
built, with `--store`; every other repair and option on this page is not built.

## Options

| Option | Effect |
|---|---|
| `-s`, `--store <STORE>` | The store `PATH` is in, in place of the file system: `minio:<bucket>`, `minio:<bucket>/<prefix>`, `s3:<bucket>` or `s3:<bucket>/<prefix>`. |
| `--dry-run` | Show what would be repaired without changing any file. Not built. |
| `--rule <RULE>` | Repair only the findings of the named rule or rules. Not built. |
| `--interactive` | Ask before each repair. Not built. |

## What it repairs

Built:

- Remove each lock that a save left behind and that has expired.

Not built:

- Add a missing `created` date, taken from the file's modification time.
- Add missing properties that a parent folder's index file declares as inherited.
- Create missing `index.md` files.
- Move a file carrying `archived: true` into the archive folder.

Three findings need a person to decide and are never repaired: a missing `type`
property, an invalid `type` value, and the classification of an inbox file.

## Expired locks

A save holds the file `<file>.arky-lock` beside a document while it writes, or
the object `<key>.arky-lock` in a MinIO or S3 bucket. A save that stops before it
gives the lock back, such as an arky that is killed, leaves it behind. The next
save of that document removes it once it has expired, but the lock of a document
nobody saves again stays.

`arky fix` removes each lock under `PATH` that is older than `expires_seconds`
of the `[lock]` table of [`arky.toml`](../config.md), and only while it is still
the lock it found: a lock that another save replaced in the meantime stays. A lock
that has not expired stays. A file whose name ends in `.lock` but not in
`.arky-lock`, such as `Cargo.lock`, is never touched.

`PATH` is a folder, the current folder when it is left out. With a MinIO or S3
store it is a path of the store as `arky write --store` takes it, and the whole
prefix when it is left out.

| `--store` | What it does |
|---|---|
| none | Repairs the file system below `PATH`. A `PATH` that is not a folder exits with code 2. |
| `minio:<bucket>[/<prefix>]` | Repairs the objects of the bucket under `PATH`. |
| `s3:<bucket>[/<prefix>]` | Repairs the objects of the AWS S3 bucket under `PATH`. |
| `mongodb:<database>/<collection>` | Exits with code 2: a MongoDB store keeps no locks. |

An S3 store reads the variables that [`arky write --store s3:`](write.md#stores)
reads, except `ARKY_S3_KMS_KEY_ARN`: fix creates no bucket.

| Variable | What it holds |
|---|---|
| `ARKY_S3_REGION` | The AWS region of the bucket, such as `eu-west-1`. Required, with no default. |
| `AWS_ACCESS_KEY_ID` | The access key every request is signed with. Required. |
| `AWS_SECRET_ACCESS_KEY` | The secret key every request is signed with. Required. No output and no error shows it. |

A missing variable exits with code 3, on one line that names it. A prefix that is
not a store path exits with code 2. AWS that cannot be reached, or that refuses
the keys, exits with code 4.

```console
$ export ARKY_S3_REGION=eu-west-1 AWS_ACCESS_KEY_ID=AKIA... AWS_SECRET_ACCESS_KEY=...
$ arky fix --store s3:acme-notes/vault
notes/report.md.arky-lock
$ unset ARKY_S3_REGION; arky fix --store s3:acme-notes/vault; echo "exit $?"
arky: ARKY_S3_REGION is not set: it holds the AWS region of the S3 bucket
exit 3
```

### Output

The path of each lock it removed, one per line on standard output, sorted. Nothing
when no lock had expired.

- On the file system, the lock's path below the folder searched, after `PATH` as
  the command line gave it, or alone when `PATH` was left out.
- In a MinIO or S3 store, the lock's path in the store, as `arky write --store`
  names files.

```console
$ ls notes
report.md  report.md.arky-lock  media
$ arky fix notes
notes/report.md.arky-lock
$ echo $?
0
$ arky fix notes
$ arky fix --store minio:arky/vault
notes/report.md.arky-lock
$ arky fix --store mongodb:notes/docs
arky: mongodb:notes/docs keeps no locks: fix takes the file system, --store minio:<bucket> or --store s3:<bucket>
$ echo $?
2
```

An error is one line on standard error: `arky: `, the path or the store as the
command line gave it, and what failed.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success: each lock that had expired is removed. |
| 2 | The command line is wrong: a `PATH` that is not a folder, a `--store mongodb:` value, or a `--store` value of any other shape. |
| 3 | The configuration is wrong: `arky.toml`, a variable the MinIO or S3 store reads, or a MinIO or S3 store that is not built into this arky. |
| 4 | The store failed: a MinIO server or AWS that cannot be reached or refuses the keys, or a file system error. A failure to write standard output is code 4 too. |

## Behavior

Files are never modified without an explicit repair. [`arky check`](check.md)
reports; only this command writes.

## Examples

```console
$ arky fix notes
$ arky fix --store minio:arky/vault
$ arky fix --store s3:acme-notes/vault
```

Not built:

```console
$ arky fix --dry-run
$ arky fix src/work/ --rule inheritance
```

## Not yet specified

- What `--dry-run` prints, whether `--interactive` and `--dry-run` may be
  combined, and what the prompt offers in interactive mode.
- Whether a repair run validates first, whether it reports what it repaired and
  in which format, and whether a failed repair halts the run.
- Whether a repair is a [change object](../change-objects.md), replayable and
  unappliable like any other write.
- Whether moving an archived file rewrites the inbound links to it, as
  [structural operations](structural-operations.md) do.
- Which repairs follow from a schema once the
  [shape language](../shape-language.md) is defined.
- Output, and the exit codes for the repairs that are not built: a run that
  repaired everything, one that repaired some findings and left others, and a
  repair that failed.

See [Design status](../../design-status.md) for the open questions here.

## Related

- [arky check](check.md) — the validation run this repairs the findings of.
- [Validation rules](../validation-rules.md) — which rules are repairable.
- [Shape language](../shape-language.md) — the schemas that decide what is
  repairable.
- [Change objects](../change-objects.md) — the transactional model.
- [Command-line interface](index.md) — global options and the command table.
