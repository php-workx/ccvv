# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.5.0] - 2026-03-08

### Added
- Content-type pre-scan classifier — pipeline now detects shell blocks, lists,
  code, and prose before running transform stages
- Shell block handling — standalone shell commands skip paragraph compaction and
  get whole-line backtick wrapping
- Precomputed candidate logic for double Cmd+C — cleaning starts on first copy,
  applied instantly on second
- CLI binary symlink (`ccvv`) bundled inside the macOS app
- Fixture-based test suite with 22 real-world clipboard captures and idempotency
  validation
- Beta version stamping for local dev builds

### Changed
- Structural transform refines code-fencing heuristics with scoring system
- Table extraction requires more columns for comma-delimited detection
- Paragraph handling uses raw line lengths and terminal width for smarter
  line joining
- App version is read dynamically from Info.plist instead of hardcoded constant
- Reduced cognitive complexity in `classify()`, `compact_paragraph()`, and
  `try_code_fence()` via helper extraction

### Fixed
- List continuation check allows equal indentation (not just deeper)
- Shell command lines no longer double-wrapped in backticks

### Removed
- Unused `ccvv_code_passthrough()` function and its tests
- Old Makefile (replaced by justfile)
