# Changelog

All notable changes to phig are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and releases follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Ref scope flags `--all`, `--branches`, `--remotes`, and `--tags` for `phig`,
  `phig log`, and `phig snapshot log`, so history can span remote-tracking
  branches and tags instead of only HEAD's ancestry. Naming a revision unions it
  with the scope; omitting one lets the scope define the walk. A scope also
  selects topological ordering, and commands that never walk history reject the
  flags as a usage error.

### Changed

- The log header names an active ref scope rather than pinning it to a single
  object.

## [1.1.1] - 2026-08-19

### Fixed

- Isolated shell-installer tests with an explicit temporary `CARGO_HOME` so
  running the release suite can never shadow a developer's installed `phig`.

## [1.1.0] - 2026-08-19

### Added

- Explicit `auto`, `unicode`, and `ascii` glyph policies with coherent graph,
  selection, divider, and overlay fallbacks.
- Style-aware golden coverage for every primary view, adaptive breakpoints,
  overlays, themes, remapped keys, ASCII, and monochrome rendering.
- Typed per-session rendering options for themes, color, dates, glyphs, and
  effective key labels while retaining the 1.0 compatibility entry points.

### Changed

- Refined the interface around native terminal backgrounds, marker-led
  selection, thin functional dividers, quieter contextual footers,
  width-prioritized headers, and compact adaptive overlays.
- Reorganized the application core, terminal adapter, renderer, and
  configuration system into small responsibility-based modules with stable
  public façades.
- Made all width budgeting terminal-cell aware and kept commit subjects useful
  across narrow layouts and every date mode.
- Made the benchmark fixture self-identifying so performance gates cannot
  silently reuse an unrelated repository.

### Fixed

- Honored semantic key remaps consistently in help and text-entry overlays,
  including bracketed paste in the changed-file picker.
- Rejected stale cross-view Git responses and prevented hidden preview focus in
  narrow layouts.
- Kept page movement aligned with visible compare, status-diff, and truncated
  patch rows.
- Cleared stale previews and reported honest positions and footer state for
  empty refs, status, blame, tree, and stash views.
- Normalized uppercase key bindings and rejected the reserved `Ctrl-C` binding
  with an actionable configuration error.

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

[Unreleased]: https://github.com/phall1/phig/compare/v1.1.1...HEAD
[1.1.1]: https://github.com/phall1/phig/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/phall1/phig/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/phall1/phig/releases/tag/v1.0.0
