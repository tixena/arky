<p align="center">
  <img src="logo.svg" width="120" alt="logo">
</p>

# quill

[![build](https://img.shields.io/badge/build-passing-green.svg)](https://example.com/ci)
[![docs](https://img.shields.io/badge/docs-online-blue.svg)](https://example.com/docs)

quill reads markdown files into a **typed tree**.

## Install

```console
$ cargo install quill
```

## Features

- Frontmatter in two syntaxes
  - TOML between `+++` lines
  - YAML between `---` lines
- A body grammar that is context-free
- Byte ranges on every node

## Comparison

| Tool  | Frontmatter | Tree |
|-------|-------------|------|
| quill | TOML, YAML  | yes  |
| other | YAML        | no   |

## Contributing

Open an issue first. See [CONTRIBUTING](CONTRIBUTING.md).
