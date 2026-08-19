# Changelog

All notable changes to phig are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and releases follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.0.0] - 2026-08-19

### Added

- Fast asynchronous commit history with graph lanes, search, pagination, and
  diff previews.
- Commit detail, explicit parent navigation, searchable changed-file picker,
  file/hunk movement, and full-screen patches.
- Explicit merge-base `compare` and exact-endpoint `diff` workflows with marked
  endpoints and mode switching.
- Read-only refs, status/conflict, revision tree/blob, blame, and stash views.
- Strict XDG TOML configuration, semantic key remapping, themes, color/date
  policy, diff options, and bounded resource limits.
- Interactive commit/ref/file/hunk/line/comparison selection with a controlling
  terminal and clean stdout.
- Bounded deterministic `phig/1` JSON snapshots, schema, version envelope,
  pagination offsets, shell completions, and complete manual pages.
- Alternate-screen opt-out, mouse opt-in, zero-config OSC 52 copying with an
  explicit off mode, suspend/resume, signal restoration, and adaptive narrow,
  stacked, and wide right-side preview layouts.
- Explicit installer-aware update checks and updates.
- GitHub Release automation for macOS and Linux ARM64/x86-64 archives,
  SHA-256 checksums, attestations, shell installer, and Homebrew tap formula.

### Security

- Repository-controlled commands, prompts, hooks, external diff/textconv,
  pagers, lazy object fetching, replacements, and terminal control sequences
  are disabled or sanitized on inspection paths.

[Unreleased]: https://github.com/phall1/phig/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/phall1/phig/releases/tag/v1.0.0
