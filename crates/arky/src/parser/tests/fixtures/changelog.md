# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

### Added

- `parse` returns byte ranges for every node.
- Support for [TOML frontmatter](docs/frontmatter.md).

## [2.3.0] - 2026-09-30

### Changed

- **Breaking:** `Mapping` keeps its keys in the order written.

### Fixed

- A fence inside a list item no longer ends the list. ([#42](https://example.com/issues/42))

## [2.2.0] - 2026-09-01

Maintenance release.

[Unreleased]: https://example.com/compare/2.3.0...HEAD
[2.3.0]: https://example.com/compare/2.2.0...2.3.0
[2.2.0]: https://example.com/releases/tag/2.2.0
