# arky store

`arky store` holds the commands on the saved stores of
[`arky.toml`](../config.md), each a store string kept under a name as
`[stores.<name>]` and named `arky://<name>` wherever a store is given. Its one
command is `arky store list`.

## arky store list

`arky store list` lists the saved stores in use from the current folder: each
name, its URL as written, and the `arky.toml` it comes from.

### Synopsis

```console
$ arky store list [-c, --config <FILE>]
```

### Options

| Option | What it takes |
|---|---|
| `-c`, `--config <FILE>` | Lists the saved stores of that one file only, in place of `ARKY_CONFIG` and of the `arky.toml` files from the current folder up. |

`arky store --help` and `arky store list --help` print help.

### Behavior

The saved stores in use are those of every `arky.toml` from the current folder up
to the root of the file system. A name that two files define is the nearer
file's, as `arky://<name>` opens it, and is listed once, from that file; nothing
lists the farther definition. With `--config`, or `ARKY_CONFIG` when `--config`
names none, the saved stores are those of that one file.

### Output

One line per saved store, sorted by name: its name, its URL as written, and the
`arky.toml` it comes from, as an absolute path. The columns are two spaces apart
and padded so they line up. The URL keeps each `${NAME}` as written, so no
variable's value, and no secret, shows. No saved store prints nothing, and the
exit code is 0.

```console
$ export MINIO_USER=me MINIO_PASSWORD=p@ss/word
$ cat ~/arky.toml
[stores.vault_b]
url = "file:b"
$ cat ~/work/arky.toml
[stores.vault_c]
url = "minio+https://minio.example.com/vault-c"
$ cd ~/work/vault
$ cat arky.toml
[stores.vault_a]
url = "minio+https://${MINIO_USER}:${MINIO_PASSWORD}@minio.example.com/vault-a"
$ arky store list
vault_a  minio+https://${MINIO_USER}:${MINIO_PASSWORD}@minio.example.com/vault-a  /home/me/work/vault/arky.toml
vault_b  file:b                                                                   /home/me/arky.toml
vault_c  minio+https://minio.example.com/vault-c                                  /home/me/work/arky.toml
$ arky store list -c ~/arky.toml
vault_b  file:b  /home/me/arky.toml
```

An `arky.toml` that does not read, or one of whose keys breaks a rule, exits with
code 3, on one line that names the file:

```console
$ cat arky.toml
[stores.vault_a]
path = "notes"
$ arky store list; echo "exit $?"
arky: /home/me/work/vault/arky.toml: [stores.vault_a] has no key path; it takes url
exit 3
```

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success, including no saved store at all. |
| 2 | The command line is wrong: an unknown option or command. |
| 3 | The configuration is wrong: an `arky.toml` that does not read, or one of whose keys breaks a rule, a file that `--config` or `ARKY_CONFIG` names and that does not exist, or an `ARKY_CONFIG` that is not UTF-8. |
| 4 | A failure to write standard output, or to read the current folder. |

## Related

- [Configuration](../config.md) — `arky.toml`, its saved stores, and the walk up
  the tree.
- [Command-line interface](index.md) — store strings and the store each command
  uses.
